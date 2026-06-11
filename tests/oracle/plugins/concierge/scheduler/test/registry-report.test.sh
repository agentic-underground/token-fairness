#!/usr/bin/env bash
# Tests for jobs-registry.sh (durable scheduled-job registry) and report.sh (the covenant report).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
. "${HERE}/lib/harness.sh"
REG="${HERE}/../jobs-registry.sh"
REPORT="${HERE}/../report.sh"
CAL_SH="${HERE}/../calibrate.sh"
jget() { printf '%s' "$1" | jq -r "$2"; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
export I2P_MACHINE_REGISTRY="${TMP}/machine.json"
export I2P_CALIBRATION_FILE="${TMP}/calibration.json"

# ---------- jobs-registry.sh ----------
test_case "register persists to project + machine index"
bash "$REG" register "$TMP" job1 "17 22-23 * * *" 400000 ".i2p/jobs/job1.json" ".i2p/x.prompt" "a note" >/dev/null
assert_eq "$(jget "$(bash "$REG" list "$TMP")" 'length')" "1"
assert_eq "$(jget "$(bash "$REG" get "$TMP" job1)" '.budget_total')" "400000"
assert_eq "$(jq '.jobs|length' "$I2P_MACHINE_REGISTRY")" "1"
assert_eq "$(jq -r '.jobs[0].repo' "$I2P_MACHINE_REGISTRY")" "$(cd "$TMP" && pwd)"

test_case "register is an upsert (same id replaces, no dup)"
bash "$REG" register "$TMP" job1 "30 2 * * *" 500000 ".i2p/jobs/job1.json" ".i2p/x.prompt" "updated" >/dev/null
assert_eq "$(jget "$(bash "$REG" list "$TMP")" 'length')" "1"
assert_eq "$(jget "$(bash "$REG" get "$TMP" job1)" '.budget_total')" "500000"

test_case "new job starts unarmed; arm sets true; reset-armed clears all"
assert_eq "$(jget "$(bash "$REG" get "$TMP" job1)" '.armed')" "false"
bash "$REG" arm "$TMP" job1 >/dev/null
assert_eq "$(jget "$(bash "$REG" get "$TMP" job1)" '.armed')" "true"
bash "$REG" reset-armed "$TMP" >/dev/null
assert_eq "$(jget "$(bash "$REG" get "$TMP" job1)" '.armed')" "false"

test_case "remove drops from project + machine index"
bash "$REG" register "$TMP" job2 "0 3 * * *" 100000 "" "" "second" >/dev/null
bash "$REG" remove "$TMP" job1 >/dev/null
assert_eq "$(jget "$(bash "$REG" list "$TMP")" 'length')" "1"
assert_eq "$(jget "$(bash "$REG" get "$TMP" job1)" '.id // "gone"')" "gone"
assert_eq "$(jq '[.jobs[] | select(.id=="job1")] | length' "$I2P_MACHINE_REGISTRY")" "0"

# ---------- report.sh ----------
test_case "scheduled report lists the job and flags unarmed"
out="$(bash "$REPORT" "$TMP" --scheduled)"
assert_contains "$out" "job2"
assert_contains "$out" "NOT armed"

test_case "estimator report shows calibration keys + tiers"
bash "$CAL_SH" close plan:medium 1000 1100 >/dev/null
bash "$CAL_SH" close uplift-fanout 1000 1000 >/dev/null
out="$(bash "$REPORT" "$TMP" --estimator)"
assert_contains "$out" "plan:medium"
assert_contains "$out" "uplift-fanout"
assert_contains "$out" "samples"

test_case "brief is non-empty when a job exists (key-indicator dashboard)"
out="$(bash "$REPORT" "$TMP" --brief)"
assert_contains "$out" "Scheduler ·"
assert_contains "$out" "job2"

test_case "brief is SILENT for an empty repo with no calibration"
EMPTY="$(mktemp -d)"; export I2P_CALIBRATION_FILE="${EMPTY}/none.json"
out="$(bash "$REPORT" "$EMPTY" --brief)"
assert_eq "$out" ""
export I2P_CALIBRATION_FILE="${TMP}/calibration.json"
rm -rf "$EMPTY"

finish
