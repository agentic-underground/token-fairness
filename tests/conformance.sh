#!/usr/bin/env bash
# conformance.sh — differential gate: the same inputs through the ORIGINAL bash scheduler
# and through `tf`, asserting identical stdout + exit code. This is the no-regression proof
# that the Rust port preserves the lockout guard's behaviour bit-for-bit.
#
# The bash oracle is VENDORED — a frozen snapshot of idea-to-production's
# plugins/concierge/scheduler/ captured at the SHA in tests/oracle/SOURCE_SHA — because the
# scheduler was removed from idea-to-production (it lives here now). It is vendored under
# oracle/plugins/concierge/scheduler so the original repo-relative paths (the oscron wrapper
# at <repo>/plugins/concierge/scheduler/run-offpeak-job.sh) resolve unchanged. Override
# BASH_DIR only to diff against a different/live tree.
#
#   TF=/path/to/tf bash tests/conformance.sh
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BASH_DIR="${BASH_DIR:-$HERE/oracle/plugins/concierge/scheduler}"
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

# ---- calibrate + estimate: RETIRED from the differential (estimator family evolved) ------
# The estimator is no longer the bash's single fixed EWMA-0.4: it now runs a multi-algorithm
# ENSEMBLE (champion + blend) with online accuracy tracking and hierarchical-taxonomy backoff
# (knowledge/estimator-kaizen.md). So `calibrate close` (sample ≥3), `calibrate ratio`, and
# `estimate` (with seeded data) intentionally DIVERGE from the bash oracle. Per the approved plan
# (the user's "evolve the estimator" choice), these are proven instead by self-contained
# frozen-vector + unit tests: crates/tf-cli/tests/{cli.rs,stateful.rs} (calibrate_sequence,
# convergence_band_tightens, estimate_vectors, ensemble_*) and crates/tf-core/src/ensemble.rs
# unit tests. The first-sample / no-data paths remain bash-identical and are frozen there.
printf '\n=== calibrate + estimate ===\n'
printf '  %sskip%s estimator family (multi-algorithm ensemble — proven by cargo frozen vectors)\n' "$_C_DIM" "$_C_RST"

# ======================================================================================
# STATEFUL + ORCHESTRATION TIER (plan §2.8) — side-effecting verbs. The pure-stdout model
# is insufficient here: we diff stdout+rc AND a canonicalised dump of every written state
# file, in isolated HOME/dirs, with a PINNED clock. Crontab is redirected via $I2P_CRONTAB
# to a fake — NEVER the real crontab.
# ======================================================================================

# Pin the oracle to a known SHA (review S4). The oracle is vendored frozen, so its provenance
# is recorded in tests/oracle/SOURCE_SHA rather than read from a live git checkout.
ORACLE_SHA_PINNED="0b46ff35cb746ad14ac165431f93dcb613b517a8"
ORACLE_SHA_NOW="$(cat "$HERE/oracle/SOURCE_SHA" 2>/dev/null | tr -d '[:space:]' || echo unknown)"
if [ "$ORACLE_SHA_NOW" != "$ORACLE_SHA_PINNED" ]; then
  printf '\n%s⚠ oracle drift%s: pinned %s but vendored %s — re-validate before trusting green.\n' \
    "$_C_DIM" "$_C_RST" "${ORACLE_SHA_PINNED:0:12}" "${ORACLE_SHA_NOW:0:12}"
fi

CLK=1700000000   # the single authoritative clock for every freshness/window case below

# canon — jq-canonicalise a state file: sort keys AND numeric-normalise (so jq's preserved
# literal `1.0500` and Rust's `1.05` compare equal; the documented EWMA-literal caveat).
canon() { jq -S 'walk(if type=="number" then .+0 else . end)' "$1" 2>/dev/null; }

