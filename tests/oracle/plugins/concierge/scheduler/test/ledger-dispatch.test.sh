#!/usr/bin/env bash
# Tests for job-ledger.sh (resume state machine) and scheduler.sh (verdict composition).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
. "${HERE}/lib/harness.sh"
JL="${HERE}/../job-ledger.sh"
SCH="${HERE}/../scheduler.sh"
EST_PROFILE="${HERE}/../profiles/reviewer-fanout.json"
jget() { printf '%s' "$1" | jq -r "$2"; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
export I2P_CALIBRATION_FILE="${TMP}/cal.json"   # isolate calibration for preflight
JID="reviewer-fanout-test"

payload() { printf '{"rate_limits":{"five_hour":{"used_percentage":%s,"resets_at":1749635640},"seven_day":{"used_percentage":%s,"resets_at":1750000000}}}' "$1" "$2"; }

# ---------- job-ledger.sh ----------
test_case "init creates a ledger with all units remaining"
bash "$JL" init "$TMP" "$JID" reviewer-fanout "a,b,c, d ,," 500000 15 >/dev/null
out="$(bash "$JL" status "$TMP" "$JID")"
assert_eq "$(jget "$out" '.units.total')" "4"          # blanks dropped
assert_eq "$(jget "$out" '.units.remaining | length')" "4"
assert_eq "$(jget "$out" '.state')" "running"
assert_eq "$(jget "$out" '.budget_total')" "500000"

test_case "mark-done moves a unit and is idempotent"
bash "$JL" mark-done "$TMP" "$JID" b >/dev/null
bash "$JL" mark-done "$TMP" "$JID" b >/dev/null   # twice → still once
out="$(bash "$JL" status "$TMP" "$JID")"
assert_eq "$(jget "$out" '.units.done | length')" "1"
assert_eq "$(jget "$out" '.units.remaining | length')" "3"

test_case "remaining lists only what's left (the cheap-resume worklist)"
out="$(bash "$JL" remaining "$TMP" "$JID" | tr '\n' ',')"
assert_eq "$out" "a,c,d,"

test_case "mark-failed records failure and removes from remaining"
bash "$JL" mark-failed "$TMP" "$JID" c >/dev/null
out="$(bash "$JL" status "$TMP" "$JID")"
assert_eq "$(jget "$out" '.units.failed | join(",")')" "c"
assert_eq "$(jget "$out" '.units.remaining | join(",")')" "a,d"

test_case "pause sets state and records a checkpoint with the live ceiling snapshot"
bash "$JL" pause "$TMP" "$JID" ceiling 86 1749635640 312000 1700000000 >/dev/null
out="$(bash "$JL" status "$TMP" "$JID")"
assert_eq "$(jget "$out" '.state')" "paused"
assert_eq "$(jget "$out" '.checkpoints | length')" "1"
assert_eq "$(jget "$out" '.checkpoints[0].reason')" "ceiling"
assert_eq "$(jget "$out" '.checkpoints[0].five_hour_pct')" "86"
assert_eq "$(jget "$out" '.checkpoints[0].units_done')" "1"

test_case "resume flips state back to running, checkpoint history preserved"
bash "$JL" resume "$TMP" "$JID" >/dev/null
out="$(bash "$JL" status "$TMP" "$JID")"
assert_eq "$(jget "$out" '.state')" "running"
assert_eq "$(jget "$out" '.checkpoints | length')" "1"

test_case "set-offpeak and set-pointer persist"
bash "$JL" set-offpeak "$TMP" "$JID" 22:00 08:00 -420 >/dev/null
bash "$JL" set-pointer "$TMP" "$JID" cached_reviews_dir doc/cached-reviews >/dev/null
out="$(bash "$JL" status "$TMP" "$JID")"
assert_eq "$(jget "$out" '.offpeak_window.tz_offset_min')" "-420"
assert_eq "$(jget "$out" '.context_pointers.cached_reviews_dir')" "doc/cached-reviews"

test_case "status on a missing job errors (exit 2)"
assert_exit 2 bash "$JL" status "$TMP" no-such-job

# ---------- scheduler.sh gate ----------
test_case "gate CONTINUE when 5h window is clear"
assert_pipe_exit 0 "$(payload 40 10)" bash "$SCH" gate
assert_contains "$ASSERT_OUT" '"verdict":"CONTINUE"'

test_case "gate HALT when ceiling reached"
assert_pipe_exit 10 "$(payload 90 10)" bash "$SCH" gate
assert_contains "$ASSERT_OUT" '"verdict":"HALT"'

test_case "gate ASK (fail closed) when no live signal"
assert_pipe_exit 20 '{"cost":{}}' bash "$SCH" gate
assert_contains "$ASSERT_OUT" '"verdict":"ASK"'

test_case "gate DEFER when require-offpeak and we are in peak (14:00)"
assert_pipe_exit 4 "$(payload 40 10)" bash "$SCH" gate --require-offpeak --now 50400 --tz-offset-min 0
assert_contains "$ASSERT_OUT" '"verdict":"DEFER"'

test_case "gate CONTINUE when require-offpeak and we ARE off-peak (02:00)"
assert_pipe_exit 0 "$(payload 40 10)" bash "$SCH" gate --require-offpeak --now 7200 --tz-offset-min 0
assert_contains "$ASSERT_OUT" '"verdict":"CONTINUE"'

test_case "gate HALT takes precedence over off-peak DEFER"
assert_pipe_exit 10 "$(payload 90 10)" bash "$SCH" gate --require-offpeak --now 50400 --tz-offset-min 0

# ---------- snapshot fallback (the live→disk bridge for non-hook Bash calls) ----------
SNAP="${TMP}/ratelimit-snapshot.json"
export I2P_RATELIMIT_SNAPSHOT="$SNAP"
mk_snap() { printf '{"captured_at":%s,"rate_limits":{"five_hour":{"used_percentage":%s,"resets_at":1749635640},"seven_day":{"used_percentage":10,"resets_at":1750000000}}}' "$1" "$2" > "$SNAP"; }

test_case "no stdin signal but FRESH snapshot at 90% → HALT (reads the bridge)"
mk_snap 1000000 90
assert_pipe_exit 10 '' bash "$SCH" gate --clock 1000300   # 5 min old, within 900s
assert_contains "$ASSERT_OUT" '"verdict":"HALT"'

test_case "no stdin signal but FRESH snapshot at 40% → CONTINUE"
mk_snap 1000000 40
assert_pipe_exit 0 '' bash "$SCH" gate --clock 1000300

test_case "STALE snapshot (older than max-age) → ASK, fail closed (never trust stale)"
mk_snap 1000000 40
assert_pipe_exit 20 '' bash "$SCH" gate --clock 1002000   # ~16 min old > 900s default
assert_contains "$ASSERT_OUT" '"verdict":"ASK"'

test_case "piped stdin signal WINS over snapshot"
mk_snap 1000000 40   # snapshot says clear...
assert_pipe_exit 10 "$(payload 90 10)" bash "$SCH" gate --clock 1000300   # ...but live stdin says halt
assert_contains "$ASSERT_OUT" '"verdict":"HALT"'
unset I2P_RATELIMIT_SNAPSHOT

# ---------- scheduler.sh preflight ----------
test_case "preflight PROBE when estimate confidence is low (declared basis)"
assert_exit 3 bash "$SCH" preflight --profile "$EST_PROFILE"
assert_contains "$ASSERT_OUT" '"verdict":"PROBE"'

test_case "preflight CONTINUE when a real measurement makes it high-confidence"
assert_exit 0 bash "$SCH" preflight --profile "$EST_PROFILE" --measured-unit-tokens 18000
assert_contains "$ASSERT_OUT" '"verdict":"CONTINUE"'

# ---------- 2.B scheduler.sh plan (ANY plan) + bracketing ----------
test_case "plan --class medium in PEAK → RUN NOW (under defer threshold)"
out="$(bash "$SCH" plan --class medium --now 50400 --tz-offset-min 0 | tail -1)"
assert_eq "$(jget "$out" .decision)" "RUN NOW"
assert_eq "$(jget "$out" .est_total)" "80000"

test_case "plan --class epic in PEAK → DEFER (exit 4)"
ASSERT_OUT="$(bash "$SCH" plan --class epic --now 50400 --tz-offset-min 0 | tail -1)"; rc_out="$ASSERT_OUT"
assert_exit 4 bash "$SCH" plan --class epic --now 50400 --tz-offset-min 0
assert_eq "$(jget "$rc_out" .decision)" "DEFER"

test_case "plan --class epic OFF-PEAK → RUN NOW"
out="$(bash "$SCH" plan --class epic --now 7200 --tz-offset-min 0 | tail -1)"
assert_eq "$(jget "$out" .decision)" "RUN NOW"
assert_eq "$(jget "$out" .in_offpeak)" "true"

test_case "plan banner is human-readable (💰 + 🕒 lines present)"
out="$(bash "$SCH" plan --class large --now 7200 --tz-offset-min 0)"
assert_contains "$out" "💰"
assert_contains "$out" "🕒 Schedule:"

test_case "bracket round-trip captures the session.json delta as actual → feeds convergence"
SESS="${TMP}/session.json"; POPEN="${TMP}/popen.json"
export I2P_SESSION_FILE="$SESS" I2P_PLANOPEN_FILE="$POPEN"
printf '{"tokens":1000000}' > "$SESS"
bash "$SCH" plan-open medium 80000 >/dev/null
printf '{"tokens":1095000}' > "$SESS"   # 95k spent during the plan
out="$(bash "$SCH" plan-close)"
assert_eq "$(jget "$out" .actual)" "95000"
assert_eq "$(jget "$out" '.convergence.samples')" "1"

test_case "plan-close with no open plan errors cleanly (exit 2)"
rm -f "$POPEN"
assert_exit 2 bash "$SCH" plan-close
unset I2P_SESSION_FILE I2P_PLANOPEN_FILE

# ---------- 2.C automatic guard: preflight-fanout.sh (PreToolUse Agent|Task backstop) ----------
PF="${HERE}/../preflight-fanout.sh"
test_case "preflight-fanout DENIES a spawn when the live ceiling is breached (90%)"
out="$(printf '%s' "$(payload 90 10)" | bash "$PF" 2>/dev/null)"
assert_contains "$out" '"permissionDecision":"deny"'

test_case "preflight-fanout stays SILENT (allows) when clear (40%)"
out="$(printf '%s' "$(payload 40 10)" | bash "$PF" 2>/dev/null)"
assert_eq "$out" ""

finish
