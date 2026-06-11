#!/usr/bin/env bash
# Tests for calibrate.sh (EWMA convergence) and scheduler-estimate.sh (evidence-first estimate).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
. "${HERE}/lib/harness.sh"
EST="${HERE}/../scheduler-estimate.sh"
CAL="${HERE}/../calibrate.sh"
PROFILE="${HERE}/../profiles/reviewer-fanout.json"

# Isolated calibration file per run — never touch the user's real ledger.
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
export I2P_CALIBRATION_FILE="${TMP}/calibration.json"

jget() { printf '%s' "$1" | jq -r "$2"; }

# ---------- calibrate.sh ----------
test_case "ratio defaults to 1.0 when unseen"
assert_eq "$(bash "$CAL" ratio neverseen)" "1.0"

test_case "first close sets ratio to the raw actual/estimate"
out="$(bash "$CAL" close jobA 1000 2000)"   # ratio 2.0 on first sample
assert_eq "$out" "2.0000"

test_case "second close blends via EWMA α=0.4: 0.4*1.0 + 0.6*2.0 = 1.6"
# (jq prints the first literal sample with full precision but a COMPUTED blend as 1.6 — canonical cost.sh behavior)
out="$(bash "$CAL" close jobA 1000 1000)"   # raw ratio 1.0; prev 2.0
assert_eq "$out" "1.6"

test_case "ratio read returns the learned value"
assert_eq "$(bash "$CAL" ratio jobA)" "1.6"

test_case "distinct profiles keep independent ratios"
bash "$CAL" close jobB 1000 500 >/dev/null   # ratio 0.5 (first sample → literal preserved)
assert_eq "$(bash "$CAL" ratio jobB)" "0.5000"
assert_eq "$(bash "$CAL" ratio jobA)" "1.6"

test_case "close with zero/negative estimate is refused (no crash, no write)"
bash "$CAL" close jobC 0 100 >/dev/null 2>&1
assert_eq "$(bash "$CAL" ratio jobC)" "1.0"

# ---------- scheduler-estimate.sh ----------
# Fresh calibration so ratio=1.0 for clean arithmetic.
export I2P_CALIBRATION_FILE="${TMP}/cal2.json"

test_case "declared basis: profile width 26 × declared 18000 × ratio 1 = 468000, LOW confidence"
out="$(bash "$EST" --profile "$PROFILE")"
assert_eq "$(jget "$out" .est_total)" "468000"
assert_eq "$(jget "$out" .basis)" "declared"
assert_eq "$(jget "$out" .confidence)" "low"
assert_eq "$(jget "$out" .fanout)" "26"

test_case "measured basis overrides declared and is HIGH confidence"
out="$(bash "$EST" --profile "$PROFILE" --measured-unit-tokens 20000 --width 10)"
assert_eq "$(jget "$out" .est_total)" "200000"
assert_eq "$(jget "$out" .basis)" "measured"
assert_eq "$(jget "$out" .confidence)" "high"

test_case "history basis used when no measurement (HIGH)"
out="$(bash "$EST" --profile "$PROFILE" --history-tokens 15000 --width 4)"
assert_eq "$(jget "$out" .est_total)" "60000"
assert_eq "$(jget "$out" .basis)" "history"

test_case "seed basis (no profile, no evidence) → 20000/unit, LOW"
out="$(bash "$EST" --name adhoc --width 3)"
assert_eq "$(jget "$out" .est_total)" "60000"
assert_eq "$(jget "$out" .basis)" "seed"
assert_eq "$(jget "$out" .confidence)" "low"

test_case "learned ratio scales the estimate"
export I2P_CALIBRATION_FILE="${TMP}/cal3.json"
bash "$CAL" close reviewer-fanout 1000 1500 >/dev/null   # ratio 1.5
out="$(bash "$EST" --profile "$PROFILE" --measured-unit-tokens 10000 --width 2)"
# 2 × 10000 × 1.5 = 30000
assert_eq "$(jget "$out" .est_total)" "30000"
assert_eq "$(jget "$out" .ratio)" "1.5000"

