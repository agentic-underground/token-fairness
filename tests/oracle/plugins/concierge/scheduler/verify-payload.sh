#!/usr/bin/env bash
# verify-payload.sh — the PHASE-0 PROBE. The step the original guard skipped.
#
# Temporarily register this on a hook event (PreToolUse, Stop, SessionStart, …) to discover EXACTLY
# what the harness puts on that event's stdin — specifically whether `.rate_limits` is present. It
# appends one line per fire to ~/.claude/state/i2p-cost/payload-probe.jsonl: the event's top-level
# keys, whether rate_limits/cost/transcript_path are present, and the tool name when known. Read that
# log to answer the [VERIFY] questions in knowledge/token-aware-scheduling.md, then UNREGISTER it.
#
# Writes only under ~/.claude; never blocks; no-op without jq.
set -uo pipefail
command -v jq >/dev/null 2>&1 || exit 0
state="${I2P_COST_STATE_DIR:-${HOME}/.claude/state/i2p-cost}"
log="${state}/payload-probe.jsonl"

# --report : show the CONCLUDED verdict (signal-probe owns the analysis now — no manual steps).
if [ "${1:-}" = "--report" ]; then
  exec bash "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/signal-probe.sh" report
fi

payload=""; [ -t 0 ] || payload="$(cat 2>/dev/null || true)"
[ -n "$payload" ] || exit 0

mkdir -p "$state" 2>/dev/null || exit 0
now="$(date +%s 2>/dev/null || echo 0)"

printf '%s' "$payload" | jq -c --argjson at "$now" '
  {
    at: $at,
    top_level_keys: (keys),
    has_rate_limits: (has("rate_limits")),
    five_hour_pct: (.rate_limits.five_hour.used_percentage // null),
    has_cost: (has("cost")),
    cost_usd: (.cost.total_cost_usd // null),
    has_transcript: (has("transcript_path")),
    tool: (.tool_name // .tool.name // null),
    hook_event: (.hook_event_name // .hookEventName // null)
  }' >> "$log" 2>/dev/null || true
exit 0
