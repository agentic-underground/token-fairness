#!/usr/bin/env bash
# preflight-fanout.sh — OPTIONAL hard backstop. PreToolUse hook on Agent|Task spawns.
#
# NOT registered by default. It activates the live ceiling as a hook-level veto on agent spawns —
# the belt-and-suspenders to the skill's orchestration discipline. Whether it can SEE the live signal
# depends on [VERIFY-1]: does a PreToolUse payload carry `.rate_limits`? Run scheduler/verify-payload.sh
# first to confirm. Until then it relies on the freshness-stamped snapshot bridge (ratelimit-snapshot.sh).
#
# Behaviour (conservative — never surprises the user by hard-denying without a real signal):
#   • If a FRESH live ceiling reading is available (payload or snapshot) and it is HALT → DENY the spawn
#     with a clear reason (the meter is protected even if the orchestrator forgot to gate).
#   • Otherwise → allow (exit 0). Absence of signal here is advisory only; the orchestration discipline
#     + ASK verdict remain the primary guard. We do not block work on a guess.
#
# To activate: add to hooks.json PreToolUse with matcher "Agent|Task". Writes nothing; needs jq.
set -uo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

command -v jq >/dev/null 2>&1 || exit 0
payload=""; [ -t 0 ] || payload="$(cat 2>/dev/null || true)"
[ -n "$payload" ] || exit 0

# Reuse the dispatcher's signal acquisition (stdin → fresh snapshot) and ceiling logic.
verdict_json="$(printf '%s' "$payload" | "${HERE}/scheduler.sh" gate 2>/dev/null)"; rc=$?

if [ "$rc" = "10" ]; then
  pct="$(printf '%s' "$verdict_json" | jq -r '.ceiling.used_pct // "?"' 2>/dev/null)"
  reason="Token ceiling reached (live window at ${pct}%). Spawning more agents now risks a lockout. Pause this job (job-ledger.sh pause) and resume when the window resets — /concierge:schedule."
  jq -cn --arg r "$reason" \
    '{hookSpecificOutput:{hookEventName:"PreToolUse", permissionDecision:"deny", permissionDecisionReason:$r}}'
  exit 0
fi

# CONTINUE / ASK / DEFER / no-signal → do not block here; the orchestrator's gate is authoritative.
exit 0
