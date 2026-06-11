#!/usr/bin/env bash
# harness.sh — a zero-dependency pure-bash test harness for the scheduler library.
#
# Why not bats? bats-core is not guaranteed present on a solo builder's machine, and
# requiring it would burden exactly the cash-tight user this feature protects. This harness
# needs nothing but bash + the tools the scripts-under-test already need (jq, awk). It runs
# offline, anywhere, today. (bats stays an OPTIONAL convenience in requirements.tsv for those
# who prefer it; the test files are plain bash either way.)
#
# Usage in a test file:
#   . "$(dirname "$0")/lib/harness.sh"
#   test_case "name"; ...; assert_eq "$got" "want"; assert_exit 10 some-cmd ...
#   finish   # prints summary, exits non-zero if any assertion failed
set -uo pipefail

_T_PASS=0 _T_FAIL=0 _T_CASE=""
_C_RED=$'\033[31m'; _C_GRN=$'\033[32m'; _C_DIM=$'\033[2m'; _C_RST=$'\033[0m'
[ -t 1 ] || { _C_RED=""; _C_GRN=""; _C_DIM=""; _C_RST=""; }

test_case() { _T_CASE="$1"; }

_pass() { _T_PASS=$((_T_PASS+1)); printf '  %sok%s   %s %s\n' "$_C_GRN" "$_C_RST" "$_T_CASE" "${1:-}"; }
_fail() { _T_FAIL=$((_T_FAIL+1)); printf '  %sFAIL%s %s %s\n' "$_C_RED" "$_C_RST" "$_T_CASE" "${1:-}"; }

# assert_eq <got> <want> [msg]
assert_eq() {
  if [ "$1" = "$2" ]; then _pass "${3:-}"
  else _fail "${3:-} ${_C_DIM}(got='$1' want='$2')${_C_RST}"; fi
}

# assert_contains <haystack> <needle> [msg]
assert_contains() {
  case "$1" in (*"$2"*) _pass "${3:-}" ;; (*) _fail "${3:-} ${_C_DIM}('$1' has no '$2')${_C_RST}" ;; esac
}

# assert_exit <want-code> <cmd...> — runs cmd, captures its stdout in $ASSERT_OUT, checks exit code.
assert_exit() {
  local want="$1"; shift
  ASSERT_OUT="$("$@" 2>/dev/null)"; local got=$?
  if [ "$got" = "$want" ]; then _pass "exit=$got"
  else _fail "${_C_DIM}(exit got=$got want=$want; out='$ASSERT_OUT')${_C_RST}"; fi
}

# assert_pipe_exit <want-code> <stdin-string> <cmd...> — feeds stdin to cmd, checks exit + sets ASSERT_OUT.
assert_pipe_exit() {
  local want="$1" input="$2"; shift 2
  ASSERT_OUT="$(printf '%s' "$input" | "$@" 2>/dev/null)"; local got=$?
  if [ "$got" = "$want" ]; then _pass "exit=$got"
  else _fail "${_C_DIM}(exit got=$got want=$want; out='$ASSERT_OUT')${_C_RST}"; fi
}

finish() {
  printf '%s— %d passed, %d failed —%s\n' "$_C_DIM" "$_T_PASS" "$_T_FAIL" "$_C_RST"
  [ "$_T_FAIL" -eq 0 ]
}
