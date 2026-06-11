#!/usr/bin/env bash
# Tests for signal-probe.sh (verdict from evidence), the calibration trend arrow, and install-oscron.sh
# idempotency/uninstall (against a FAKE crontab — never touches the real one).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
. "${HERE}/lib/harness.sh"
SP="${HERE}/../signal-probe.sh"
CAL_SH="${HERE}/../calibrate.sh"
OSCRON="${HERE}/../install-oscron.sh"
jget() { printf '%s' "$1" | jq -r "$2"; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

# ---------- signal-probe.sh ----------
export I2P_PAYLOAD_PROBE="${TMP}/probe.jsonl"
export I2P_SIGNAL_FINDINGS="${TMP}/findings.json"

test_case "conclude with NO rate_limits in any capture → no-hook-signal / budget-cap"
printf '%s\n' \
  '{"hook_event":"PreToolUse","has_rate_limits":false,"has_cost":false}' \
  '{"hook_event":"Stop","has_rate_limits":false,"has_cost":false}' > "$I2P_PAYLOAD_PROBE"
bash "$SP" conclude >/dev/null
assert_eq "$(jget "$(cat "$I2P_SIGNAL_FINDINGS")" .verdict)" "no-hook-signal"
assert_eq "$(jget "$(cat "$I2P_SIGNAL_FINDINGS")" .guard_mode)" "budget-cap"
assert_eq "$(bash "$SP" verdict)" "no-hook-signal"

test_case "conclude WITH a rate_limits capture → hook-signal-available / live-ceiling"
printf '%s\n' \
  '{"hook_event":"Stop","has_rate_limits":true,"has_cost":true}' \
  '{"hook_event":"PreToolUse","has_rate_limits":false}' > "$I2P_PAYLOAD_PROBE"
bash "$SP" conclude >/dev/null
assert_eq "$(jget "$(cat "$I2P_SIGNAL_FINDINGS")" .verdict)" "hook-signal-available"
assert_eq "$(jget "$(cat "$I2P_SIGNAL_FINDINGS")" '.events.Stop.with_rate_limits')" "1"

test_case "verdict is 'unknown' before any conclusion"
rm -f "$I2P_SIGNAL_FINDINGS"
assert_eq "$(bash "$SP" verdict)" "unknown"
unset I2P_PAYLOAD_PROBE I2P_SIGNAL_FINDINGS

# ---------- trend arrow ----------
export I2P_CALIBRATION_FILE="${TMP}/cal.json"
test_case "band tightens after a spread → trend 'improving', current < prev_band"
bash "$CAL_SH" close p 1000 1000 >/dev/null
bash "$CAL_SH" close p 1000 2000 >/dev/null            # widen
for i in $(seq 1 8); do bash "$CAL_SH" close p 1000 1500 >/dev/null; done   # tighten toward mean
c="$(bash "$CAL_SH" confidence p)"
assert_eq "$(jget "$c" .trend)" "improving"
awk -v b="$(jget "$c" .p95_band_pct)" -v p="$(jget "$c" .prev_band)" 'BEGIN{exit !(b<p)}' \
  && _pass "band $(jget "$c" .p95_band_pct) < prev $(jget "$c" .prev_band)" || _fail "band not < prev"
unset I2P_CALIBRATION_FILE

# ---------- install-oscron.sh against a fake crontab ----------
export FAKECRON_STORE="${TMP}/crontab.txt"
cat > "${TMP}/fakecron" <<'EOF'
#!/usr/bin/env bash
# Mimic real crontab atomicity: -l reads the spool, - replaces it via temp+mv (no truncation race
# when a pipeline reads (-l) and writes (-) the same store concurrently).
case "${1:-}" in
  -l) cat "$FAKECRON_STORE" 2>/dev/null ;;
  -)  t="$(mktemp)"; cat > "$t"; mv "$t" "$FAKECRON_STORE" ;;
esac
EOF
chmod +x "${TMP}/fakecron"
export I2P_CRONTAB="bash ${TMP}/fakecron"
REPO="$(cd "$HERE/../../../.." && pwd)"   # repo root (test→scheduler→concierge→plugins→root)
printf '0 5 * * * echo unrelated\n' > "$FAKECRON_STORE"   # a pre-existing unrelated entry

test_case "install adds exactly one tagged line, preserving the unrelated entry"
bash "$OSCRON" "$REPO" uplift-cached-reviews >/dev/null
assert_eq "$(grep -c 'i2p-scheduler:uplift-cached-reviews' "$FAKECRON_STORE")" "1"
assert_eq "$(grep -c 'unrelated' "$FAKECRON_STORE")" "1"

test_case "re-install is idempotent (still one tagged line)"
bash "$OSCRON" "$REPO" uplift-cached-reviews "30 2 * * *" >/dev/null
assert_eq "$(grep -c 'i2p-scheduler:uplift-cached-reviews' "$FAKECRON_STORE")" "1"
assert_contains "$(cat "$FAKECRON_STORE")" "30 2 * * *"

test_case "uninstall removes the tagged line, keeps the unrelated entry"
bash "$OSCRON" --uninstall uplift-cached-reviews >/dev/null
assert_eq "$(grep -c 'i2p-scheduler:uplift-cached-reviews' "$FAKECRON_STORE")" "0"
assert_eq "$(grep -c 'unrelated' "$FAKECRON_STORE")" "1"
unset I2P_CRONTAB FAKECRON_STORE

finish
