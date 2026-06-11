#!/usr/bin/env bash
# scheduler-estimate.sh — L0 PRE-FLIGHT ESTIMATOR. The pane of glass before any fan-out.
#
# Answers, deterministically: "what will this job cost, and do we trust the number?"
#   est_total = fanout_width × per_unit_tokens × calibration_ratio
#
# per_unit_tokens is chosen by EVIDENCE, best-first — the user's "run one of n, measure, then
# decide" made mechanical:
#   1. --measured-unit-tokens M   a real probe of ONE unit just ran           → basis=measured, HIGH
#   2. --history-tokens M         a historical median for like jobs           → basis=history,  HIGH
#   3. profile.estimated_unit_tokens   the profile's declared guess           → basis=declared, LOW
#   4. seed default (20000)       nothing better exists yet                    → basis=seed,     LOW
#
# calibration_ratio is the learned actual/estimate EWMA for this profile (calibrate.sh ratio),
# so every job that passes through sharpens the next estimate (estimate→actual convergence).
#
# A LOW confidence verdict is the signal to PROBE: run one unit for real, feed its tokens back in
# via --measured-unit-tokens, and the estimate becomes HIGH. The caller (skill/model) decides.
#
#   scheduler-estimate.sh --profile P [--width N] [--name NAME] \
#       [--measured-unit-tokens M] [--history-tokens M]
#
# Output: one-line JSON {name,per_unit,basis,confidence,fanout,ratio,est_total}. Deterministic; needs jq+awk.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CALIBRATE="${HERE}/calibrate.sh"
SEED_UNIT=20000   # last-resort per-unit seed (tokens); corrected by calibration over time

profile="" width="" name="" measured="" history="" class=""
while [ $# -gt 0 ]; do
  case "$1" in
    --profile) profile="${2:-}"; shift 2 ;;
    --width)   width="${2:-}";   shift 2 ;;
    --name)    name="${2:-}";    shift 2 ;;
    --class)   class="${2:-}";   shift 2 ;;
    --measured-unit-tokens) measured="${2:-}"; shift 2 ;;
    --history-tokens)       history="${2:-}";  shift 2 ;;
    --profile=*) profile="${1#*=}"; shift ;;
    --width=*)   width="${1#*=}";   shift ;;
    --name=*)    name="${1#*=}";    shift ;;
    --class=*)   class="${1#*=}";   shift ;;
    --measured-unit-tokens=*) measured="${1#*=}"; shift ;;
    --history-tokens=*)       history="${1#*=}";  shift ;;
    -h|--help) grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) shift ;;
  esac
done

command -v jq  >/dev/null 2>&1 || { echo '{"error":"jq-required"}'; exit 2; }
command -v awk >/dev/null 2>&1 || { echo '{"error":"awk-required"}'; exit 2; }

# Profile-less CLASS path: price ANY plan (not just fan-outs). The model classifies the plan's size
# (judgement); the seed table here is corrected by the per-class calibration over time (determinism),
# so the seed barely matters once samples accrue. width=1; the "unit" IS the whole plan.
class_seed() { case "$1" in
  small) echo 25000 ;; medium) echo 80000 ;; large) echo 250000 ;; epic) echo 700000 ;; *) echo "" ;;
esac; }

pj() { [ -n "$profile" ] && [ -r "$profile" ] && jq -r "$1 // empty" "$profile" 2>/dev/null || true; }

is_pos_int() { case "$1" in (''|*[!0-9]*) return 1 ;; (*) [ "$1" -gt 0 ] ;; esac; }

# CLASS path short-circuits the profile/evidence logic.
class_seed_val=""
if [ -n "$class" ]; then class_seed_val="$(class_seed "$class")"; fi
if [ -n "$class_seed_val" ]; then
  name="plan:${class}"; width=1; per_unit="$class_seed_val"; basis="class"; confidence="low"
else

[ -n "$name" ]  || name="$(pj '.name')"
[ -n "$name" ]  || name="unnamed"
[ -n "$width" ] || width="$(pj '.fanout.width_default')"
case "$width" in (''|*[!0-9]*) width=1 ;; esac

declared="$(pj '.estimated_unit_tokens')"
case "$declared" in (''|*[!0-9]*) declared="" ;; esac

# Choose per_unit by evidence, best-first.
if   is_pos_int "$measured"; then per_unit="$measured"; basis="measured"; confidence="high"
elif is_pos_int "$history";  then per_unit="$history";  basis="history";  confidence="high"
elif is_pos_int "$declared"; then per_unit="$declared"; basis="declared"; confidence="low"
else per_unit="$SEED_UNIT"; basis="seed"; confidence="low"; fi

fi   # end CLASS-vs-profile branch

ratio="$( "$CALIBRATE" ratio "$name" 2>/dev/null || echo "1.0" )"
case "$ratio" in (''|*[!0-9.]*) ratio="1.0" ;; esac

est_total="$(awk -v w="$width" -v u="$per_unit" -v r="$ratio" 'BEGIN{ printf "%d", (w*u*r)+0.5 }')"

# Convergence picture: the learned p95 band for this name, and the est bracketed by it. `confidence`
# (high/low, basis-derived) still drives the PROBE decision; `convergence`+`interval` show the user
# how trustworthy — and how much MORE trustworthy over time — the number is.
conv="$( "$CALIBRATE" confidence "$name" 2>/dev/null || echo '{"samples":0,"mean_ratio":1.0,"sd":0,"p95_band_pct":60,"tier":"SEEDING"}' )"
band="$(printf '%s' "$conv" | jq -r '.p95_band_pct // 60' 2>/dev/null)"; case "$band" in (''|*[!0-9.]*) band=60 ;; esac
lo="$(awk -v e="$est_total" -v b="$band" 'BEGIN{ v=e*(1-b/100); if(v<0)v=0; printf "%d", v+0.5 }')"
hi="$(awk -v e="$est_total" -v b="$band" 'BEGIN{ printf "%d", e*(1+b/100)+0.5 }')"

printf '{"name":"%s","per_unit":%s,"basis":"%s","confidence":"%s","fanout":%s,"ratio":%s,"est_total":%s,"convergence":%s,"interval":[%s,%s]}\n' \
  "$name" "$per_unit" "$basis" "$confidence" "$width" "$ratio" "$est_total" "$conv" "$lo" "$hi"
