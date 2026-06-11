#!/usr/bin/env bash
# report.sh — "what's scheduled, and how's the estimator doing?" The covenant made visible.
#
# Two sections, deterministic, read-only:
#   📋 SCHEDULED JOBS — from the durable registry + each job's ledger: state, done/remaining, budget,
#      last checkpoint, and whether the cron is armed THIS session (re-arm prompt if not).
#   📈 ESTIMATOR — from the calibration ledger: per profile/class, sample count, learned mean ratio,
#      p95 band, and convergence tier. This is the continuous-improvement signal — estimates getting
#      measurably better over time, the SOLID self-improvement covenant in numbers.
#
#   report.sh [dir] [--scheduled | --estimator | --brief]
#     --brief : one compact block for the SessionStart hook; prints NOTHING when there is nothing
#               pending and no calibration yet (so it never nags an empty repo).
set -uo pipefail
command -v jq >/dev/null 2>&1 || { echo "report: jq required" >&2; exit 0; }

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REG="${HERE}/jobs-registry.sh"
CAL_SH="${HERE}/calibrate.sh"
CAL="${I2P_CALIBRATION_FILE:-${HOME}/.claude/state/i2p-cost/calibration.json}"
SIG="${I2P_SIGNAL_FINDINGS:-${HOME}/.claude/state/i2p-cost/signal-findings.json}"

dir="."; mode="full"
for a in "$@"; do case "$a" in
  --scheduled) mode="scheduled" ;; --estimator) mode="estimator" ;; --brief) mode="brief" ;;
  -*) : ;; *) dir="$a" ;;
esac; done

fmt_tok() { awk -v t="$1" 'BEGIN{ t=t+0; if(t>=1000000) printf "%.1fM", t/1000000; else if(t>=1000) printf "%dk", int(t/1000+0.5); else printf "%d", t }'; }

jobs_json="$(bash "$REG" list "$dir" 2>/dev/null || echo '[]')"
n_jobs="$(printf '%s' "$jobs_json" | jq 'length' 2>/dev/null || echo 0)"
n_cal="$( [ -r "$CAL" ] && jq 'keys | length' "$CAL" 2>/dev/null || echo 0 )"

print_scheduled() {
  printf '📋 Scheduled jobs (%s)\n' "$n_jobs"
  [ "$n_jobs" -eq 0 ] && { printf '   none registered in this repo.\n'; return; }
  printf '%s' "$jobs_json" | jq -c '.[]' | while read -r j; do
    id="$(printf '%s' "$j" | jq -r '.id')"
    cron="$(printf '%s' "$j" | jq -r '.cron // "—"')"
    budget="$(printf '%s' "$j" | jq -r '.budget_total // 0')"
    armed="$(printf '%s' "$j" | jq -r '.armed // false')"
    armed_via="$(printf '%s' "$j" | jq -r '.armed_via // "session"')"
    ledger_rel="$(printf '%s' "$j" | jq -r '.ledger // empty')"
    note="$(printf '%s' "$j" | jq -r '.note // empty')"
    lf="${dir%/}/${ledger_rel#./}"
    if [ -n "$ledger_rel" ] && [ -r "$lf" ]; then
      state="$(jq -r '.state // "?"' "$lf" 2>/dev/null)"
      done_n="$(jq -r '(.units.done|length) // 0' "$lf" 2>/dev/null)"
      rem_n="$(jq -r '(.units.remaining|length) // 0' "$lf" 2>/dev/null)"
      tot_n="$(jq -r '.units.total // 0' "$lf" 2>/dev/null)"
      ck="$(jq -r '(.checkpoints | last) | if . then "\(.reason)@\(.five_hour_pct)%" else "—" end' "$lf" 2>/dev/null)"
      progress="${done_n}/${tot_n} done · ${rem_n} left · ${state} · last checkpoint ${ck}"
    else
      progress="(ledger not created yet — first off-peak fire will init it)"
    fi
    printf '  • %s — cron "%s" · budget %s\n' "$id" "$cron" "$(fmt_tok "$budget")"
    printf '      %s\n' "$progress"
    [ -n "$note" ] && printf '      ↳ %s\n' "$note"
    if [ "$armed" = "true" ] && [ "$armed_via" = "oscron" ]; then
      printf '      ✓ OS-cron armed — fires even with Claude closed (machine awake); survives restarts.\n'
    elif [ "$armed" = "true" ]; then
      printf '      ✓ armed this session (in-session cron; re-arm needed after a restart).\n'
    else
      printf '      ⚠ NOT armed — install OS-cron (install-oscron.sh) or re-arm the in-session cron.\n'
    fi
  done
}

