#!/usr/bin/env bash
# calibrate.sh — the estimate↔actual CONVERGENCE engine for job profiles.
#
# Reads/writes the SHARED home-state ledger ~/.claude/state/i2p-cost/calibration.json — the same
# file i2p's lifecycle cost.sh learns per-PHASE ratios in. Profiles get their own top-level keys
# (e.g. "reviewer-fanout") ALONGSIDE the phase keys (DISCOVER…); the schema is additive, no
# migration. The EWMA formula here is the CANONICAL α=0.4 mirror of cost.sh's `close` (so a job's
# profile estimate self-corrects exactly as a lifecycle phase estimate does). It lives here — not
# via a cross-plugin call to cost.sh — because CONCIERGE is self-contained and must not resolve a
# sibling plugin's path (CLAUDE.md). The shared FILE is the contract; the formula is kept identical.
#
#   calibrate.sh ratio <name>                 # echo learned ratio_ewma for <name> (default 1.0)
#   calibrate.sh close <name> <estimate> <actual>   # fold actual/estimate into the EWMA; echo new ratio
#
# Pure arithmetic; deterministic; needs jq + awk. Never blocks (advisory exit 0 paths for bad input).
set -uo pipefail

EWMA_ALPHA=0.4
CAL="${I2P_CALIBRATION_FILE:-${HOME}/.claude/state/i2p-cost/calibration.json}"   # override for tests

command -v jq >/dev/null 2>&1 || { echo "1.0"; exit 0; }

cmd="${1:-ratio}"; name="${2:-}"

[ -n "$name" ] || { echo "usage: calibrate.sh {ratio <name>|close <name> <estimate> <actual>}" >&2; exit 2; }

