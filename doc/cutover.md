# Cutover — retiring the idea-to-production bash scheduler

token-fairness is the standalone successor to CONCIERGE's bash token-scheduler. Every observable
contract of the original bash is reproduced by the `tf` binary and proven two ways:

- **`cargo test`** — self-contained frozen vectors for every verb (the CI gate, no bash needed).
- **`tests/conformance.sh`** — a byte-for-byte differential against the bash oracle, covering stdout,
  exit codes, and every written state file (ledger, registry, snapshot, signal-findings, plan-open,
  calibration, crontab). The oracle is now **vendored** at
  [`tests/oracle/plugins/concierge/scheduler/`](../tests/oracle/), a frozen snapshot captured at SHA
  `0b46ff35cb746ad14ac165431f93dcb613b517a8` (recorded in `tests/oracle/SOURCE_SHA`) — because the
  scheduler has been **removed from idea-to-production**, so there is no longer a live tree to diff
  against. The snapshot is immutable; the port is measured against it forever.

## The executed cutover — a single hard removal

The decision was a **clean break**, not a sequenced keep-the-bash-as-fallback migration:
idea-to-production **deleted** concierge's `scheduler/` tree, the `token-scheduler` skill, the
`/concierge:schedule` command, and the `token-aware-scheduling.md` knowledge doc; its `CLAUDE.md`
§TOKEN SAFETY now states plainly that token safety lives in this plugin.

**Accepted trade-off (review C2):** because Claude Code has **no cross-marketplace dependency
resolution**, token-fairness is a **manual, optional** install
(`/plugin marketplace add ~/Code/token-fairness` + `/plugin install scheduler@token-fairness`). With
the bash gone from i2p, an i2p user who has **not** installed token-fairness has **no** active token
guard. That is the deliberate consequence of "remove the scheduler from the marketplace": the guard
is now a property of *installing token-fairness*, globally, rather than of being in the i2p repo.
There is no bash fallback — the vendored oracle exists only for the conformance proof, not as a
runtime path.

### The port mapping (also the vendored-oracle layout)

Each `tf` verb reproduces one bash script; this is the conformance pairing:

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

### State-file coexistence (review C4)

If both this plugin and an *older, pre-removal* concierge are installed on the same machine during a
user's own upgrade, they write the same
`~/.claude/state/i2p-cost/{ratelimit-snapshot,signal-findings,calibration,session}.json`. The contract
is **shared-path coexistence**: the schemas are byte-identical (proven by conformance), so a file
written by either side is read correctly by the other. One asymmetry: `tf signal` / `tf report` honour
`I2P_SIGNAL_FINDINGS` with a HOME default and do **not** read `I2P_COST_STATE_DIR` (faithful to
`signal-probe.sh`), whereas `tf snapshot` honours `I2P_COST_STATE_DIR` (faithful to
`ratelimit-snapshot.sh`). With no overrides (the normal case) every path resolves to the same dir. For
a clean split, point token-fairness at `~/.claude/state/token-fairness/` via the env overrides and
run a one-time copy of `calibration.json` (Open Decision D3).

### The session.json writer travels WITH the scheduler (review C3)

`session.json .tokens` is the ACTUAL-spend signal that makes convergence work; it was written by
concierge's `statusline/capture-cost.sh`. token-fairness ships its own
[`hooks/session-tokens.sh`](../plugins/scheduler/hooks/session-tokens.sh) Stop-hook writer so
convergence keeps working independently of concierge. As a backstop, `tf plan-close` emits a visible
warning when it sees `baseline == current == 0` — a missing writer is never silent.

## Follow-up (token-fairness side)

- **Per-arch binaries (review W2):** `bin/` currently ships `tf-x86_64-linux` only. macOS/ARM users
  need their targets built before token-fairness is a guard for them; until then the missing-arch case
  is simply "no guard" (there is no bash fallback). Ship darwin-arm64 / darwin-x64 / linux-arm64.
- **Naming (Open Decision D5):** `token-fairness` / `tf` are working names — run `/ideator:name`
  before wider publishing.