test_case "width override beats profile default"
export I2P_CALIBRATION_FILE="${TMP}/cal4.json"
out="$(bash "$EST" --profile "$PROFILE" --width 1 --measured-unit-tokens 12345)"
assert_eq "$(jget "$out" .est_total)" "12345"

# ---------- 2.A convergence: Welford stats + tier + tightening band ----------
export I2P_CALIBRATION_FILE="${TMP}/conv.json"

test_case "unseen name → SEEDING, wide default band"
out="$(bash "$CAL" confidence fresh)"
assert_eq "$(jget "$out" .tier)" "SEEDING"
assert_eq "$(jget "$out" .samples)" "0"
assert_eq "$(jget "$out" .p95_band_pct)" "60.0"

test_case "Welford mean over 3 equal ratios (all 1.5) = 1.5, sd 0"
for i in 1 2 3; do bash "$CAL" close conv 1000 1500 >/dev/null; done   # ratio 1.5 each
out="$(bash "$CAL" confidence conv)"
assert_eq "$(jget "$out" .mean_ratio)" "1.5000"
assert_eq "$(jget "$out" .sd)" "0.0000"
assert_eq "$(jget "$out" .tier)" "CALIBRATING"   # n=3 (<5)

test_case "Welford sample variance over [1.0, 2.0] matches hand calc (sd=0.7071)"
export I2P_CALIBRATION_FILE="${TMP}/conv2.json"
bash "$CAL" close c2 1000 1000 >/dev/null   # ratio 1.0
bash "$CAL" close c2 1000 2000 >/dev/null   # ratio 2.0
out="$(bash "$CAL" confidence c2)"
assert_eq "$(jget "$out" .mean_ratio)" "1.5000"
assert_eq "$(jget "$out" .sd)" "0.7071"

test_case "tier climbs to CONVERGED with ≥10 tight samples and band ≤15%"
export I2P_CALIBRATION_FILE="${TMP}/conv3.json"
for i in $(seq 1 12); do bash "$CAL" close c3 1000 1000 >/dev/null; done   # identical → sd 0
out="$(bash "$CAL" confidence c3)"
assert_eq "$(jget "$out" .tier)" "CONVERGED"
assert_eq "$(jget "$out" .p95_band_pct)" "0.0"

test_case "band STRICTLY shrinks as identical samples accumulate (convergence is real)"
export I2P_CALIBRATION_FILE="${TMP}/conv4.json"
# two spread samples then many tight ones — band after 5 must be < band after 5+spread baseline
bash "$CAL" close c4 1000 1000 >/dev/null
bash "$CAL" close c4 1000 2000 >/dev/null
b2=$(jget "$(bash "$CAL" confidence c4)" .p95_band_pct)
for i in $(seq 1 8); do bash "$CAL" close c4 1000 1500 >/dev/null; done   # pull toward mean, shrink sd
b10=$(jget "$(bash "$CAL" confidence c4)" .p95_band_pct)
awk -v a="$b2" -v b="$b10" 'BEGIN{ exit !(b < a) }' && _pass "band $b10 < $b2" || _fail "band did not shrink ($b10 vs $b2)"

test_case "estimate output carries convergence object + interval bracketing est_total"
export I2P_CALIBRATION_FILE="${TMP}/conv5.json"
out="$(bash "$EST" --name c5 --width 1 --measured-unit-tokens 100000)"
et=$(jget "$out" .est_total); lo=$(jget "$out" '.interval[0]'); hi=$(jget "$out" '.interval[1]')
assert_eq "$(jget "$out" .convergence.tier)" "SEEDING"
awk -v l="$lo" -v e="$et" -v h="$hi" 'BEGIN{ exit !(l<=e && e<=h && l<h) }' && _pass "interval [$lo,$hi] brackets $et" || _fail "interval bad"

finish
