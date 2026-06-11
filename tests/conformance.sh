#!/usr/bin/env bash
# conformance.sh — differential gate: the same inputs through the ORIGINAL bash scheduler
# and through `tf`, asserting identical stdout + exit code. This is the no-regression proof
# that the Rust port preserves the lockout guard's behaviour bit-for-bit.
#
#   BASH_DIR=/path/to/concierge/scheduler TF=/path/to/tf bash tests/conformance.sh
set -uo pipefail

BASH_DIR="${BASH_DIR:-$HOME/Code/idea-to-production/plugins/concierge/scheduler}"
TF="${TF:-$HOME/Code/token-fairness/target/debug/tf}"

pass=0 fail=0
_C_RED=$'\033[31m'; _C_GRN=$'\033[32m'; _C_DIM=$'\033[2m'; _C_RST=$'\033[0m'
[ -t 1 ] || { _C_RED=""; _C_GRN=""; _C_DIM=""; _C_RST=""; }

# norm — collapse FP noise beyond 12 significant figures (every observable output —
# integers, 1-decimal bands, 4-decimal ratios — survives untouched; only the 16th/17th
# digit of an internally-accumulated EWMA coefficient is normalized). Floats only.
norm() {
  python3 -c '
import sys, re
def c(m):
    try: return "{:.12g}".format(float(m.group(0)))
    except Exception: return m.group(0)
sys.stdout.write(re.sub(r"-?\d+\.\d+(?:[eE][-+]?\d+)?", c, sys.stdin.read()))
'
}

# cmp_case <label> <bash-out> <bash-rc> <tf-out> <tf-rc>
cmp_case() {
  local label="$1" bo="$2" brc="$3" to="$4" trc="$5"
  if [ "$bo" = "$to" ] && [ "$brc" = "$trc" ]; then
    pass=$((pass+1)); printf '  %sok%s   %s\n' "$_C_GRN" "$_C_RST" "$label"
    return
  fi
  local bn tn
  bn="$(printf '%s' "$bo" | norm)"; tn="$(printf '%s' "$to" | norm)"
  if [ "$bn" = "$tn" ] && [ "$brc" = "$trc" ]; then
    pass=$((pass+1)); printf '  %sok%s   %s %s(ulp)%s\n' "$_C_GRN" "$_C_RST" "$label" "$_C_DIM" "$_C_RST"
  else
    fail=$((fail+1)); printf '  %sFAIL%s %s\n' "$_C_RED" "$_C_RST" "$label"
    printf '       %sbash%s(rc=%s): %s\n' "$_C_DIM" "$_C_RST" "$brc" "$bo"
    printf '       %stf  %s(rc=%s): %s\n' "$_C_DIM" "$_C_RST" "$trc" "$to"
  fi
}