# cmp_state <label> <bash-file> <tf-file>
cmp_state() {
  local label="$1" bf="$2" tf="$3"
  if diff <(canon "$bf") <(canon "$tf") >/dev/null 2>&1; then
    pass=$((pass+1)); printf '  %sok%s   %s %s(state)%s\n' "$_C_GRN" "$_C_RST" "$label" "$_C_DIM" "$_C_RST"
  else
    fail=$((fail+1)); printf '  %sFAIL%s %s %s(state)%s\n' "$_C_RED" "$_C_RST" "$label" "$_C_DIM" "$_C_RST"
    diff <(canon "$bf") <(canon "$tf") | sed 's/^/       /'
  fi
}

ST="$(mktemp -d)"; BH="$ST/bh"; TH="$ST/th"; mkdir -p "$BH" "$TH"
trap 'rm -rf "$ST"' EXIT

# ---- ledger (job-ledger.sh) ----------------------------------------------------------
# Every step runs the SAME inputs through both tools, bash always into $BH, tf into $TH.
printf '\n=== ledger ===\n'
ledq() { local label="$1" sub="$2"; shift 2
  local bo brc to trc
  bo="$(bash "$BASH_DIR/job-ledger.sh" "$sub" "$BH" job-1 "$@" 2>/dev/null)"; brc=$?
  to="$("$TF" ledger "$sub" "$TH" job-1 "$@" 2>/dev/null)"; trc=$?
  # init/status echo the ledger path, which differs by dir (bh vs th) — normalise it.
  bo="${bo//$BH/D}"; to="${to//$TH/D}"
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
ledq "init"        init reviewer-fanout "u1, u2 ,u3,,u4" 500000 15
ledq "mark-done"   mark-done   u2
ledq "mark-failed" mark-failed u3
ledq "remaining"   remaining
ledq "pause"       pause ceiling 85 1700003600 123456 "$CLK"
ledq "resume"      resume
ledq "set-offpeak" set-offpeak 23:00 07:00 -420
ledq "set-pointer" set-pointer derived_plan ./.i2p/plan.md
ledq "status"      status
ledq "no-ledger"   mark-done x   # against job-1 it exists; test a missing one:
bo="$(bash "$BASH_DIR/job-ledger.sh" status "$BH" ghost 2>/dev/null)"; brc=$?
to="$("$TF" ledger status "$TH" ghost 2>/dev/null)"; trc=$?
cmp_case "missing-ledger" "$bo" "$brc" "$to" "$trc"
cmp_state "ledger-file" "$BH/.i2p/jobs/job-1.json" "$TH/.i2p/jobs/job-1.json"

# ---- registry (jobs-registry.sh) -----------------------------------------------------
printf '\n=== registry ===\n'
BM="$ST/bmach.json"; TM="$ST/tmach.json"
# Two separate project dirs (so the project files don't collide) but identical inputs.
RB="$ST/pb"; RT="$ST/pt"; mkdir -p "$RB" "$RT"
regpair() { local label="$1" sub="$2"; shift 2
  local bo brc to trc
  bo="$(I2P_MACHINE_REGISTRY="$BM" bash "$BASH_DIR/jobs-registry.sh" "$sub" "$RB" "$@" 2>/dev/null)"; brc=$?
  to="$(I2P_MACHINE_REGISTRY="$TM" "$TF" registry "$sub" "$RT" "$@" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
regpair "register-1" register j1 "17 22 * * *" 300000 ./.i2p/jobs/j1.json ./p.txt "nightly"
regpair "register-2" register j2 "0 3 * * 0" 500 ./.i2p/jobs/j2.json ./w.txt ""
regpair "arm-oscron"  arm j1 oscron
regpair "arm-session" arm j2 session
regpair "reset-armed" reset-armed
regpair "list"        list
regpair "get"         get j1
regpair "get-missing" get nope
regpair "remove"      remove j2
cmp_state "registry-proj" "$RB/.i2p/scheduled-jobs.json" "$RT/.i2p/scheduled-jobs.json"
# Machine registry: repo key differs by dir, so normalise it away before comparing.
jq -S 'walk(if type=="object" and has("repo") then .repo="REPO" else . end)' "$BM" > "$ST/bm.norm" 2>/dev/null
jq -S 'walk(if type=="object" and has("repo") then .repo="REPO" else . end)' "$TM" > "$ST/tm.norm" 2>/dev/null
if diff "$ST/bm.norm" "$ST/tm.norm" >/dev/null 2>&1; then
  pass=$((pass+1)); printf '  %sok%s   registry-machine %s(state, repo-normalised)%s\n' "$_C_GRN" "$_C_RST" "$_C_DIM" "$_C_RST"
else fail=$((fail+1)); printf '  %sFAIL%s registry-machine\n' "$_C_RED" "$_C_RST"; diff "$ST/bm.norm" "$ST/tm.norm" | sed 's/^/       /'; fi

# ---- snapshot (ratelimit-snapshot.sh) ------------------------------------------------
printf '\n=== snapshot ===\n'
SNB="$ST/snb"; SNT="$ST/snt"; mkdir -p "$SNB" "$SNT"
PAY_SIG='{"hook_event_name":"PostToolUse","rate_limits":{"five_hour":{"used_percentage":42.5,"resets_at":1749635640},"seven_day":{"used_percentage":18.2,"resets_at":1750000000}},"cost":{"total_cost_usd":1.23}}'
bo="$(printf '%s' "$PAY_SIG" | I2P_COST_STATE_DIR="$SNB" bash "$BASH_DIR/ratelimit-snapshot.sh" 2>/dev/null)"; brc=$?
to="$(printf '%s' "$PAY_SIG" | I2P_COST_STATE_DIR="$SNT" I2P_CLOCK="$CLK" "$TF" snapshot 2>/dev/null)"; trc=$?
cmp_case "snapshot-write" "$bo" "$brc" "$to" "$trc"
# captured_at/concluded_at are wall-clock on the bash side — pin them before comparing.
for f in "$SNB/ratelimit-snapshot.json" "$SNT/ratelimit-snapshot.json"; do jq '.captured_at=0' "$f" > "$f.p" && mv "$f.p" "$f"; done
for f in "$SNB/signal-findings.json" "$SNT/signal-findings.json"; do jq '.concluded_at=0' "$f" > "$f.p" && mv "$f.p" "$f"; done
cmp_state "snapshot-file" "$SNB/ratelimit-snapshot.json" "$SNT/ratelimit-snapshot.json"
cmp_state "snapshot-findings" "$SNB/signal-findings.json" "$SNT/signal-findings.json"
# no-signal payload → no-op, no file written
bo="$(printf '%s' '{"hello":1}' | I2P_COST_STATE_DIR="$ST/nb" bash "$BASH_DIR/ratelimit-snapshot.sh" 2>/dev/null; echo "rc=$?:$([ -e "$ST/nb/ratelimit-snapshot.json" ] && echo file || echo nofile)")"
to="$(printf '%s' '{"hello":1}' | I2P_COST_STATE_DIR="$ST/nt" "$TF" snapshot 2>/dev/null; echo "rc=$?:$([ -e "$ST/nt/ratelimit-snapshot.json" ] && echo file || echo nofile)")"
cmp_case "snapshot-no-signal" "$bo" 0 "$to" 0

# ---- verify-payload + signal (verify-payload.sh / signal-probe.sh) -------------------
printf '\n=== verify-payload + signal ===\n'
VB="$ST/vb"; VT="$ST/vt"; mkdir -p "$VB" "$VT"
for p in '{"hook_event_name":"PreToolUse","tool_name":"Task","rate_limits":{"five_hour":{"used_percentage":50}}}' \
         '{"hook_event_name":"PreToolUse","tool_name":"Bash"}' \
         '{"hook_event_name":"Stop","cost":{"total_cost_usd":2.5}}'; do
  printf '%s' "$p" | I2P_COST_STATE_DIR="$VB" bash "$BASH_DIR/verify-payload.sh" 2>/dev/null
  printf '%s' "$p" | I2P_COST_STATE_DIR="$VT" I2P_CLOCK="$CLK" "$TF" verify-payload 2>/dev/null
done
# Probe log: pin `at`, compare line-by-line canonical.
jq -cS 'del(.at)' "$VB/payload-probe.jsonl" > "$ST/pb.norm" 2>/dev/null
jq -cS 'del(.at)' "$VT/payload-probe.jsonl" > "$ST/pt.norm" 2>/dev/null
if diff "$ST/pb.norm" "$ST/pt.norm" >/dev/null 2>&1; then pass=$((pass+1)); printf '  %sok%s   probe-log %s(state)%s\n' "$_C_GRN" "$_C_RST" "$_C_DIM" "$_C_RST"
else fail=$((fail+1)); printf '  %sFAIL%s probe-log\n' "$_C_RED" "$_C_RST"; diff "$ST/pb.norm" "$ST/pt.norm" | sed 's/^/       /'; fi
SB="$VB/sf.json"; SFT="$VT/sf.json"
sigq() { local label="$1"; shift
  local bo brc to trc
  bo="$(I2P_PAYLOAD_PROBE="$VB/payload-probe.jsonl" I2P_SIGNAL_FINDINGS="$SB" bash "$BASH_DIR/signal-probe.sh" "$@" 2>/dev/null)"; brc=$?
  to="$(I2P_PAYLOAD_PROBE="$VT/payload-probe.jsonl" I2P_SIGNAL_FINDINGS="$SFT" I2P_CLOCK="$CLK" "$TF" signal "$@" 2>/dev/null)"; trc=$?
  # `conclude` echoes the findings path, which legitimately differs (vb vs vt) — normalise it.
  bo="${bo//$SB/SF}"; to="${to//$SFT/SF}"
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
sigq "conclude" conclude
sigq "verdict"  verdict
sigq "report"   report
for f in "$SB" "$SFT"; do jq '.concluded_at=0' "$f" > "$f.p" && mv "$f.p" "$f"; done
cmp_state "signal-findings" "$SB" "$SFT"

# ---- report (report.sh) --------------------------------------------------------------
printf '\n=== report ===\n'
RCAL="$ST/rcal.json"
for n in 1 2 3 4 5 6; do I2P_CALIBRATION_FILE="$RCAL" "$TF" calibrate close reviewer-fanout 100000 $((100000+n*2000)) >/dev/null; done
I2P_CALIBRATION_FILE="$RCAL" "$TF" calibrate close plan:medium 80000 84000 >/dev/null
RPB="$ST/rpb"; RPT="$ST/rpt"; mkdir -p "$RPB" "$RPT"
RREG="I2P_MACHINE_REGISTRY=$ST/rmach.json"
for d in "$RPB" "$RPT"; do
  env $RREG bash "$BASH_DIR/jobs-registry.sh" register "$d" nightly "17 22 * * *" 1500000 ./.i2p/jobs/nightly.json ./p.txt "the big fan-out" >/dev/null
  env $RREG bash "$BASH_DIR/jobs-registry.sh" arm "$d" nightly oscron >/dev/null
  bash "$BASH_DIR/job-ledger.sh" init "$d" nightly reviewer-fanout "a,b,c,d" 1500000 15 >/dev/null
  bash "$BASH_DIR/job-ledger.sh" mark-done "$d" nightly a >/dev/null
  bash "$BASH_DIR/job-ledger.sh" pause "$d" nightly ceiling 85 1700003600 99000 "$CLK" >/dev/null
done
RSIG="$ST/rsig.json"; printf '{"verdict":"hook-signal-available","guard_mode":"live-ceiling","events":{}}' > "$RSIG"
repq() { local label="$1" dir="$2"; shift 2
  local bo brc to trc
  bo="$(I2P_CALIBRATION_FILE="$RCAL" I2P_SIGNAL_FINDINGS="$RSIG" bash "$BASH_DIR/report.sh" "$dir" "$@" 2>/dev/null)"; brc=$?
  to="$(I2P_CALIBRATION_FILE="$RCAL" I2P_SIGNAL_FINDINGS="$RSIG" "$TF" report "$dir" "$@" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
repq "report-scheduled" "$RPB" --scheduled
repq "report-estimator" "$RPB" --estimator
repq "report-brief"     "$RPB" --brief
repq "report-full"      "$RPB"
repq "report-empty-brief" "$ST/emptyrepo" --brief

# ---- gate / plan / convergence loop (scheduler.sh) -----------------------------------
printf '\n=== gate (freshness boundaries §2.8.5) ===\n'
gate_fresh() { local label="$1" age="$2"
  local sd="$ST/g$label" cap=1700000000 clk; clk=$((cap+age)); mkdir -p "$sd"
  printf '{"captured_at":%s,"rate_limits":{"five_hour":{"used_percentage":42,"resets_at":1750000000}}}' "$cap" > "$sd/ratelimit-snapshot.json"
  local bo brc to trc
  bo="$(printf '%s' '{"none":1}' | I2P_COST_STATE_DIR="$sd" bash "$BASH_DIR/scheduler.sh" gate --clock "$clk" 2>/dev/null)"; brc=$?
  to="$(printf '%s' '{"none":1}' | I2P_COST_STATE_DIR="$sd" "$TF" gate --clock "$clk" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
gate_fresh "age-0"    0
gate_fresh "age-900"  900
gate_fresh "age-901"  901
gate_fresh "age-neg"  -10

printf '\n=== plan / preflight / convergence loop ===\n'
PLCAL="$ST/plcal.json"
for n in 1 2 3 4 5 6; do I2P_CALIBRATION_FILE="$PLCAL" "$TF" calibrate close reviewer-fanout 100000 $((100000+n*2000)) >/dev/null; done
schq() { local label="$1"; shift
  local bo brc to trc
  bo="$(I2P_CALIBRATION_FILE="$PLCAL" bash "$BASH_DIR/scheduler.sh" "$@" 2>/dev/null)"; brc=$?
  to="$(I2P_CALIBRATION_FILE="$PLCAL" "$TF" "$@" 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
schq "preflight-class"  preflight --class large
# (preflight-meas / plan-named retired — they embed a seeded estimate, which the ensemble evolves;
#  the no-data class paths below stay bash-identical.)
schq "plan-peak-large"  plan --class large --now 1700038800 --tz-offset-min 0
schq "plan-offpeak"     plan --class large --now "$CLK" --tz-offset-min -420

# Full convergence loop (review C3): plan-open → session delta → plan-close → EWMA advanced.
LCB="$ST/lcb.json"; LCT="$ST/lct.json"   # calibration files, bash vs tf
loop_side() { # <bin...> <SESSION> <POPEN> <CAL>
  local bin="$1" sess="$2" pop="$3" cal="$4"
  printf '{"tokens":1000}' > "$sess"
  env I2P_SESSION_FILE="$sess" I2P_PLANOPEN_FILE="$pop" I2P_CALIBRATION_FILE="$cal" $bin plan-open medium 80000
  printf '{"tokens":85000}' > "$sess"
  env I2P_SESSION_FILE="$sess" I2P_PLANOPEN_FILE="$pop" I2P_CALIBRATION_FILE="$cal" $bin plan-close
}
bo="$(loop_side "bash $BASH_DIR/scheduler.sh" "$ST/sb.json" "$ST/pob.json" "$LCB" 2>/dev/null)"; brc=$?
to="$(loop_side "$TF" "$ST/st.json" "$ST/pot.json" "$LCT" 2>/dev/null)"; trc=$?
cmp_case "convergence-loop-stdout" "$bo" "$brc" "$to" "$trc"
# (convergence-loop-cal state-diff retired: the tf calibration file now carries ensemble fields
#  the bash lacks; the convergence advance is asserted by `samples` below + the cargo loop test.)
ewma_after="$(jq -r '."plan:medium".samples' "$LCT" 2>/dev/null)"
if [ "$ewma_after" = "1" ]; then pass=$((pass+1)); printf '  %sok%s   convergence-advanced %s(EWMA folded 1 sample)%s\n' "$_C_GRN" "$_C_RST" "$_C_DIM" "$_C_RST"
else fail=$((fail+1)); printf '  %sFAIL%s convergence-advanced (samples=%s)\n' "$_C_RED" "$_C_RST" "$ewma_after"; fi

# preflight-fanout deny on HALT
printf '\n=== preflight-fanout ===\n'
pff() { local label="$1" pay="$2"
  local bo brc to trc
  bo="$(printf '%s' "$pay" | bash "$BASH_DIR/preflight-fanout.sh" 2>/dev/null)"; brc=$?
  to="$(printf '%s' "$pay" | "$TF" preflight-fanout 2>/dev/null)"; trc=$?
  cmp_case "$label" "$bo" "$brc" "$to" "$trc"
}
pff "deny-halt"  '{"rate_limits":{"five_hour":{"used_percentage":92,"resets_at":1750000000}}}'
pff "allow-clear" '{"rate_limits":{"five_hour":{"used_percentage":40,"resets_at":1750000000}}}'

# ---- oscron (install-oscron.sh) via FAKE crontab -------------------------------------
printf '\n=== oscron (fake crontab) ===\n'
FC="$ST/fakecron"
cat > "$FC" <<'CRON'
#!/usr/bin/env bash
# Faithful to real crontab: `-` buffers ALL of stdin then atomically replaces the spool, so
# the `crontab -l` at the head of a `{ crontab -l|…; echo}|crontab -` pipe reads intact.
S="${FAKECRON_STORE:?}"
case "$1" in (-l) [ -f "$S" ] && cat "$S" || exit 1 ;; (-) t="$(mktemp)"; cat > "$t"; mv "$t" "$S" ;; esac
CRON
chmod +x "$FC"
OREPO="$(cd "$BASH_DIR/../../.." && pwd)"   # the i2p repo root (has the bash wrapper)
WRAP="$BASH_DIR/run-offpeak-job.sh"
OSB="$ST/osb.cron"; OST="$ST/ost.cron"
osc_b() { FAKECRON_STORE="$OSB" I2P_CRONTAB="$FC" bash "$BASH_DIR/install-oscron.sh" "$@"; }
osc_t() { FAKECRON_STORE="$OST" I2P_CRONTAB="$FC" I2P_OFFPEAK_WRAPPER="$WRAP" "$TF" oscron "$@"; }
# install twice (idempotent), compare stdout of the 2nd + the resulting crontab store.
osc_b "$OREPO" nightly >/dev/null; osc_b "$OREPO" nightly >/dev/null
osc_t install "$OREPO" nightly >/dev/null; osc_t install "$OREPO" nightly >/dev/null
bo="$(osc_b "$OREPO" weekly "0 3 * * 0")"; brc=$?
to="$(osc_t install "$OREPO" weekly "0 3 * * 0")"; trc=$?
cmp_case "oscron-install" "$bo" "$brc" "$to" "$trc"
if diff "$OSB" "$OST" >/dev/null 2>&1; then pass=$((pass+1)); printf '  %sok%s   oscron-crontab %s(state)%s\n' "$_C_GRN" "$_C_RST" "$_C_DIM" "$_C_RST"
else fail=$((fail+1)); printf '  %sFAIL%s oscron-crontab\n' "$_C_RED" "$_C_RST"; diff "$OSB" "$OST" | sed 's/^/       /'; fi
bo="$(osc_b --uninstall nightly)"; brc=$?
to="$(osc_t uninstall nightly)"; trc=$?
cmp_case "oscron-uninstall" "$bo" "$brc" "$to" "$trc"
diff "$OSB" "$OST" >/dev/null 2>&1 && { pass=$((pass+1)); printf '  %sok%s   oscron-crontab-after-uninstall %s(state)%s\n' "$_C_GRN" "$_C_RST" "$_C_DIM" "$_C_RST"; } || { fail=$((fail+1)); printf '  %sFAIL%s oscron-crontab-after-uninstall\n' "$_C_RED" "$_C_RST"; }

printf '\n========================================\n'
if [ "$fail" -eq 0 ]; then
  printf '%sALL %d CASES GREEN — port is bit-faithful. Light is green, trap is clean.%s\n' "$_C_GRN" "$pass" "$_C_RST"
  exit 0
else
  printf '%s%d passed, %d FAILED.%s\n' "$_C_RED" "$pass" "$fail" "$_C_RST"
  exit 1
fi
