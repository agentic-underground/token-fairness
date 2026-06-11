# Token-aware scheduling — the operating model

The canonical reference for CONCIERGE's token-aware job scheduler: a fair, defensive guardian for the
solo builder whose usage meter is real money. It exists because a wide fan-out (review every reviewer
+ every value-handler) exhausted the limit and locked the user out **three times**, finishing nothing
and costing money each time — while an explicit "10M headroom" guard never fired. This document is the
operating model and the post-mortem reconciliation. The skill that enacts it is
[`skills/token-scheduler/SKILL.md`](../skills/token-scheduler/SKILL.md); the tested code is in
[`../scheduler/`](../scheduler/).

## The root bug, and why the old guard never fired

The previous guard watched `~/.claude/state/i2p-cost/session.json` — a **stale, wrong-metric proxy**: a
session token *count* written only at the Stop hook, which has nothing to do with the rolling
rate-limit window that actually locks you out. So it never fired against the real ceiling.

The harness **already delivers the real signal** to every turn — the same fields the status line
renders (`statusline/i2p-statusline.sh`):

```
.rate_limits.five_hour.used_percentage    .rate_limits.five_hour.resets_at
.rate_limits.seven_day.used_percentage    .rate_limits.seven_day.resets_at
```

The guard must read **that**, live. "It's a LIVE MONITOR — of course it's not on disk" was exactly
right. We mirror the latest live reading to a freshness-stamped snapshot **only** so non-hook code can
read it (see *The live→disk bridge*), and we stamp it so staleness is visible and **fails closed**. We
never guard on a stale or wrong-metric proxy again.

## Five layers — the meter is protected at every one

| Layer | Script | Guarantee |
|---|---|---|
| **L0 Pre-flight estimate** | `tf estimate`, `tf calibrate` | Predict cost before fanning out; if confidence is LOW, probe one unit and re-estimate. |
| **L1 Live ceiling guard** | `tf ceiling-check` (via `tf gate`) | Halt the next wave when a live window reaches `100 − headroom`. Pure, deterministic, **fails closed**. |
| **L2 Hard budget cap** | Workflow `budget` API (`+Xk`) | A wide fan-out **refuses to run** without an explicit `+Xk` directive; the budget is a physical ceiling. |
| **L3 Off-peak scheduler** | `tf offpeak-window`, `tf offpeak-budget` | Run unattended in the quiet hours; reserve a morning allowance from the login-time answer. |
| **L4 Cheap resume** | `tf ledger` | Resume does only what's LEFT — never re-derive context or re-fan the whole job. |

**Safety invariant:** no wave is spawned unless L1 returns CONTINUE immediately before it, and no
autonomous Workflow runs without L2's cap set. Every unknown resolves to **HALT** or **ASK**, never
silently CONTINUE.

## The dispatcher's verdicts (`tf` (the binary))

`tf` (the binary) owns **no arithmetic** — it sequences the tested helpers and names one verdict.
Determinism lives in the scripts; judgement (probe? defer? ask?) is the model's.

- **CONTINUE** — clear; spawn the next throttled wave (≤ `max_parallel`, never 130).
- **PROBE** — estimate confidence LOW; run ONE unit, measure, re-estimate with `--measured-unit-tokens`.
- **DEFER** — not in the off-peak window; hold the job for the quiet hours.
- **HALT** — a live window reached the ceiling; checkpoint the ledger and stop. Resume when it resets.
- **ASK** — no usable live signal; surface to the user. **Never proceed blind.**

## The live→disk bridge (why a snapshot is not the old proxy)

The harness feeds `.rate_limits.*` to **hooks** and the status line, but **not** to the ad-hoc `Bash`
calls an orchestrator makes mid-turn. So `tf snapshot` (the hook) (a hook) writes the latest live
reading — the actual windows, percentages + reset epochs — to `ratelimit-snapshot.json`, stamped with
`captured_at`. `tf gate` prefers a payload piped straight in (from a hook); only when none is
present does it read the snapshot, and only if it is **fresh** (`--snapshot-max-age`, default 900s) —
otherwise it returns ASK. This is the live reading pressed against glass, not a wrong-metric count.

