#!/usr/bin/env bash
# ceiling-check.sh — L1 LIVE CEILING GUARD. The safety floor of the token-aware scheduler.
#
# THE FIX for REVIEW_TOKEN_GUARD_FAILURE: the old guard watched the stale on-disk proxy
# ~/.claude/state/i2p-cost/session.json (written only at the Stop hook) and never fired.
# This reads the LIVE signal the harness already delivers on the hook stdin payload —
# the same fields the status line renders (i2p-statusline.sh): the rolling rate-limit
# windows. It is pure arithmetic: no model, no network, deterministic, FAILS CLOSED.
#
#   <live-payload-json> | ceiling-check.sh [--headroom PCT] [--window five_hour|seven_day|both]
#
# Reads the live payload on STDIN:
#   .rate_limits.five_hour.used_percentage   (0..100, may be fractional)
#   .rate_limits.five_hour.resets_at         (unix epoch)
#   .rate_limits.seven_day.used_percentage
#   .rate_limits.seven_day.resets_at
#
# Verdict (one-line JSON on stdout) + exit code — the contract every caller branches on:
#   exit 0   CLEAR      every requested window is below its ceiling → safe to spawn the next wave
#   exit 10  HALT       a requested window reached the ceiling → stop spawning, checkpoint, pause
#   exit 20  NO_SIGNAL  the payload lacked a usable rate-limit number → treat AS HALT (fail closed)
#
# ceiling = 100 - headroom. breach when used_percentage >= ceiling. Default headroom 15 (stop at 85%).
# Honours post-mortem Rule: when in doubt, do NOT proceed. Every unknown path returns NO_SIGNAL.
set -uo pipefail

headroom=15
window="both"

while [ $# -gt 0 ]; do
  case "$1" in
    --headroom) headroom="${2:-}"; shift 2 ;;
    --window)   window="${2:-}";   shift 2 ;;
    --headroom=*) headroom="${1#*=}"; shift ;;
    --window=*)   window="${1#*=}";   shift ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) shift ;;
  esac
done

# Validate headroom is an integer 0..100 — a malformed guard is a dangerous guard, fail closed.
is_int() { case "$1" in (''|*[!0-9]*) return 1 ;; (*) return 0 ;; esac; }
if ! is_int "$headroom" || [ "$headroom" -gt 100 ]; then
  printf '{"verdict":"NO_SIGNAL","reason":"bad-headroom","headroom":"%s"}\n' "$headroom"
  exit 20
fi
case "$window" in five_hour|seven_day|both) ;; *) window="both" ;; esac

payload=""; [ -t 0 ] || payload="$(cat 2>/dev/null || true)"

# extract_pct <window-key> → echoes the used_percentage, or empty if absent/unparseable.
# Prefers jq; falls back to a deterministic regex so a missing jq still yields a real number
# (and only a genuinely absent field yields empty → NO_SIGNAL).
extract_pct() {
  local key="$1" out=""
  if command -v jq >/dev/null 2>&1; then
    out="$(printf '%s' "$payload" | jq -r --arg k "$key" \
      '(.rate_limits[$k].used_percentage // empty)' 2>/dev/null)"
  fi
  if [ -z "$out" ]; then
    # Fallback: pull the window object, then its used_percentage number.
    out="$(printf '%s' "$payload" \
      | tr -d '\n' \
      | sed -n "s/.*\"${key}\"[[:space:]]*:[[:space:]]*{\([^}]*\)}.*/\1/p" \
      | sed -n 's/.*"used_percentage"[[:space:]]*:[[:space:]]*\([0-9.]*\).*/\1/p')"
  fi
  printf '%s' "$out"
}
extract_reset() {
  local key="$1" out=""
  if command -v jq >/dev/null 2>&1; then
    out="$(printf '%s' "$payload" | jq -r --arg k "$key" \
      '(.rate_limits[$k].resets_at // empty)' 2>/dev/null)"
  fi
  if [ -z "$out" ]; then
    out="$(printf '%s' "$payload" | tr -d '\n' \
      | sed -n "s/.*\"${key}\"[[:space:]]*:[[:space:]]*{\([^}]*\)}.*/\1/p" \
      | sed -n 's/.*"resets_at"[[:space:]]*:[[:space:]]*\([0-9]*\).*/\1/p')"
  fi
  printf '%s' "$out"
}

ceiling=$(( 100 - headroom ))

# breaches <pct> → 0 (true) if pct >= ceiling. Float-safe via awk.
breaches() {
  awk -v p="$1" -v c="$ceiling" 'BEGIN{ exit !(p+0 >= c+0) }'
}

# Evaluate one window. Sets globals: ev_pct ev_reset ev_state (CLEAR|HALT|NO_SIGNAL).
eval_window() {
  local key="$1"
  ev_pct="$(extract_pct "$key")"
  ev_reset="$(extract_reset "$key")"
  if [ -z "$ev_pct" ]; then ev_state="NO_SIGNAL"; return; fi
  # Must look like a number.
  case "$ev_pct" in (*[!0-9.]*|''|.) ev_state="NO_SIGNAL"; return ;; esac
  if breaches "$ev_pct"; then ev_state="HALT"; else ev_state="CLEAR"; fi
}

want_five=0 want_seven=0
case "$window" in
  five_hour) want_five=1 ;;
  seven_day) want_seven=1 ;;
  both)      want_five=1; want_seven=1 ;;
esac

worst="CLEAR" hit_window="" hit_pct="" hit_reset=""
note() {  # escalate to the worst verdict seen; NO_SIGNAL > HALT > CLEAR
  local st="$1" key="$2" pct="$3" reset="$4"
  case "$st" in
    NO_SIGNAL) worst="NO_SIGNAL"; hit_window="$key"; hit_pct="$pct"; hit_reset="$reset" ;;
    HALT) if [ "$worst" != "NO_SIGNAL" ]; then worst="HALT"; hit_window="$key"; hit_pct="$pct"; hit_reset="$reset"; fi ;;
    CLEAR) if [ "$worst" = "CLEAR" ] && [ -z "$hit_window" ]; then hit_window="$key"; hit_pct="$pct"; hit_reset="$reset"; fi ;;
  esac
}

if [ "$want_five" = 1 ]; then
  eval_window "five_hour"; note "$ev_state" "five_hour" "$ev_pct" "$ev_reset"
fi
if [ "$want_seven" = 1 ]; then
  eval_window "seven_day"; note "$ev_state" "seven_day" "$ev_pct" "$ev_reset"
fi

[ -z "$hit_pct" ] && hit_pct="null"
[ -z "$hit_reset" ] && hit_reset="null"
# Quote the pct/reset only when non-numeric.
fmt() { case "$1" in (''|null) printf 'null' ;; (*[!0-9.]*) printf '"%s"' "$1" ;; (*) printf '%s' "$1" ;; esac; }

printf '{"verdict":"%s","window":"%s","used_pct":%s,"ceiling":%s,"headroom":%s,"resets_at":%s}\n' \
  "$worst" "${hit_window:-none}" "$(fmt "$hit_pct")" "$ceiling" "$headroom" "$(fmt "$hit_reset")"

case "$worst" in
  CLEAR)     exit 0 ;;
  HALT)      exit 10 ;;
  NO_SIGNAL) exit 20 ;;
esac
