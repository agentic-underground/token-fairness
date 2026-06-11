#!/usr/bin/env bash
# inject-token-safety.sh — SessionStart hook. Delivers the TOKEN SAFETY protocol into the agent's
# context as `additionalContext`, so the discipline that governs PLAN MODE travels with the plugin
# and applies in EVERY repo (not only a repo that ships a CLAUDE.md).
#
# Why this exists: the original bash scheduler delivered this protocol via idea-to-production's
# repo-level CLAUDE.md — which only governs sessions opened IN that repo. The harness has no
# ExitPlanMode hook, so plan-mode governance is necessarily knowledge-driven (a context block) with
# the PreToolUse(Agent|Task) ceiling gate as the enforced backstop. Injecting it here makes the
# discipline global. It is silent to the user (additionalContext only); it just teaches the agent.
#
# The protocol is repointed at the resolved per-arch `tf` binary (an absolute path), so the agent
# has a concrete, runnable command regardless of cwd or PATH.
set -uo pipefail

# Drain the SessionStart payload; not needed.
[ -t 0 ] || cat >/dev/null 2>&1 || true

ROOT="${CLAUDE_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
arch="$(uname -m 2>/dev/null || echo unknown)"
os="$(uname -s 2>/dev/null | tr 'A-Z' 'a-z' || echo unknown)"
case "$os" in linux*) os=linux ;; darwin*) os=darwin ;; esac
TF="${ROOT}/bin/tf-${arch}-${os}"
# Resolver: prefer the shipped per-arch binary; else a `tf` on PATH; else the bash shim path.
if [ ! -x "$TF" ]; then
  if command -v tf >/dev/null 2>&1; then TF="tf"; else TF="bash ${ROOT}/hooks/tf-hook.sh"; fi
fi

read -r -d '' CTX <<EOF || true
## TOKEN SAFETY — automatic scheduling (applies to EVERY plan, all work, all jobs)

This machine ships the token-fairness scheduler (the \`tf\` binary at: ${TF}). Its whole purpose is to
protect the usage meter from a paid lockout. It is automatic — never a slash command. Follow this
protocol; it is not optional and not only for fan-outs.

Whenever you produce a plan (plan mode or otherwise), before presenting it:
1. Classify the plan's size — small · medium · large · epic (your judgement).
2. Stamp it: run \`${TF} plan --class <size> --now \$(date +%s)\` (or \`--profile <p> --width <n>\` for a
   multi-agent fan-out). Paste its two-line banner into the plan — approximate cost + a p95
   confidence/convergence figure + a RUN-NOW vs DEFER-off-peak decision. The estimator converges:
   every job sharpens the next estimate.
3. Bracket it so the actual feeds convergence: \`${TF} plan-open <size> <est>\` at kickoff and
   \`${TF} plan-close\` at completion (the session-token delta is the actual). Every plan passes
   through — that is the only way the estimate:actual sampling improves.

For any multi-agent fan-out, additionally: carry an explicit +Xk budget directive (consent), keep
waves throttled (≤ the profile's max_parallel), and gate EVERY wave through \`${TF} gate\` — HALT and
checkpoint to the job ledger (\`${TF} ledger pause\`) before the live ceiling; resume from
\`${TF} ledger remaining\` only. A PreToolUse(Agent|Task) hook auto-denies spawns at the live ceiling
as a backstop.

Two signals, never confused: rate_limits (live used_percentage + resets_at) is the CEILING guard;
session.json cumulative tokens is the ACTUAL-spend measure for convergence. The monthly USD cap is not
machine-readable — guard it via the +Xk budget + consent, never pretend to sense it.
EOF

if command -v jq >/dev/null 2>&1; then
  jq -cn --arg c "$CTX" '{hookSpecificOutput:{hookEventName:"SessionStart", additionalContext:$c}}'
else
  # Pure-bash fallback: emit a minimal valid JSON object with the essentials.
  printf '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"TOKEN SAFETY: before presenting any plan, stamp it with `%s plan --class <size> --now $(date +%%s)` and bracket it with `%s plan-open <size> <est>` / `%s plan-close`. Gate every fan-out wave through `%s gate`; a PreToolUse(Agent|Task) hook denies spawns at the live ceiling."}}\n' "$TF" "$TF" "$TF" "$TF"
fi
exit 0