case "$cmd" in
  ratio)
    [ -r "$CAL" ] || { echo "1.0"; exit 0; }
    jq -r --arg p "$name" '(.[$p].ratio_ewma // 1.0)' "$CAL" 2>/dev/null || echo "1.0"
    ;;

  close)
    est="${3:-}"; act="${4:-}"
    case "$est" in (''|*[!0-9]*) echo "calibrate: estimate must be a positive integer" >&2; exit 0 ;; esac
    case "$act" in (''|*[!0-9]*) echo "calibrate: actual must be a positive integer"   >&2; exit 0 ;; esac
    [ "$est" -gt 0 ] || { echo "calibrate: estimate must be > 0" >&2; exit 0; }
    ratio="$(awk -v a="$act" -v e="$est" 'BEGIN{ printf "%.4f", a/e }')"
    mkdir -p "$(dirname "$CAL")" 2>/dev/null
    [ -r "$CAL" ] || printf '{}\n' > "$CAL"
    # Capture the band BEFORE this sample so the report can show a trend (tightening = improving).
    oldn="$(jq -r --arg p "$name" '(.[$p].w_n // 0)' "$CAL" 2>/dev/null)"; case "$oldn" in (''|*[!0-9]*) oldn=0 ;; esac
    oldmean="$(jq -r --arg p "$name" '(.[$p].w_mean // 1)' "$CAL" 2>/dev/null)"
    oldm2="$(jq -r --arg p "$name" '(.[$p].w_m2 // 0)' "$CAL" 2>/dev/null)"
    old_band="$(awk -v n="$oldn" -v mean="$oldmean" -v m2="$oldm2" 'BEGIN{
      if(n+0<=0){print 60.0; exit} if(n>=2){var=m2/(n-1); if(var<0)var=0; sd=sqrt(var); b=(mean+0!=0?1.645*sd/mean*100:50)} else b=50;
      if(n<5 && b<40)b=40; printf "%.1f", b }')"
    # EWMA ratio (legacy) PLUS Welford online mean/variance of the ratio (n, mean, m2) — additive.
    # Welford: n+=1; d=r-mean; mean+=d/n; m2+=d*(r-mean). Variance = m2/(n-1). This is what lets the
    # p95 confidence band TIGHTEN as samples accumulate (the visible convergence the user asked for).
    jq --arg p "$name" --argjson r "$ratio" --argjson alpha "$EWMA_ALPHA" '
      .[$p] = ((.[$p] // {samples:0, ratio_ewma:1.0, w_n:0, w_mean:0, w_m2:0})
        | .ratio_ewma = (if (.samples // 0) == 0 then $r else ($alpha*$r + (1-$alpha)*(.ratio_ewma // 1.0)) end)
        | .samples = ((.samples // 0) + 1)
        | .last_ratio = $r
        | .w_n = ((.w_n // 0) + 1)
        | (.w_mean // 0) as $oldmean
        | ($r - $oldmean) as $delta
        | .w_mean = ($oldmean + $delta / .w_n)
        | .w_m2 = ((.w_m2 // 0) + $delta * ($r - .w_mean))
        | .prev_band = $ob)' \
      --argjson ob "$old_band" \
      "$CAL" > "${CAL}.tmp.$$" && mv -f "${CAL}.tmp.$$" "$CAL"
    jq -r --arg p "$name" '.[$p].ratio_ewma' "$CAL" 2>/dev/null || echo "1.0"
    ;;

  confidence)
    # Emit the convergence picture for <name>: how many samples, the learned mean ratio, and the
    # p95 RELATIVE band (1.645·sd/mean) that an estimate should be bracketed by — plus a tier the
    # user can read at a glance. Fewer samples ⇒ honestly wider band. Pure arithmetic in awk.
    n="$( [ -r "$CAL" ] && jq -r --arg p "$name" '(.[$p].w_n // 0)' "$CAL" 2>/dev/null || echo 0 )"
    mean="$( [ -r "$CAL" ] && jq -r --arg p "$name" '(.[$p].w_mean // 1)' "$CAL" 2>/dev/null || echo 1 )"
    m2="$( [ -r "$CAL" ] && jq -r --arg p "$name" '(.[$p].w_m2 // 0)' "$CAL" 2>/dev/null || echo 0 )"
    prev="$( [ -r "$CAL" ] && jq -r --arg p "$name" '(.[$p].prev_band // -1)' "$CAL" 2>/dev/null || echo -1 )"
    case "$n" in (''|*[!0-9]*) n=0 ;; esac
    case "$prev" in (''|*[!0-9.-]*) prev=-1 ;; esac
    awk -v n="$n" -v mean="$mean" -v m2="$m2" -v prev="$prev" 'BEGIN{
      if (n+0 <= 0) { printf "{\"samples\":0,\"mean_ratio\":1.0000,\"sd\":0.0000,\"p95_band_pct\":60.0,\"tier\":\"SEEDING\",\"prev_band\":%.1f,\"trend\":\"flat\"}\n", prev; exit }
      if (n >= 2) { var = m2/(n-1); if (var < 0) var = 0; sd = sqrt(var); band = (mean+0!=0 ? 1.645*sd/mean*100 : 50) }
      else { sd = 0; band = 50 }
      if (n < 5) { tier = "CALIBRATING"; if (band < 40) band = 40 }
      else if (n >= 10 && band <= 15) tier = "CONVERGED";
      else tier = "CONVERGING";
      trend = "flat";
      if (prev >= 0) { if (band < prev - 0.05) trend = "improving"; else if (band > prev + 0.05) trend = "worsening" }
      printf "{\"samples\":%d,\"mean_ratio\":%.4f,\"sd\":%.4f,\"p95_band_pct\":%.1f,\"tier\":\"%s\",\"prev_band\":%.1f,\"trend\":\"%s\"}\n", n, mean, sd, band, tier, prev, trend;
    }'
    ;;

  *) echo "usage: calibrate.sh {ratio <name>|close <name> <estimate> <actual>|confidence <name>}" >&2; exit 2 ;;
esac
