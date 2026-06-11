#!/usr/bin/env bash
# job-ledger.sh — L4 CHEAP-RESUME LEDGER. The reason a HALT is never wasteful.
#
# A paused job must resume by doing only what's LEFT — not by re-deriving expensive context and
# re-fanning 130 agents (the exact waste that made the original lockout cost real money). This is
# the durable record of done / remaining / failed units, the budget + off-peak terms the job runs
# under, pointers to already-derived context (so resume reuses it), and a checkpoint log of every
# pause with the LIVE ceiling snapshot that triggered it (what an alert-only cron reads).
#
# Project-local at <dir>/.i2p/jobs/<job-id>.json — mirrors .i2p/cost.json conventions.
#
#   job-ledger.sh init <dir> <job-id> <profile> <unit1,unit2,…> [budget_total] [headroom]
#   job-ledger.sh mark-done   <dir> <job-id> <unit>
#   job-ledger.sh mark-failed <dir> <job-id> <unit>
#   job-ledger.sh remaining   <dir> <job-id>           # newline-separated remaining units
#   job-ledger.sh pause   <dir> <job-id> <reason> [used_pct] [resets_at] [spent_tokens]
#   job-ledger.sh resume  <dir> <job-id>
#   job-ledger.sh set-offpeak <dir> <job-id> <start> <end> <tz_offset_min>
#   job-ledger.sh status  <dir> <job-id>               # the ledger JSON
#
# Atomic jq edits; pure state transitions. Needs jq. Stamp times come in as args (epoch) for testability.
set -uo pipefail

command -v jq >/dev/null 2>&1 || { echo "job-ledger: jq required" >&2; exit 2; }

cmd="${1:-status}"; dir="${2:-.}"; jid="${3:-}"
[ -n "$jid" ] || { echo "job-ledger: <job-id> required" >&2; exit 2; }
safe_jid="$(printf '%s' "$jid" | tr -c 'A-Za-z0-9._-' '_')"
LDIR="${dir%/}/.i2p/jobs"
LF="${LDIR}/${safe_jid}.json"

edit() {  # apply a jq program (with extra args) atomically to $LF
  local prog="$1"; shift
  [ -r "$LF" ] || { echo "job-ledger: no ledger at $LF" >&2; exit 2; }
  jq "$@" "$prog" "$LF" > "${LF}.tmp.$$" && mv -f "${LF}.tmp.$$" "$LF"
}

case "$cmd" in
  init)
    profile="${4:-}"; units_csv="${5:-}"; budget="${6:-0}"; headroom="${7:-15}"
    case "$budget"   in (''|*[!0-9]*) budget=0 ;; esac
    case "$headroom" in (''|*[!0-9]*) headroom=15 ;; esac
    mkdir -p "$LDIR" 2>/dev/null || { echo "job-ledger: cannot create $LDIR" >&2; exit 2; }
    # units_csv → JSON array (trim blanks, drop empties)
    units_json="$(printf '%s' "$units_csv" | jq -R 'split(",") | map(gsub("^\\s+|\\s+$";"")) | map(select(length>0))')"
    jq -n --arg jid "$jid" --arg profile "$profile" --argjson units "$units_json" \
          --argjson budget "$budget" --argjson headroom "$headroom" '
      { schema_version:"1.0", job_id:$jid, profile:$profile,
        budget_total:$budget, headroom_pct:$headroom,
        offpeak_window:null, state:"running",
        units:{ total:($units|length), done:[], remaining:$units, failed:[] },
        context_pointers:{}, checkpoints:[] }' > "${LF}.tmp.$$" && mv -f "${LF}.tmp.$$" "$LF"
    echo "job-ledger: initialised $LF ($(printf '%s' "$units_json" | jq 'length') units)"
    ;;

  mark-done)
    unit="${4:-}"; [ -n "$unit" ] || { echo "job-ledger: <unit> required" >&2; exit 2; }
    edit '
      .units.remaining -= [$u]
      | .units.failed   -= [$u]
      | .units.done = ((.units.done + [$u]) | unique)' --arg u "$unit"
    ;;

  mark-failed)
    unit="${4:-}"; [ -n "$unit" ] || { echo "job-ledger: <unit> required" >&2; exit 2; }
    edit '
      .units.remaining -= [$u]
      | .units.failed = ((.units.failed + [$u]) | unique)' --arg u "$unit"
    ;;

  remaining)
    [ -r "$LF" ] || { echo "job-ledger: no ledger at $LF" >&2; exit 2; }
    jq -r '.units.remaining[]?' "$LF"
    ;;

  pause)
    reason="${4:-unspecified}"; used="${5:-null}"; reset="${6:-null}"; spent="${7:-null}"; at="${8:-0}"
    numornull() { case "$1" in (''|null) echo null ;; (*[!0-9.]*) echo null ;; (*) echo "$1" ;; esac; }
    used="$(numornull "$used")"; reset="$(numornull "$reset")"; spent="$(numornull "$spent")"
    case "$at" in (''|*[!0-9]*) at=0 ;; esac
    edit '
      .state = "paused"
      | .checkpoints += [{ at:$at, reason:$reason, five_hour_pct:$used, resets_at:$reset,
                           spent_tokens:$spent, units_done:(.units.done|length),
                           units_remaining:(.units.remaining|length) }]' \
      --argjson at "$at" --arg reason "$reason" --argjson used "$used" \
      --argjson reset "$reset" --argjson spent "$spent"
    echo "job-ledger: paused ${jid} (reason=${reason})"
    ;;

  resume)
    edit '.state = "running"'
    echo "job-ledger: resumed ${jid}"
    ;;

  set-offpeak)
    start="${4:-22:00}"; end="${5:-08:00}"; tz="${6:-0}"
    case "$tz" in (''|*[!0-9-]*) tz=0 ;; esac
    edit '.offpeak_window = { start:$s, end:$e, tz_offset_min:$tz }' \
      --arg s "$start" --arg e "$end" --argjson tz "$tz"
    ;;

  set-pointer)
    key="${4:-}"; val="${5:-}"; [ -n "$key" ] || { echo "job-ledger: <key> <value> required" >&2; exit 2; }
    edit '.context_pointers[$k] = $v' --arg k "$key" --arg v "$val"
    ;;

  status)
    [ -r "$LF" ] || { echo "job-ledger: no ledger at $LF" >&2; exit 2; }
    cat "$LF"
    ;;

  *) echo "usage: job-ledger.sh {init|mark-done|mark-failed|remaining|pause|resume|set-offpeak|set-pointer|status} <dir> <job-id> …" >&2; exit 2 ;;
esac