## Off-peak: supervised-autonomous, and the morning reserve

Off-peak is **22:00–08:00** (configurable). Supervised-autonomous means: the user launches the job
once before bed with a `+Xk` budget; it runs wave-by-wave through the quiet hours, **pauses the instant
the live ceiling is hit**, and resumes from the ledger when the window resets — never exceeding the
declared budget.

At job inception the scheduler asks **"what time will you log in tomorrow?"** and `tf offpeak-budget`
turns that one answer into a per-window spend plan: windows that fully reset before login may be spent
to `100 − headroom` (85%); the window the user **inherits at login** is held to `100 − morning_reserve`
(default 40%), so they wake to a usable allowance. The closer login falls into the user's peak day, the
larger the reserve — protecting the user's working hours over overnight throughput.

## Reconciliation with `REVIEW_TOKEN_GUARD_FAILURE.md`

That post-mortem is right about the symptoms and kept rules, but its central claim — "the limit is
invisible to scripts" — is **false** (the live signal above), and that false premise drove its
over-correction ("crons must NEVER autonomously resume; prefer serial in-session"), which throws out
exactly what the user wants. The reconciliation:

- **Kept, verbatim:** rate-limit error ⇒ **HALT, do not retry**; a wide fan-out **requires an explicit
  `+Xk` budget** or refuses; **pre-flight beats post-hoc**; **throttle** the fan-out (4, not 130).
- **Corrected:** autonomous resume was unsafe *because the guard was broken*. With the live ceiling
  checked per-wave (L1), a hard budget cap (L2), and cheap incremental resume (L4), **supervised
  off-peak continuation is safe** — the cost of a wrongful resume is bounded to one small wave, not a
  monthly limit.

### Guarded scheduled jobs (the sanctioned autonomous pattern)

The post-mortem's "a cron must NEVER resume" was conditional on a broken guard. The sharpened rule: a
**guarded scheduled job MAY run autonomously off-peak** when **all four** hold — (1) the user
**consented** to it (launched/approved it, with a stated off-peak window), (2) it carries a hard `+Xk`
**budget cap**, (3) it routes **every wave through `tf gate`** (live ceiling, fail-closed),
and (4) it resumes only from the **ledger's `remaining`** (cheap, bounded). What stays forbidden is a
**blind** cron — one with no live guard and no budget, exactly the original failure. A scheduled job
that edits files must work on a **branch**, never `main`, so nothing autonomous lands unreviewed. A
cron that does *not* meet all four conditions may only **alert** (read `checkpoints[]`), never resume.

## Honest limits

- **Monthly USD cap is genuinely not machine-readable** — no disk file, no stdin field. It is guarded
  only by the L2 declared budget + consent, never sensed. (The lockout that prompted this was the
  5-hour window, which **is** live-guardable.)
- **No built-in resume primitive** — L4's ledger is ours; anything not captured in `context_pointers`
  is re-derived on resume.

## Needs empirical verification (probe before relying — `tf snapshot` (the hook))

The snapshot bridge makes the guard robust regardless, but to know which mechanism is *primary*, run
`tf verify-payload` as a temporary hook and inspect the dumped payloads:

1. Does a `PreToolUse` (`Agent`/`Task`) hook receive `.rate_limits`? If yes, `tf preflight-fanout` (the hook) can
   become a hard backstop. If no, the orchestration discipline + snapshot bridge are the guard.
2. Do subagents receive the live payload? (Assume not — the **orchestrator** is the single guard point.)
3. Inside a Workflow, is `.rate_limits` readable, or only `budget.*`? (If only `budget.*`, L2 is the
   in-Workflow guard and L1 governs main-session orchestration.)
4. Does `.cost.total_cost_usd` update intra-turn?

Every path already fails closed, so nothing proceeds blind while these are unconfirmed.
