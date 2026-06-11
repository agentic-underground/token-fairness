#!/usr/bin/env bash
# scheduler.sh — the THIN DISPATCHER. Composes the pure helpers into one verdict; owns NO arithmetic.
#
# This is the seam the model/skill calls. The judgement (probe or ask? defer to off-peak?) is the
# model's; the numbers (ceiling breached? estimate? which window?) belong to the tested helpers.
# scheduler.sh only sequences them and names the verdict — keeping determinism in code and judgement
# in the model, exactly as required.
#
# Verdicts:
#   CONTINUE  safe to spawn the next throttled wave
#   PROBE     estimate confidence is LOW — run ONE unit, measure, re-estimate before fanning out
#   DEFER     not in the off-peak window (with --require-offpeak) — hold the job for the quiet hours
#   HALT      the live ceiling is reached — stop, checkpoint, wait for the window to reset
#   ASK       no usable live signal (fail closed) — surface to the user, never silently proceed
#
#   <live-payload> | scheduler.sh gate [--headroom N] [--window both] \
#                        [--require-offpeak --now EPOCH [--login EPOCH] [--start HH:MM] [--end HH:MM] [--tz-offset-min M]]
#   scheduler.sh preflight --profile P [--width N] [--name NAME] [--measured-unit-tokens M] [--history-tokens M]
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CEIL="${HERE}/ceiling-check.sh"
EST="${HERE}/scheduler-estimate.sh"
OW="${HERE}/offpeak-window.sh"
CALIBRATE="${HERE}/calibrate.sh"
DEFER_THRESHOLD=150000   # an est at/above this, while in PEAK, is deferred to off-peak

fmt_tok() { awk -v t="$1" 'BEGIN{ t=t+0; if(t>=1000000) printf "%.1fM", t/1000000; else if(t>=1000) printf "%dk", int(t/1000+0.5); else printf "%d", t }'; }

sub="${1:-}"; shift || true

