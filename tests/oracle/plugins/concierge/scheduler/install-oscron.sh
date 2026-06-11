#!/usr/bin/env bash
# install-oscron.sh — install (or remove) the OS-level crontab entry for a guarded off-peak job.
# This is what makes the job survive Claude being closed: cron fires the headless wrapper regardless
# of whether the interactive app is running (machine must be awake).
#
#   install-oscron.sh <repo-dir> <job-id> [cron-expr]     # default cron: "17 22,23,0-7 * * *"
#   install-oscron.sh --uninstall <job-id>
#
# Idempotent: each job's line is tagged with a unique marker comment; re-installing replaces it, and
# uninstall removes exactly that line and nothing else. Never touches other crontab entries.
set -uo pipefail
CRONTAB="${I2P_CRONTAB:-crontab}"   # overridable for tests (a fake that honours -l and -)
command -v "${CRONTAB%% *}" >/dev/null 2>&1 || { echo "install-oscron: no crontab on this system" >&2; exit 2; }

current_crontab() { $CRONTAB -l 2>/dev/null || true; }

if [ "${1:-}" = "--uninstall" ]; then
  job="${2:-}"; [ -n "$job" ] || { echo "usage: install-oscron.sh --uninstall <job-id>" >&2; exit 2; }
  marker="# i2p-scheduler:${job}"
  current_crontab | grep -vF "$marker" | $CRONTAB -
  echo "install-oscron: removed crontab entry for ${job}"
  exit 0
fi

repo="${1:-}"; job="${2:-}"; cron="${3:-17 22,23,0-7 * * *}"
[ -n "$repo" ] && [ -n "$job" ] || { echo "usage: install-oscron.sh <repo-dir> <job-id> [cron-expr]" >&2; exit 2; }
repo_abs="$(cd "$repo" 2>/dev/null && pwd || echo "$repo")"
wrapper="${repo_abs}/plugins/concierge/scheduler/run-offpeak-job.sh"
[ -r "$wrapper" ] || { echo "install-oscron: wrapper not found: $wrapper" >&2; exit 2; }
log="${HOME}/.claude/state/i2p-cost/offpeak-job.log"
marker="# i2p-scheduler:${job}"

line="${cron} bash ${wrapper} ${repo_abs} ${job} >> ${log} 2>&1  ${marker}"

# Replace any existing line for this job (idempotent), keep everything else verbatim.
{ current_crontab | grep -vF "$marker"; echo "$line"; } | $CRONTAB -
echo "install-oscron: installed for ${job}"
echo "  ${line}"
