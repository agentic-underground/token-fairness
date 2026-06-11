---
description: Run a large, token-intensive fan-out SAFELY — estimate cost, require an explicit +Xk budget, gate every wave against the LIVE 5-hour window, halt before a lockout, and run off-peak (22:00–08:00) while reserving your morning allowance.
---

Route a big or wide job through the token-aware scheduler so it can never exhaust the usage limit or
cause a paid lockout. Follow the [`token-scheduler` skill](../skills/token-scheduler/SKILL.md) — it is
the discipline; the tested arithmetic lives in the `tf` binary (`${CLAUDE_PLUGIN_ROOT}/bin/`,
invoked via `${CLAUDE_PLUGIN_ROOT}/hooks/tf-hook.sh`).

What it does, in order:

1. **Pre-flight estimate** — `tf preflight`; if confidence is LOW, probe one unit, measure,
   re-estimate. You see the cost before anything fans out.
2. **Budget gate** — a wide fan-out **requires** a `+Xk` directive (e.g. "run this with +500k").
   Without it, the scheduler refuses to run autonomously.
3. **Throttled wave loop** — ≤ `max_parallel` agents per wave (not 130), and **every wave is gated
   against the live `rate_limits.five_hour` window** via `tf gate`. HALT before the ceiling.
4. **Resume ledger** — `.i2p/jobs/<job-id>.json`; a HALT checkpoints, and resume does only what's LEFT.
5. **Off-peak** — answer *"what time will you log in tomorrow?"* and it runs overnight while reserving
   a morning allowance (`tf offpeak-budget`).

Tell the scheduler what the job is (e.g. "review every reviewer and value-handler") and include a
budget directive like `+500k`. To verify what live signal your harness exposes first, run the probe
`tf verify-payload` (via `tf-hook.sh`) on a hook (see
[`knowledge/token-aware-scheduling.md`](../knowledge/token-aware-scheduling.md)).

The deterministic guard is proven in the token-fairness repo by two gates: `cargo test`
(self-contained frozen vectors) and `bash tests/conformance.sh` (a byte-for-byte differential
against the original bash scheduler, pinned to its oracle SHA).