case "$sub" in
  preflight)
    est_out="$("$EST" "$@" 2>/dev/null)"
    conf="$(printf '%s' "$est_out" | jq -r '.confidence // "low"' 2>/dev/null)"
    if [ "$conf" = "high" ]; then verdict="CONTINUE"; else verdict="PROBE"; fi
    printf '{"verdict":"%s","estimate":%s}\n' "$verdict" "${est_out:-null}"
    [ "$verdict" = "CONTINUE" ] && exit 0 || exit 3   # exit 3 = PROBE
    ;;

  plan)
    # Stamp ANY plan (class-based or fan-out) with cost + p95 convergence + a schedule decision.
    # Emits two human banner lines, then a machine JSON line (last line) for the caller to parse.
    p_now="" p_start="22:00" p_end="08:00" p_tz="" est_args=()
    while [ $# -gt 0 ]; do
      case "$1" in
        --now) p_now="${2:-}"; shift 2 ;;
        --start) p_start="${2:-}"; shift 2 ;;
        --end) p_end="${2:-}"; shift 2 ;;
        --tz-offset-min) p_tz="${2:-}"; shift 2 ;;
        *) est_args+=("$1"); shift ;;
      esac
    done
    est_out="$("$EST" "${est_args[@]}" 2>/dev/null)"
    est_total="$(printf '%s' "$est_out" | jq -r '.est_total // 0' 2>/dev/null)"
    band="$(printf '%s' "$est_out" | jq -r '.convergence.p95_band_pct // 60' 2>/dev/null)"
    tier="$(printf '%s' "$est_out" | jq -r '.convergence.tier // "SEEDING"' 2>/dev/null)"
    samples="$(printf '%s' "$est_out" | jq -r '.convergence.samples // 0' 2>/dev/null)"
    pname="$(printf '%s' "$est_out" | jq -r '.name // "plan"' 2>/dev/null)"
    case "$est_total" in (''|*[!0-9]*) est_total=0 ;; esac

    # Schedule decision: in off-peak → RUN NOW; in peak AND est is large → DEFER; else RUN NOW.
    decision="RUN NOW"; in_offpeak="n/a"; ow_out="null"
    if [ -n "$p_now" ]; then
      ow_args=(--now "$p_now" --start "$p_start" --end "$p_end"); [ -n "$p_tz" ] && ow_args+=(--tz-offset-min "$p_tz")
      ow_out="$("$OW" "${ow_args[@]}" 2>/dev/null)"
      in_offpeak="$(printf '%s' "$ow_out" | jq -r '.in_offpeak // "false"' 2>/dev/null)"
      if [ "$in_offpeak" != "true" ] && [ "$est_total" -ge "$DEFER_THRESHOLD" ] 2>/dev/null; then decision="DEFER"; fi
    fi

    band_disp="$(awk -v b="$band" 'BEGIN{ printf "%.0f", b+0 }')"
    printf '💰 ~%s tokens · p95 ±%s%% · %s (%s samples)\n' "$(fmt_tok "$est_total")" "$band_disp" "$tier" "$samples"
    if [ "$decision" = "DEFER" ]; then
      printf '🕒 Schedule: DEFER → off-peak %s–%s (now is peak; est is large)\n' "$p_start" "$p_end"
    else
      printf '🕒 Schedule: RUN NOW%s\n' "$([ "$in_offpeak" = "true" ] && echo " (off-peak)" || true)"
    fi
    printf '{"name":"%s","est_total":%s,"p95_band_pct":%s,"tier":"%s","samples":%s,"decision":"%s","in_offpeak":"%s"}\n' \
      "$pname" "$est_total" "$band" "$tier" "$samples" "$decision" "$in_offpeak"
    [ "$decision" = "DEFER" ] && exit 4 || exit 0
    ;;

  plan-open)
    # Bracket a plan: snapshot the cumulative session token count now as the baseline. Pairs with
    # plan-close, which reads it again — the delta is the plan's ACTUAL cost (session.json = the
    # right signal for after-the-fact actuals; NOT the live ceiling, which is the rate_limits signal).
    pclass="${1:-}"; pest="${2:-}"
    case "$pclass" in (''|*) : ;; esac
    [ -n "$pclass" ] || { echo "scheduler: plan-open <class> <est>" >&2; exit 2; }
    case "$pest" in (''|*[!0-9]*) pest=0 ;; esac
    SESSION="${I2P_SESSION_FILE:-${HOME}/.claude/state/i2p-cost/session.json}"
    POPEN="${I2P_PLANOPEN_FILE:-${HOME}/.claude/state/i2p-cost/plan-open.json}"
    base="$( [ -r "$SESSION" ] && jq -r '(.tokens // 0)' "$SESSION" 2>/dev/null || echo 0 )"
    case "$base" in (''|*[!0-9]*) base=0 ;; esac
    mkdir -p "$(dirname "$POPEN")" 2>/dev/null
    jq -n --arg c "$pclass" --argjson e "$pest" --argjson b "$base" \
      '{class:$c, est:$e, baseline_tokens:$b}' > "${POPEN}.tmp.$$" && mv -f "${POPEN}.tmp.$$" "$POPEN"
    printf '{"opened":"plan:%s","est":%s,"baseline_tokens":%s}\n' "$pclass" "$pest" "$base"
    ;;

  plan-close)
    SESSION="${I2P_SESSION_FILE:-${HOME}/.claude/state/i2p-cost/session.json}"
    POPEN="${I2P_PLANOPEN_FILE:-${HOME}/.claude/state/i2p-cost/plan-open.json}"
    [ -r "$POPEN" ] || { echo '{"error":"no-open-plan"}'; exit 2; }
    pclass="$(jq -r '.class // empty' "$POPEN" 2>/dev/null)"
    pest="$(jq -r '.est // 0' "$POPEN" 2>/dev/null)"
    base="$(jq -r '.baseline_tokens // 0' "$POPEN" 2>/dev/null)"
    cur="$( [ -r "$SESSION" ] && jq -r '(.tokens // 0)' "$SESSION" 2>/dev/null || echo 0 )"
    case "$cur" in (''|*[!0-9]*) cur=0 ;; esac
    case "$base" in (''|*[!0-9]*) base=0 ;; esac
    case "$pest" in (''|*[!0-9]*) pest=0 ;; esac
    actual=$(( cur - base )); [ "$actual" -lt 0 ] && actual=0
    # Feed convergence only when both est and actual are usable.
    conv="null"
    if [ "$pest" -gt 0 ] && [ "$actual" -gt 0 ]; then
      "$CALIBRATE" close "plan:${pclass}" "$pest" "$actual" >/dev/null 2>&1
      conv="$("$CALIBRATE" confidence "plan:${pclass}" 2>/dev/null)"
    fi
    rm -f "$POPEN" 2>/dev/null
    printf '{"class":"plan:%s","est":%s,"actual":%s,"convergence":%s}\n' \
      "$pclass" "$pest" "$actual" "${conv:-null}"
    ;;

  gate)
    headroom=15 window="both" require_offpeak=0
    now="" start="22:00" end="08:00" tzoff="" snap_max_age=900 clock=""
    while [ $# -gt 0 ]; do
      case "$1" in
        --headroom) headroom="${2:-}"; shift 2 ;;
        --window) window="${2:-}"; shift 2 ;;
        --require-offpeak) require_offpeak=1; shift ;;
        --now) now="${2:-}"; shift 2 ;;
        --start) start="${2:-}"; shift 2 ;;
        --end) end="${2:-}"; shift 2 ;;
        --tz-offset-min) tzoff="${2:-}"; shift 2 ;;
        --snapshot-max-age) snap_max_age="${2:-}"; shift 2 ;;
        --clock) clock="${2:-}"; shift 2 ;;   # epoch override for snapshot-freshness (testing)
        *) shift ;;
      esac
    done

    payload=""; [ -t 0 ] || payload="$(cat 2>/dev/null || true)"

    # Where's the live signal? Prefer the payload piped in (a hook handed it to us). If that has no
    # rate_limits (e.g. an ad-hoc orchestrator Bash call, which the harness does NOT feed the signal),
    # fall back to the freshness-stamped snapshot — but only if it's fresh, else fail closed.
    have_signal="$(printf '%s' "$payload" | jq -r \
      '(.rate_limits.five_hour.used_percentage // .rate_limits.seven_day.used_percentage // empty)' 2>/dev/null)"
    if [ -z "$have_signal" ]; then
      snap="${I2P_RATELIMIT_SNAPSHOT:-${I2P_COST_STATE_DIR:-${HOME}/.claude/state/i2p-cost}/ratelimit-snapshot.json}"
      if [ -r "$snap" ]; then
        cap="$(jq -r '.captured_at // 0' "$snap" 2>/dev/null)"; case "$cap" in (''|*[!0-9]*) cap=0 ;; esac
        nowclk="${clock:-$(date +%s 2>/dev/null || echo 0)}"; case "$nowclk" in (''|*[!0-9]*) nowclk=0 ;; esac
        case "$snap_max_age" in (''|*[!0-9]*) snap_max_age=900 ;; esac
        age=$(( nowclk - cap ))
        if [ "$cap" -gt 0 ] && [ "$age" -ge 0 ] && [ "$age" -le "$snap_max_age" ]; then
          payload="$(cat "$snap" 2>/dev/null)"   # snapshot has top-level .rate_limits — same shape
        fi
        # stale/absent snapshot → leave payload as-is → ceiling-check returns NO_SIGNAL → ASK (fail closed)
      fi
    fi

    # L1 live ceiling — the load-bearing check.
    ceil_out="$(printf '%s' "$payload" | "$CEIL" --headroom "$headroom" --window "$window" 2>/dev/null)"; ceil_rc=$?
    case "$ceil_rc" in
      10) printf '{"verdict":"HALT","ceiling":%s}\n' "${ceil_out:-null}"; exit 10 ;;
      20) printf '{"verdict":"ASK","reason":"no-live-signal","ceiling":%s}\n' "${ceil_out:-null}"; exit 20 ;;
    esac

    # Optional off-peak gate — DEFER if the quiet hours haven't started.
    ow_out="null"
    if [ "$require_offpeak" = 1 ] && [ -n "$now" ]; then
      ow_args=(--now "$now" --start "$start" --end "$end")
      [ -n "$tzoff" ] && ow_args+=(--tz-offset-min "$tzoff")
      ow_out="$("$OW" "${ow_args[@]}" 2>/dev/null)"
      if [ "$(printf '%s' "$ow_out" | jq -r '.in_offpeak // "false"' 2>/dev/null)" != "true" ]; then
        printf '{"verdict":"DEFER","ceiling":%s,"offpeak":%s}\n' "${ceil_out:-null}" "${ow_out:-null}"
        exit 4   # exit 4 = DEFER
      fi
    fi

    printf '{"verdict":"CONTINUE","ceiling":%s,"offpeak":%s}\n' "${ceil_out:-null}" "${ow_out:-null}"
    exit 0
    ;;

  -h|--help|'') grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) echo "scheduler: unknown subcommand '$sub' (try: gate | preflight)" >&2; exit 2 ;;
esac
