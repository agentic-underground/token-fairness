#!/usr/bin/env bash
# red-team-spend.sh — the adversarial acceptance gate for spend-safety (doc/design/spend-safety-enforcement.md).
#
# It actively TRIES to violate each invariant and fails loudly unless the system blocks/flags it.
# "Certain" = we tried to break each guarantee and could not. Run before lifting the FAN-OUT FREEZE.
#
# Covers the CORE invariants that exist today:
#   INV-1  a Workflow fan-out with no declared budget is DENIED
#   INV-2  once the session cap is reached, even a single Agent is DENIED
#   INV-3  tf spend surfaces transcript spend that session.json missed (no secret spend)
# Pending their pieces (asserted as TODO, not silently skipped):
#   INV-4  fable refused            → piece P-D
#   INV-5  deep-research model map  → piece P-E
set -uo pipefail

TF="${TF:-target/release/tf}"
[ -x "$TF" ] || TF="plugins/scheduler/bin/tf-x86_64-linux"
[ -x "$TF" ] || { echo "red-team: no tf binary (build with: cargo build --release)"; exit 2; }

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
export I2P_COST_STATE_DIR="$TMP/state"
mkdir -p "$I2P_COST_STATE_DIR"
pass=0; fail=0
ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31m✗ %s\033[0m\n' "$1"; fail=$((fail+1)); }

echo "RED-TEAM: spend-safety invariants"

# Baseline caps: per-fanout 150k, session 2M, baseline 0 (no session.json yet → spent 0).
"$TF" budget set --session-cap 2000000 --per-fanout-cap 150000 --reset >/dev/null

# ── INV-1: a Workflow with no arm must be DENIED ───────────────────────────────
out="$(printf '{"tool_name":"Workflow"}' | "$TF" preflight-spend)"
echo "$out" | grep -q '"permissionDecision":"deny"' \
  && ok "INV-1 unarmed Workflow → DENY" \
  || bad "INV-1 unarmed Workflow was NOT denied: $out"

# Arming the 605k runaway must itself be refused (exceeds per-fanout cap).
"$TF" budget arm 605000 >/dev/null 2>&1 \
  && bad "INV-1 arming 605k runaway was allowed" \
  || ok "INV-1 arming 605k runaway → refused"

# A sane armed fan-out is allowed (and only then).
"$TF" budget arm 120000 >/dev/null
out="$(printf '{"tool_name":"Workflow"}' | "$TF" preflight-spend)"
[ -z "$out" ] && ok "INV-1 armed 120k Workflow → allowed" \
             || bad "INV-1 armed Workflow wrongly denied: $out"
"$TF" budget disarm >/dev/null

# ── INV-2: once the session cap is reached, a single Agent is DENIED ────────────
# Simulate cumulative spend at/over the cap via session.json, baseline 0.
printf '{"session_id":"rt","tokens":2000000,"usd":0}' > "$I2P_COST_STATE_DIR/session.json"
"$TF" budget set --session-cap 2000000 --per-fanout-cap 150000 >/dev/null  # baseline stays 0
out="$(printf '{"tool_name":"Agent"}' | "$TF" preflight-spend)"
echo "$out" | grep -q '"permissionDecision":"deny"' \
  && ok "INV-2 Agent over session cap → DENY" \
  || bad "INV-2 Agent over cap was NOT denied: $out"
# A non-spawn tool is never gated, even over cap.
out="$(printf '{"tool_name":"Read"}' | "$TF" preflight-spend)"
[ -z "$out" ] && ok "INV-2 Read never gated" || bad "INV-2 Read wrongly gated: $out"

# ── INV-3: tf spend surfaces spend session.json missed ─────────────────────────
PROJ="$TMP/proj"; SID="sess1"
mkdir -p "$PROJ/$SID/subagents"
# main transcript: 1M opus input ($5.00). session.json will under-report this.
printf '%s\n' '{"type":"assistant","message":{"model":"claude-opus-4-8","usage":{"input_tokens":1000000,"output_tokens":0}}}' > "$PROJ/$SID.jsonl"
# subagent transcript: 1M haiku input ($1.00) — the kind of spend session.json omits.
printf '%s\n' '{"type":"assistant","message":{"model":"claude-haiku-4-5","usage":{"input_tokens":1000000,"output_tokens":0}}}' > "$PROJ/$SID/subagents/agent-x.jsonl"
# session.json claims only the main transcript's 1,000,000 tokens.
printf '{"session_id":"%s","tokens":1000000,"usd":0}' "$SID" > "$I2P_COST_STATE_DIR/session.json"
sp="$("$TF" spend --project-dir "$PROJ" --session "$SID")"
echo "$sp" | grep -q '"total_tokens":2000000' \
  && ok "INV-3 tf spend totals across main+subagent (2,000,000)" \
  || bad "INV-3 wrong total: $sp"
echo "$sp" | grep -q '"untracked_by_session_json":1000000' \
  && ok "INV-3 surfaces the 1,000,000 tokens session.json missed" \
  || bad "INV-3 untracked gap not surfaced: $sp"

# ── INV-4 / INV-5: pending their pieces — asserted as TODO, never silently passed ─
if "$TF" route --cognition thought-intensive 2>/dev/null | grep -qi 'fable'; then
  bad "INV-4 route yielded fable (and the ban piece P-D is not built)"
else
  printf '  \033[33m•\033[0m INV-4 fable-ban (P-D) not yet built — tracked, not asserted\n'
fi
printf '  \033[33m•\033[0m INV-5 deep-research model map (P-E) not yet built — tracked, not asserted\n'

echo "RED-TEAM: $pass passed, $fail failed"
[ "$fail" -eq 0 ] || exit 1
