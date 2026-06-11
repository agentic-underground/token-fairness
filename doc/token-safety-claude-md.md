# Paste-in TOKEN SAFETY block for a repo's `CLAUDE.md`

The `scheduler` plugin already injects this protocol globally at session start (via
`hooks/inject-token-safety.sh`), so you normally do **not** need this. Use it when you want a repo to
carry the discipline **explicitly** in its own `CLAUDE.md` — e.g. a repo shared with collaborators who
may not have the plugin installed, or where you want to customize the wording.

Copy the block below into the repo's `CLAUDE.md`. It assumes `tf` is resolvable on `PATH`; if it is
not, replace `tf` with `bash <path-to>/plugins/scheduler/hooks/tf-hook.sh` (the per-arch resolver).

---

```markdown
## TOKEN SAFETY — automatic scheduling (applies to EVERY plan, all work, all jobs)

This project is guarded by the token-fairness scheduler (`tf`). Protect the usage meter from a paid
lockout. This is automatic — never a slash command — and applies to every plan, not only fan-outs.

**Whenever you produce a plan** (plan mode or otherwise), before presenting it:
1. **Classify** the plan's size — `small` · `medium` · `large` · `epic` (your judgement).
2. **Stamp it**: `tf plan --class <size> --now $(date +%s)` (or `--profile <p> --width <n>` for a
   multi-agent fan-out). Paste its two-line banner into the plan — approximate cost + a p95
   confidence/convergence figure + a RUN-NOW vs DEFER-off-peak decision.
3. **Bracket it**: `tf plan-open <size> <est>` at kickoff and `tf plan-close` at completion (the
   session-token delta is the actual). Every plan passes through — that is the only way the
   estimate:actual sampling improves.

**For any multi-agent fan-out**: carry an explicit `+Xk` budget directive (consent), keep waves
throttled (≤ the profile's `max_parallel`), and gate **every** wave through `tf gate` — HALT and
checkpoint to the job ledger (`tf ledger pause`) before the live ceiling; resume from
`tf ledger remaining` only. A `PreToolUse(Agent|Task)` hook auto-denies spawns at the live ceiling.

Two signals, never confused: **`rate_limits`** (live `used_percentage` + `resets_at`) is the CEILING
guard; **`session.json`** cumulative tokens is the ACTUAL-spend measure for convergence. The monthly
USD cap is not machine-readable — guard it via the `+Xk` budget + consent, never pretend to sense it.
```
