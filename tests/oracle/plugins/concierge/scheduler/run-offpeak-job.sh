#!/usr/bin/env bash
# run-offpeak-job.sh — the OS-cron entry point. Runs a guarded off-peak job HEADLESS, so it survives
# Claude being closed (as long as the machine is awake). Invoked by a crontab line (install-oscron.sh).
#
#   run-offpeak-job.sh <repo-dir> <job-id>
#
# Guards, in order: (1) flock single-instance — never two runs at once, and never double-applies a
# unit alongside the in-session path; (2) off-peak window check — does nothing outside 22:00–08:00;
# (3) hands the persisted job prompt to a headless `claude -p` with a SCOPED tool allowlist. The job's
# own prompt carries the hard guards (budget cap, branch isolation, per-wave throttle). Because this
# harness exposes no live rate-limit signal to hooks (see signal-findings.json), the budget cap +
# off-peak + throttle ARE the ceiling — by design.
#
# All output appends to ~/.claude/state/i2p-cost/offpeak-job.log (the startup report summarises it).
set -uo pipefail

repo="${1:-}"; job="${2:-}"
[ -n "$repo" ] && [ -n "$job" ] || { echo "usage: run-offpeak-job.sh <repo-dir> <job-id>" >&2; exit 2; }
[ -d "$repo" ] || { echo "run-offpeak-job: repo not found: $repo" >&2; exit 2; }

SCHED="${repo%/}/plugins/concierge/scheduler"
state="${HOME}/.claude/state/i2p-cost"; mkdir -p "$state" 2>/dev/null
log="${state}/offpeak-job.log"
lock="${state}/offpeak-job-${job//[^A-Za-z0-9_-]/_}.lock"
stamp() { date '+%Y-%m-%dT%H:%M:%S%z' 2>/dev/null || echo "?"; }

# (1) Single-instance: non-blocking lock. If a run is already active, bow out silently.
exec 9>"$lock" || exit 0
if command -v flock >/dev/null 2>&1; then
  flock -n 9 || { echo "$(stamp) [$job] another run holds the lock — skipping" >> "$log"; exit 0; }
fi

# (2) Off-peak gate — do nothing during peak hours.
if [ -r "${SCHED}/offpeak-window.sh" ]; then
  inoff="$(bash "${SCHED}/offpeak-window.sh" --now "$(date +%s)" 2>/dev/null | jq -r '.in_offpeak // "false"' 2>/dev/null)"
  if [ "$inoff" != "true" ]; then echo "$(stamp) [$job] peak hours — skipping" >> "$log"; exit 0; fi
fi

# (3) The persisted prompt is the source of truth for what the job does.
prompt_file="${repo%/}/.i2p/scheduled-jobs/${job}.prompt.txt"
[ -r "$prompt_file" ] || { echo "$(stamp) [$job] no prompt at $prompt_file" >> "$log"; exit 0; }

command -v claude >/dev/null 2>&1 || { echo "$(stamp) [$job] claude CLI not on PATH" >> "$log"; exit 0; }

echo "$(stamp) [$job] off-peak fire — launching headless claude" >> "$log"
cd "$repo" || exit 0
# Scoped autonomy: only the tools the job needs, no interactive prompts (-p). The off-peak gate +
# the prompt's budget/branch/throttle rules bound it.
claude -p "$(cat "$prompt_file")" \
  --permission-mode acceptEdits \
  --allowedTools Read Edit Glob Grep Bash \
  >> "$log" 2>&1
rc=$?
echo "$(stamp) [$job] headless run finished (rc=$rc)" >> "$log"
exit 0
