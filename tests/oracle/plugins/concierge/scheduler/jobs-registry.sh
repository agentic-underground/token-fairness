#!/usr/bin/env bash
# jobs-registry.sh — the DURABLE scheduled-job registry. Survives crashes & restarts.
#
# CronCreate is session-only — it dies when Claude exits. So the job DEFINITION is persisted here on
# disk, in two scopes: the project's `.i2p/scheduled-jobs.json` (authoritative for this repo) and a
# per-machine index at ~/.claude/state/i2p-cost/scheduled-jobs.json (so a startup in any project can
# see what's scheduled on this machine). On the next session a SessionStart report reads this and the
# job's ledger, tells the user what's pending, and prompts the agent to RE-ARM the cron (CronCreate).
# Nothing is ever silently lost to a crash.
#
#   jobs-registry.sh register <dir> <id> <cron> <budget> <ledger-rel> <prompt-rel> <note>
#   jobs-registry.sh list   <dir>            # project registry jobs (JSON array)
#   jobs-registry.sh get    <dir> <id>       # one job (JSON)
#   jobs-registry.sh remove <dir> <id>       # drop from project + machine index
#
# Atomic jq writes; needs jq. Stamp times pass in via the caller (none stored here beyond created day).
set -uo pipefail
command -v jq >/dev/null 2>&1 || { echo "jobs-registry: jq required" >&2; exit 2; }

cmd="${1:-list}"; dir="${2:-.}"
PROJ="${dir%/}/.i2p/scheduled-jobs.json"
MACHINE="${I2P_MACHINE_REGISTRY:-${HOME}/.claude/state/i2p-cost/scheduled-jobs.json}"
repo_abs="$(cd "${dir}" 2>/dev/null && pwd || echo "${dir}")"

ensure() { local f="$1"; mkdir -p "$(dirname "$f")" 2>/dev/null; [ -r "$f" ] || printf '{"jobs":[]}\n' > "$f"; }
write_atomic() { local f="$1" prog="$2"; shift 2; jq "$@" "$prog" "$f" > "${f}.tmp.$$" && mv -f "${f}.tmp.$$" "$f"; }

case "$cmd" in
  register)
    id="${3:-}"; cron="${4:-}"; budget="${5:-0}"; ledger="${6:-}"; prompt="${7:-}"; note="${8:-}"
    [ -n "$id" ] || { echo "jobs-registry: <id> required" >&2; exit 2; }
    case "$budget" in (''|*[!0-9]*) budget=0 ;; esac
    ensure "$PROJ"; ensure "$MACHINE"
    # Project scope: upsert by id.
    write_atomic "$PROJ" '
      .jobs = ((.jobs // []) | map(select(.id != $id)) + [{
        id:$id, cron:$cron, recurring:true, budget_total:$budget,
        ledger:$ledger, prompt_file:$prompt, note:$note, armed:false }])' \
      --arg id "$id" --arg cron "$cron" --argjson budget "$budget" \
      --arg ledger "$ledger" --arg prompt "$prompt" --arg note "$note"
    # Machine index: upsert by repo+id.
    write_atomic "$MACHINE" '
      .jobs = ((.jobs // []) | map(select(.repo != $repo or .id != $id)) + [{
        repo:$repo, id:$id, cron:$cron, budget_total:$budget, note:$note }])' \
      --arg repo "$repo_abs" --arg id "$id" --arg cron "$cron" --argjson budget "$budget" --arg note "$note"
    echo "jobs-registry: registered ${id} (project + machine index)"
    ;;

  list)
    ensure "$PROJ"; jq -c '.jobs // []' "$PROJ"
    ;;

  get)
    id="${3:-}"; ensure "$PROJ"
    jq -c --arg id "$id" '(.jobs // []) | map(select(.id == $id)) | .[0] // {}' "$PROJ"
    ;;

  arm)  # mark a job armed; method = oscron (durable, survives restart) | session (CronCreate, ephemeral)
    id="${3:-}"; method="${4:-session}"; ensure "$PROJ"
    write_atomic "$PROJ" '.jobs = ((.jobs // []) | map(if .id == $id then (.armed = true | .armed_via = $m) else . end))' \
      --arg id "$id" --arg m "$method"
    echo "jobs-registry: armed ${id} (via ${method})"
    ;;

  reset-armed)  # SessionStart calls this. CronCreate (session) crons die with the session; OS-cron does
                # NOT — so keep armed_via:oscron armed, clear only ephemeral session arming.
    ensure "$PROJ"
    write_atomic "$PROJ" '.jobs = ((.jobs // []) | map(if (.armed_via // "session") == "oscron" then . else (.armed = false) end))'
    ;;

  remove)
    id="${3:-}"; ensure "$PROJ"; ensure "$MACHINE"
    write_atomic "$PROJ"    '.jobs = ((.jobs // []) | map(select(.id != $id)))' --arg id "$id"
    write_atomic "$MACHINE" '.jobs = ((.jobs // []) | map(select(.repo != $repo or .id != $id)))' --arg repo "$repo_abs" --arg id "$id"
    echo "jobs-registry: removed ${id}"
    ;;

  *) echo "usage: jobs-registry.sh {register|list|get|arm|remove} <dir> [id] …" >&2; exit 2 ;;
esac