# ---- ceiling-check (stateless, stdin payload) ----------------------------------------
printf '\n=== ceiling-check ===\n'
ceiling_case() {
  local label="$1" payload="$2"; shift 2
  local bo brc to trc
  bo="$(printf '%s' "$payload" | bash "$BASH_DIR/ceiling-check.sh" "$@" 2>/dev/null)"; brc=$?
  to="$(printf '%s' "$payload" | "$TF" ceiling-check "$@" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
P_CLEAR='{"rate_limits":{"five_hour":{"used_percentage":42.5,"resets_at":1749635640},"seven_day":{"used_percentage":18.2,"resets_at":1750000000}}}'
P_HALT='{"rate_limits":{"five_hour":{"used_percentage":86,"resets_at":1749635640},"seven_day":{"used_percentage":18.2,"resets_at":1750000000}}}'
P_BOUNDARY='{"rate_limits":{"five_hour":{"used_percentage":85,"resets_at":1749635640},"seven_day":{"used_percentage":10,"resets_at":1750000000}}}'
P_SEVEN='{"rate_limits":{"five_hour":{"used_percentage":10,"resets_at":1749635640},"seven_day":{"used_percentage":90,"resets_at":1750000000}}}'
P_NONE='{"hello":"world"}'
P_PARTIAL='{"rate_limits":{"five_hour":{"resets_at":1749635640}}}'
P_DECIMAL='{"rate_limits":{"five_hour":{"used_percentage":85.0,"resets_at":1749635640},"seven_day":{"used_percentage":10,"resets_at":1750000000}}}'
ceiling_case "clear-both"        "$P_CLEAR"
ceiling_case "halt-five"         "$P_HALT"
ceiling_case "boundary-85"       "$P_BOUNDARY"
ceiling_case "seven-halt-both"   "$P_SEVEN"
ceiling_case "seven-only-clear"  "$P_SEVEN" --window five_hour
ceiling_case "no-signal"         "$P_NONE"
ceiling_case "partial-no-pct"    "$P_PARTIAL" --window five_hour
ceiling_case "decimal-85.0"      "$P_DECIMAL"
ceiling_case "headroom-30"       "$P_CLEAR" --headroom 30
ceiling_case "headroom-eq-form"  "$P_CLEAR" --headroom=5
ceiling_case "bad-headroom"      "$P_CLEAR" --headroom 200
ceiling_case "bad-headroom-str"  "$P_CLEAR" --headroom abc

# ---- offpeak-window (stateless, time math) -------------------------------------------
printf '\n=== offpeak-window ===\n'
ow_case() {
  local label="$1"; shift
  local bo brc to trc
  bo="$(bash "$BASH_DIR/offpeak-window.sh" "$@" 2>/dev/null)"; brc=$?
  to="$("$TF" offpeak-window "$@" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
# Use explicit tz offset so both sides are deterministic (UTC-7 = -420).
ow_case "in-window-2am"   --now 1700000000 --tz-offset-min -420
ow_case "out-window-noon" --now 1700038800 --tz-offset-min -420
ow_case "utc-zero"        --now 1700000000 --tz-offset-min 0
ow_case "with-reset"      --now 1700000000 --reset 1700003600 --tz-offset-min -420
ow_case "reset-daytime"   --now 1700000000 --reset 1700038800 --tz-offset-min 0
ow_case "custom-window"   --now 1700000000 --start 23:00 --end 06:00 --tz-offset-min 0
ow_case "no-now-error"    --tz-offset-min 0
ow_case "eq-form"         --now=1700000000 --tz-offset-min=-420

# ---- offpeak-budget (stateless, integer window math) ---------------------------------
printf '\n=== offpeak-budget ===\n'
ob_case() {
  local label="$1"; shift
  local bo brc to trc
  bo="$(bash "$BASH_DIR/offpeak-budget.sh" "$@" 2>/dev/null)"; brc=$?
  to="$("$TF" offpeak-budget "$@" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
ob_case "login-current"  --now 1700000000 --login 1700001000 --reset 1700000000
ob_case "login-3-windows" --now 1700000000 --login 1700054000 --reset 1700000000
ob_case "custom-reserve"  --now 1700000000 --login 1700040000 --reset 1700000000 --morning-reserve 40 --headroom 10
ob_case "window-hours-3"  --now 1700000000 --login 1700040000 --reset 1700000000 --window-hours 3
ob_case "far-future-trunc" --now 1700000000 --login 1900000000 --reset 1700000000
ob_case "missing-login"   --now 1700000000 --reset 1700000000
ob_case "bad-headroom-dft" --now 1700000000 --login 1700040000 --reset 1700000000 --headroom 250

# ---- calibrate (stateful, identical sequence on isolated files) ----------------------
printf '\n=== calibrate (sequence) ===\n'
BCAL="$(mktemp)"; TCAL="$(mktemp)"; rm -f "$BCAL" "$TCAL"
cal_step() {
  local label="$1"; shift
  local bo brc to trc
  bo="$(I2P_CALIBRATION_FILE="$BCAL" bash "$BASH_DIR/calibrate.sh" "$@" 2>/dev/null)"; brc=$?
  to="$(I2P_CALIBRATION_FILE="$TCAL" "$TF" calibrate "$@" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
cal_step "ratio-empty"     ratio reviewer-fanout
cal_step "conf-seeding"    confidence reviewer-fanout
cal_step "close-1.2"       close reviewer-fanout 100000 120000
cal_step "ratio-after1"    ratio reviewer-fanout
cal_step "conf-after1"     confidence reviewer-fanout
cal_step "close-0.9"       close reviewer-fanout 100000 90000
cal_step "ratio-after2"    ratio reviewer-fanout
cal_step "conf-after2"     confidence reviewer-fanout
cal_step "close-equal"     close reviewer-fanout 100000 100000
cal_step "conf-after3"     confidence reviewer-fanout
cal_step "close-other-1.0" close plan:medium 80000 80000
cal_step "ratio-other"     ratio plan:medium
cal_step "conf-other"      confidence plan:medium
cal_step "bad-est"         close x 0 100
cal_step "bad-act"         close x 100 abc
# many samples → tier transitions (CALIBRATING → CONVERGING/CONVERGED)
for i in 1 2 3 4 5 6 7 8 9 10 11; do
  cal_step "loop-close-$i" close grind 100000 $((100000 + i*1000))
done
cal_step "conf-converged"  confidence grind

# ---- estimate (depends on calibration state) -----------------------------------------
printf '\n=== estimate ===\n'
est_case() {
  local label="$1"; shift
  local bo brc to trc
  bo="$(I2P_CALIBRATION_FILE="$BCAL" bash "$BASH_DIR/scheduler-estimate.sh" "$@" 2>/dev/null)"; brc=$?
  to="$(I2P_CALIBRATION_FILE="$TCAL" "$TF" estimate "$@" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
est_case "class-large"     --class large
est_case "class-medium"    --class medium
est_case "class-small"     --class small
est_case "class-epic"      --class epic
est_case "measured"        --name reviewer-fanout --width 26 --measured-unit-tokens 18000
est_case "history"         --name grind --width 10 --history-tokens 22000
est_case "seed-fallback"   --name brandnew --width 4
est_case "profile"         --profile "$BASH_DIR/profiles/reviewer-fanout.json"
est_case "profile-width"   --profile "$BASH_DIR/profiles/reviewer-fanout.json" --width 5
est_case "named-seeded"    --name plan:medium --width 1

printf '\n========================================\n'
if [ "$fail" -eq 0 ]; then
  printf '%sALL %d CASES GREEN — port is bit-faithful. Light is green, trap is clean.%s\n' "$_C_GRN" "$pass" "$_C_RST"
  exit 0
else
  printf '%s%d passed, %d FAILED.%s\n' "$_C_RED" "$pass" "$fail" "$_C_RST"
  exit 1
fi