print_estimator() {
  printf '📈 Estimator convergence (%s tracked)\n' "$n_cal"
  [ "$n_cal" -eq 0 ] && { printf '   no samples yet — estimates start at SEEDING and sharpen as jobs complete.\n'; return; }
  jq -r 'keys[]' "$CAL" 2>/dev/null | sort | while read -r k; do
    c="$(bash "$CAL_SH" confidence "$k" 2>/dev/null)"
    s="$(printf '%s' "$c" | jq -r '.samples')"; mr="$(printf '%s' "$c" | jq -r '.mean_ratio')"
    band="$(printf '%s' "$c" | jq -r '.p95_band_pct')"; tier="$(printf '%s' "$c" | jq -r '.tier')"
    case "$(printf '%s' "$c" | jq -r '.trend')" in improving) ar="↑";; worsening) ar="↓";; *) ar="→";; esac
    printf '  %-22s %3s samples · mean×%s · p95 ±%s%% · %s %s\n' "$k" "$s" "$mr" "$band" "$tier" "$ar"
  done
}

case "$mode" in
  scheduled) print_scheduled ;;
  estimator) print_estimator ;;
  brief)
    # Two tight key-indicator lines — a dashboard, not prose. Silent when there's nothing to say.
    [ "$n_jobs" -eq 0 ] && [ "$n_cal" -eq 0 ] && exit 0

    # Line 1 — scheduler: first job's progress + how it's armed + the live-signal mode.
    if [ "$n_jobs" -gt 0 ]; then
      j="$(printf '%s' "$jobs_json" | jq -c '.[0]')"
      id="$(printf '%s' "$j" | jq -r '.id')"
      armed="$(printf '%s' "$j" | jq -r '.armed // false')"; via="$(printf '%s' "$j" | jq -r '.armed_via // "session"')"
      ledger_rel="$(printf '%s' "$j" | jq -r '.ledger // empty')"; lf="${dir%/}/${ledger_rel#./}"
      prog="pending"
      [ -n "$ledger_rel" ] && [ -r "$lf" ] && prog="$(jq -r '"\((.units.done|length))/\(.units.total) done"' "$lf" 2>/dev/null)"
      if   [ "$armed" = "true" ] && [ "$via" = "oscron" ]; then armstr="OS-cron armed"
      elif [ "$armed" = "true" ]; then armstr="armed (session)"
      else armstr="⚠ NOT armed"; fi
      sig="$( [ -r "$SIG" ] && jq -r '.guard_mode // "unknown"' "$SIG" 2>/dev/null || echo "unknown" )"
      extra=""; [ "$n_jobs" -gt 1 ] && extra=" (+$((n_jobs-1)) more)"
      printf '🛡️ Scheduler · %s %s · %s · signal: %s%s\n' "$id" "$prog" "$armstr" "$sig" "$extra"
    fi

    # Line 2 — estimator: up to 2 representative profiles with band · tier · trend.
    if [ "$n_cal" -gt 0 ]; then
      line="$(jq -r 'keys[]' "$CAL" 2>/dev/null | sort | head -2 | while read -r k; do
        c="$(bash "$CAL_SH" confidence "$k" 2>/dev/null)"
        b="$(printf '%s' "$c" | jq -r '.p95_band_pct')"; t="$(printf '%s' "$c" | jq -r '.tier')"
        case "$(printf '%s' "$c" | jq -r '.trend')" in improving) a="↑";; worsening) a="↓";; *) a="→";; esac
        printf '%s ±%s%% %s %s · ' "$k" "$b" "$t" "$a"
      done)"
      printf '📈 Estimator · %s profiles · %s\n' "$n_cal" "${line% · }"
    fi
    ;;
  *)
    print_scheduled; echo; print_estimator ;;
esac
