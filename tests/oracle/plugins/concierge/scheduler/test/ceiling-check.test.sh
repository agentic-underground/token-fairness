#!/usr/bin/env bash
# Tests for ceiling-check.sh — the L1 live guard. Every branch of the verdict logic is pinned:
# CLEAR / HALT boundaries, NO_SIGNAL fail-closed paths, per-window selection, and the
# verdict-precedence (NO_SIGNAL > HALT > CLEAR).
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
. "${HERE}/lib/harness.sh"
CC="${HERE}/../ceiling-check.sh"

# A helper to build a payload with given five-hour and seven-day percentages.
payload() { # <5h-pct> <7d-pct>
  printf '{"rate_limits":{"five_hour":{"used_percentage":%s,"resets_at":1749635640},"seven_day":{"used_percentage":%s,"resets_at":1750000000}}}' "$1" "$2"
}

# --- CLEAR/HALT boundary at default headroom 15 (ceiling = 85) ---
test_case "84% five_hour is CLEAR (below 85 ceiling)"
assert_pipe_exit 0 "$(payload 84 10)" bash "$CC" --window five_hour
assert_contains "$ASSERT_OUT" '"verdict":"CLEAR"'

test_case "85% five_hour is HALT (>= ceiling)"
assert_pipe_exit 10 "$(payload 85 10)" bash "$CC" --window five_hour
assert_contains "$ASSERT_OUT" '"verdict":"HALT"'

test_case "86% five_hour is HALT"
assert_pipe_exit 10 "$(payload 86 10)" bash "$CC" --window five_hour

test_case "fractional 84.9% is CLEAR, 85.1% is HALT"
assert_pipe_exit 0  "$(payload 84.9 0)" bash "$CC" --window five_hour
assert_pipe_exit 10 "$(payload 85.1 0)" bash "$CC" --window five_hour

# --- custom headroom ---
test_case "headroom 50 → ceiling 50: 49 CLEAR, 50 HALT"
assert_pipe_exit 0  "$(payload 49 0)" bash "$CC" --window five_hour --headroom 50
assert_pipe_exit 10 "$(payload 50 0)" bash "$CC" --window five_hour --headroom 50

test_case "headroom 0 → ceiling 100: only 100 halts"
assert_pipe_exit 0  "$(payload 99 0)"  bash "$CC" --window five_hour --headroom 0
assert_pipe_exit 10 "$(payload 100 0)" bash "$CC" --window five_hour --headroom 0

test_case "headroom 100 → ceiling 0: even 0% halts (paranoid)"
assert_pipe_exit 10 "$(payload 0 0)" bash "$CC" --window five_hour --headroom 100

# --- both-window logic ---
test_case "both: 7-day breach while 5-hour clear → HALT on seven_day"
assert_pipe_exit 10 "$(payload 10 90)" bash "$CC" --window both
assert_contains "$ASSERT_OUT" '"window":"seven_day"'

test_case "both: both clear → CLEAR"
assert_pipe_exit 0 "$(payload 10 10)" bash "$CC" --window both

test_case "both: 5-hour breach reported even if 7-day clear"
assert_pipe_exit 10 "$(payload 90 10)" bash "$CC" --window both
assert_contains "$ASSERT_OUT" '"window":"five_hour"'

test_case "window=seven_day ignores a 5-hour breach"
assert_pipe_exit 0 "$(payload 99 10)" bash "$CC" --window seven_day

# --- NO_SIGNAL: fail closed ---
test_case "missing rate_limits → NO_SIGNAL (exit 20)"
assert_pipe_exit 20 '{"cost":{"total_cost_usd":1.0}}' bash "$CC" --window five_hour
assert_contains "$ASSERT_OUT" '"verdict":"NO_SIGNAL"'

test_case "empty payload → NO_SIGNAL"
assert_pipe_exit 20 '' bash "$CC" --window five_hour

test_case "malformed JSON → NO_SIGNAL (fail closed, never CLEAR)"
assert_pipe_exit 20 '{rate_limits: oops' bash "$CC" --window five_hour

test_case "non-numeric used_percentage → NO_SIGNAL"
assert_pipe_exit 20 '{"rate_limits":{"five_hour":{"used_percentage":"high"}}}' bash "$CC" --window five_hour

test_case "NO_SIGNAL outranks a HALT in both-window mode"
# five_hour present & breaching, seven_day field absent → overall NO_SIGNAL (fail closed)
assert_pipe_exit 20 '{"rate_limits":{"five_hour":{"used_percentage":90}}}' bash "$CC" --window both
assert_contains "$ASSERT_OUT" '"verdict":"NO_SIGNAL"'

# --- malformed flag → fail closed ---
test_case "bad headroom (non-int) → NO_SIGNAL"
assert_pipe_exit 20 "$(payload 10 10)" bash "$CC" --headroom abc --window five_hour

test_case "headroom > 100 → NO_SIGNAL"
assert_pipe_exit 20 "$(payload 10 10)" bash "$CC" --headroom 150 --window five_hour

test_case "--flag=value form works"
assert_pipe_exit 10 "$(payload 90 0)" bash "$CC" --window=five_hour --headroom=15

# --- jq-absent fallback: build a bin dir with the coreutils symlinked but NOT jq ---
NOJQ_BIN="$(mktemp -d)"; trap 'rm -rf "$NOJQ_BIN"' EXIT
for t in cat tr sed awk wc grep env bash; do
  src="$(command -v "$t" 2>/dev/null)"; [ -n "$src" ] && ln -sf "$src" "$NOJQ_BIN/$t"
done
# sanity: jq must be unreachable under this PATH
if PATH="$NOJQ_BIN" command -v jq >/dev/null 2>&1; then
  test_case "jq-hiding setup"; _fail "jq still on PATH — fallback test invalid"
else
  test_case "regex fallback (no jq) still reads the percentage → HALT at 90%"
  assert_pipe_exit 10 "$(payload 90 0)" env PATH="$NOJQ_BIN" bash "$CC" --window five_hour
  assert_contains "$ASSERT_OUT" '"verdict":"HALT"'

  test_case "regex fallback CLEAR at 10%"
  assert_pipe_exit 0 "$(payload 10 0)" env PATH="$NOJQ_BIN" bash "$CC" --window five_hour
fi

finish
