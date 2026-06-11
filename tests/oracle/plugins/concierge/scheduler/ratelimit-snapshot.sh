#!/usr/bin/env bash
# ratelimit-snapshot.sh — the LIVE→disk BRIDGE. Run as a hook on whatever event carries the signal.
#
# The harness delivers the live rolling-window reading (.rate_limits.*) to HOOKS and the status line —
# but NOT to the ad-hoc Bash calls an orchestrator makes mid-turn. So the in-session guard would be
# blind without this: each time a hook fires with the signal, we mirror the LATEST live reading to a
# freshness-stamped snapshot the dispatcher can read.
#
# This is NOT the discredited session.json proxy (a stale, wrong-metric token COUNT). It is the actual
# rate-limit WINDOWS — percentages + reset epochs — refreshed every event and stamped with captured_at,
# so any consumer can see how old it is and FAIL CLOSED when it's stale. The monitor stays live; this
# is only the pane of glass the live reading is pressed against so non-hook code can see it.
#
# No-op (exit 0) when the payload carries no rate_limits — harmless to register on many events.
set -uo pipefail

command -v jq >/dev/null 2>&1 || exit 0
payload=""; [ -t 0 ] || payload="$(cat 2>/dev/null || true)"
[ -n "$payload" ] || exit 0

# Only write when a real five-hour or seven-day percentage is present.
has_signal="$(printf '%s' "$payload" | jq -r '
  (.rate_limits.five_hour.used_percentage // .rate_limits.seven_day.used_percentage // empty)' 2>/dev/null)"
[ -n "$has_signal" ] || exit 0

state="${I2P_COST_STATE_DIR:-${HOME}/.claude/state/i2p-cost}"
mkdir -p "$state" 2>/dev/null || exit 0
snap="${state}/ratelimit-snapshot.json"
now="$(date +%s 2>/dev/null || echo 0)"

printf '%s' "$payload" | jq -c --argjson at "$now" '
  { captured_at:$at, rate_limits:(.rate_limits // {}), cost:(.cost // {}) }' \
  > "${snap}.tmp.$$" 2>/dev/null && mv -f "${snap}.tmp.$$" "$snap" 2>/dev/null

# Forward-compat self-update: we just proved THIS event delivers the live signal. If the standing
# verdict says otherwise (or is absent), flip it — so signal-probe findings never go stale if a
# future harness build starts delivering .rate_limits on hooks. Cheap, and only runs on a real write.
evt="$(printf '%s' "$payload" | jq -r '.hook_event_name // .hookEventName // "unknown"' 2>/dev/null)"
findings="${state}/signal-findings.json"
[ -r "$findings" ] || printf '{"events":{}}' > "$findings"
jq -c --arg e "$evt" --argjson at "$now" '
  .verdict = "hook-signal-available" | .guard_mode = "live-ceiling" | .concluded_at = $at
  | .events[$e] = ((.events[$e] // {fires:0,with_rate_limits:0}) | .present = true)' \
  "$findings" > "${findings}.tmp.$$" 2>/dev/null && mv -f "${findings}.tmp.$$" "$findings" 2>/dev/null
exit 0
