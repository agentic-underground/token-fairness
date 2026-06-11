#!/usr/bin/env bash
# run.sh — run every *.test.sh in this directory. Zero dependencies beyond bash + jq + awk
# (the same tools the scripts-under-test need). No bats, no network, runs anywhere, today.
#
#   bash plugins/concierge/scheduler/test/run.sh
#
# Exit 0 only if every suite passes. This is the determinism gate the user demanded: the
# arithmetic that protects the usage meter is proven correct before it ever guards a real job.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

fail=0 suites=0
for t in "$HERE"/*.test.sh; do
  [ -e "$t" ] || continue
  suites=$((suites+1))
  printf '\n=== %s ===\n' "$(basename "$t")"
  if ! bash "$t"; then fail=$((fail+1)); fi
done

printf '\n========================================\n'
if [ "$fail" -eq 0 ]; then
  printf 'ALL %d SUITES GREEN — the meter is protected. Lock and load.\n' "$suites"
  exit 0
else
  printf '%d of %d suites FAILED.\n' "$fail" "$suites"
  exit 1
fi
