#!/usr/bin/env bash
# signal-probe.sh — the SELF-DRIVING signal probe. No manual "remove the hooks block" ever again.
#
# WHAT THE PROBE IS FOR: to learn which hook events (if any) carry the harness's LIVE `.rate_limits`
# meter — i.e. whether the in-session/cron ceiling guard can ever SEE the usage window, or whether the
# budget cap + off-peak + throttle must BE the guard. The answer is a property of the harness build, so
# we determine it once from captured evidence and record a verdict; behaviour adapts to it automatically.
#
#   signal-probe.sh conclude   # read the capture log → write signal-findings.json (one-shot, idempotent)
#   signal-probe.sh report     # print the findings in plain language
#   signal-probe.sh verdict    # echo just the machine verdict (no-hook-signal | hook-signal-available | unknown)
#
# Reads  ${I2P_PAYLOAD_PROBE:-~/.claude/state/i2p-cost/payload-probe.jsonl}
# Writes ${I2P_SIGNAL_FINDINGS:-~/.claude/state/i2p-cost/signal-findings.json}
# Pure jq/awk; needs jq. The companion recorder in ratelimit-snapshot.sh keeps findings fresh if a
# future harness build starts delivering the signal — so this never goes stale or needs babysitting.
set -uo pipefail
command -v jq >/dev/null 2>&1 || { echo "signal-probe: jq required" >&2; exit 0; }

PROBE="${I2P_PAYLOAD_PROBE:-${HOME}/.claude/state/i2p-cost/payload-probe.jsonl}"
FINDINGS="${I2P_SIGNAL_FINDINGS:-${HOME}/.claude/state/i2p-cost/signal-findings.json}"
now="$(date +%s 2>/dev/null || echo 0)"

cmd="${1:-report}"

case "$cmd" in
  conclude)
    [ -r "$PROBE" ] || { echo "signal-probe: no capture log at $PROBE (nothing to conclude)"; exit 0; }
    # Per-event tallies + overall presence, straight from the evidence.
    events="$(jq -cs '
      group_by(.hook_event)
      | map({ key:(.[0].hook_event // "null"),
              value:{ fires:length,
                      with_rate_limits:(map(select(.has_rate_limits))|length),
                      present:((map(select(.has_rate_limits))|length) > 0) } })
      | from_entries' "$PROBE" 2>/dev/null)"
    printf '%s' "$events" | jq -e . >/dev/null 2>&1 || events='{}'
    total_with="$(jq -s '[.[] | select(.has_rate_limits)] | length' "$PROBE" 2>/dev/null)"
    case "$total_with" in (''|*[!0-9]*) total_with=0 ;; esac
    if [ "$total_with" -gt 0 ]; then verdict="hook-signal-available"; guard="live-ceiling"
    else verdict="no-hook-signal"; guard="budget-cap"; fi
    mkdir -p "$(dirname "$FINDINGS")" 2>/dev/null
    jq -n --argjson at "$now" --arg v "$verdict" --arg g "$guard" --argjson ev "$events" \
          --argjson tw "$total_with" '
      { concluded_at:$at, verdict:$v, guard_mode:$g, total_captures_with_signal:$tw, events:$ev,
        note:(if $v=="no-hook-signal"
              then "No hook event carries .rate_limits in this harness build. The live ceiling guard returns ASK; the budget cap + off-peak window + per-wave throttle are the real guards. (The interactive statusline does receive the signal, but a headless cron has no statusline.)"
              else "At least one hook event carries .rate_limits — the snapshot bridge can feed the live ceiling guard." end) }' \
      > "${FINDINGS}.tmp.$$" && mv -f "${FINDINGS}.tmp.$$" "$FINDINGS" \
      || { echo "signal-probe: failed to write findings" >&2; exit 1; }
    echo "signal-probe: concluded → ${verdict} (guard: ${guard}); written to ${FINDINGS}"
    ;;

  verdict)
    [ -r "$FINDINGS" ] && jq -r '.verdict // "unknown"' "$FINDINGS" 2>/dev/null || echo "unknown"
    ;;

  report)
    if [ ! -r "$FINDINGS" ]; then
      echo "No signal findings yet. Run: signal-probe.sh conclude"
      exit 0
    fi
    v="$(jq -r '.verdict' "$FINDINGS")"; g="$(jq -r '.guard_mode' "$FINDINGS")"
    echo "🔎 Live-signal probe — verdict: ${v}  (guard mode: ${g})"
    jq -r '.events | to_entries[] | "   \(.key): \(.value.fires) fires · rate_limits in \(.value.with_rate_limits)"' "$FINDINGS" 2>/dev/null
    echo "   $(jq -r '.note' "$FINDINGS")"
    ;;

  *) echo "usage: signal-probe.sh {conclude|report|verdict}" >&2; exit 2 ;;
esac
