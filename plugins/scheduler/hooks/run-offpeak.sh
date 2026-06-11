#!/usr/bin/env bash
# run-offpeak.sh — the OS-cron wrapper a crontab line invokes (installed by `tf oscron install`).
# Kept as a bash shim (not the binary directly) so the cron line is `bash <wrapper> …` exactly
# like the bash oracle, and so the minimal cron env can resolve absolute paths here. It delegates
# the real guard logic (flock single-instance → off-peak gate → headless `claude -p`) to
# `tf run-offpeak`, which the shim resolves per-arch.
#
#   bash run-offpeak.sh <repo-dir> <job-id>
set -uo pipefail
exec bash "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/tf-hook.sh" run-offpeak "$@"
