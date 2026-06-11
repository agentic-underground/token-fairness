# Cutover — retiring the idea-to-production bash scheduler, safely

token-fairness is the standalone successor to CONCIERGE's bash token-scheduler. Every observable
contract of the bash (`~/Code/idea-to-production/plugins/concierge/scheduler/`) is reproduced by the
`tf` binary and proven two ways:

- **`cargo test`** — self-contained frozen vectors for every verb (the CI gate, no bash needed).
- **`tests/conformance.sh`** — a byte-for-byte differential against the bash oracle, pinned to SHA
  `0b46ff35cb746ad14ac165431f93dcb613b517a8`, covering stdout, exit codes, and every written state
  file (ledger, registry, snapshot, signal-findings, plan-open, calibration, crontab).

Because Claude Code has **no cross-marketplace dependency resolution**, token-fairness is a manual,
optional install (`/plugin marketplace add` + `/plugin install scheduler@token-fairness`). So the
cutover must never leave the ALWAYS-ON token guard broken. It is sequenced (review C1):

## Release 1 — ship token-fairness; i2p KEEPS its bash

token-fairness ships (binary + plugin + marketplace). idea-to-production keeps concierge's bash as a
graceful-degradation fallback. The `CLAUDE.md §TOKEN SAFETY` protocol calls a **resolver** that
prefers `tf` and falls back to the bash when `tf` is absent:

```bash
# Resolver: prefer the tested binary; fall back to the bash original when tf isn't installed.
tf_run() {
  if command -v tf >/dev/null 2>&1; then
    tf "$@"
  else
    # map the verb to the bash script (gate/preflight/plan/plan-open/plan-close → scheduler.sh;
    # estimate → scheduler-estimate.sh; ledger → job-ledger.sh; registry → jobs-registry.sh; …)
    bash "$HOME/Code/idea-to-production/plugins/concierge/scheduler/<mapped>.sh" "$@"
  fi
}
```

Both paths work; `tf` is preferred when present. The verb→script map is the inverse of the port:

| `tf` verb | bash script |
|---|---|
| `gate` / `preflight` / `plan` / `plan-open` / `plan-close` | `scheduler.sh <verb>` |
| `estimate` | `scheduler-estimate.sh` |
| `calibrate` | `calibrate.sh` |
| `ceiling-check` | `ceiling-check.sh` |
| `offpeak-window` / `offpeak-budget` | `offpeak-window.sh` / `offpeak-budget.sh` |
| `ledger` | `job-ledger.sh` |
| `registry` | `jobs-registry.sh` |
| `snapshot` | `ratelimit-snapshot.sh` |
| `signal` / `verify-payload` | `signal-probe.sh` / `verify-payload.sh` |
| `report` | `report.sh` |
| `oscron` / `run-offpeak` | `install-oscron.sh` / `run-offpeak-job.sh` |
| `preflight-fanout` | `preflight-fanout.sh` |

### State-file coexistence during the half-migration (review C4)

Both plugins may be installed at once. They write the same
`~/.claude/state/i2p-cost/{ratelimit-snapshot,signal-findings,calibration,session}.json`. The chosen
contract is **shared-path coexistence**: the schemas are byte-identical (proven by conformance), so a
snapshot written by either side is read correctly by the other. The one asymmetry to know:
`tf signal` / `tf report` honour `I2P_SIGNAL_FINDINGS` with a HOME default and do **not** read
`I2P_COST_STATE_DIR` (faithful to `signal-probe.sh`), whereas `tf snapshot` honours
`I2P_COST_STATE_DIR` (faithful to `ratelimit-snapshot.sh`). With no overrides set (the normal case)
every path resolves to the same `~/.claude/state/i2p-cost/`, so the two writers coexist. If a clean
split is ever wanted, point token-fairness at `~/.claude/state/token-fairness/` via the env overrides
and run a one-time copy of `calibration.json` (Open Decision D3).

### The session.json writer travels WITH the scheduler (review C3 — CRITICAL)

`session.json .tokens` is the ACTUAL-spend signal that makes convergence work, and it was written by
concierge's `statusline/capture-cost.sh`. token-fairness ships its own
[`hooks/session-tokens.sh`](../plugins/scheduler/hooks/session-tokens.sh) Stop-hook writer so
convergence keeps working even if concierge is later removed. As a backstop, `tf plan-close` emits a
visible warning when it sees `baseline == current == 0` — so a missing writer is never silent.

## Release 2+ — retire the bash

Once adoption is real, delete concierge's `scheduler/` and drop the resolver's bash arm. Until then
the bash is the fallback, not dead weight. Also update the foundry
`knowledge/orchestration/tier-assignment.md` reference and the glossary to point at `tf` /
token-fairness.

## What "i2p's scheduler can be retired" means today

It means token-fairness is a **complete, proven, drop-in** replacement: every verb, exit code, state
file, and crontab line is reproduced and gated. The retirement itself is i2p's Release-2 step (delete
the bash, drop the resolver arm); this repo provides everything that step depends on.
