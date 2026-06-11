# token-fairness

A cross-cutting concern devoted **solely to token-fairness and waste-elimination**: never
exhaust a usage limit, never spend a token a deterministic process could have spent for free.

It is the standalone home of the token-aware scheduler that was born inside the
[idea-to-production](https://github.com/whatbirdisthat/idea-to-production) CONCIERGE plugin,
re-implemented as a single compiled Rust binary — `tf` — and shipped as a self-contained
Claude Code plugin.

## Why it exists

The scheduler protects a solo builder's usage meter from a paid lockout: it estimates a job's
cost before any fan-out, gates every wave against the **live** rolling rate-limit window, halts
and checkpoints before the ceiling, and can run heavy work off-peak while reserving a morning
allowance. The estimator **converges** — every job sharpens the next estimate.

It began as ~16 bash scripts. Editing correctness-critical arithmetic (EWMA + Welford online
variance, ledger state machines, ceiling math) in bash was laboured and unsafe. `tf` is the
port: compile-time types over the math, a single ~static binary with a millisecond cold start
(it runs on hot hooks — every agent spawn, every tool, every stop), and `rust-analyzer` for the
authoring the bash never had.

## Status

**Complete — the full port, packaged as a marketplace.** Every verb of the bash scheduler is
reproduced by `tf` and proven two ways: `cargo test` (self-contained frozen vectors, the CI gate)
and `tests/conformance.sh` (a byte-for-byte differential against the bash oracle, pinned to its
commit SHA, covering stdout, exit codes, **and every written state file**). 119 differential cases
+ 15 frozen-vector tests, all green.

Ported & conformance-proven (`tf` ⇔ bash, byte-exact output + exit codes + state files):

| Module | bash origin | `tf` verb(s) |
|---|---|---|
| calibrate (EWMA + Welford convergence) | `calibrate.sh` | `calibrate {ratio,close,confidence}` |
| ceiling guard (live rate-limit window) | `ceiling-check.sh` | `ceiling-check` |
| pre-flight estimator | `scheduler-estimate.sh` | `estimate` |
| off-peak window clock + budget | `offpeak-window.sh` / `offpeak-budget.sh` | `offpeak-window` / `offpeak-budget` |
| cheap-resume ledger | `job-ledger.sh` | `ledger {init,mark-done,mark-failed,remaining,pause,resume,set-offpeak,set-pointer,status}` |
| durable job registry | `jobs-registry.sh` | `registry {register,list,get,arm,reset-armed,remove}` |
| live→disk snapshot bridge | `ratelimit-snapshot.sh` | `snapshot` |
| signal probe + payload recorder | `signal-probe.sh` / `verify-payload.sh` | `signal {conclude,verdict,report}` / `verify-payload` |
| convergence report | `report.sh` | `report [--scheduled\|--estimator\|--brief]` |
| dispatcher (plan/gate/preflight) | `scheduler.sh` / `preflight-fanout.sh` | `gate` `plan` `plan-open` `plan-close` `preflight` `preflight-fanout` |
| OS-cron install + headless runner | `install-oscron.sh` / `run-offpeak-job.sh` | `oscron {install,uninstall}` / `run-offpeak` |
| **cognition routing (Phase 2)** | *new* | `route --cognition <class>` → best-fit model + $-cost band |

Shipped as the **`scheduler` plugin** under [`plugins/scheduler/`](plugins/scheduler/) in a
single-plugin marketplace ([`.claude-plugin/marketplace.json`](.claude-plugin/marketplace.json)):
a per-arch `tf` binary in `bin/`, invoked from hooks via a `bash` shim
([`hooks/tf-hook.sh`](plugins/scheduler/hooks/tf-hook.sh)), the `token-scheduler` skill + `/schedule`
command, the knowledge canon, and a Stop-hook `session.json .tokens` writer so the estimator keeps
converging on its own. Retirement of the idea-to-production bash is sequenced in
[`doc/cutover.md`](doc/cutover.md) — Release 1 ships a resolver fallback; the bash is never deleted in
the introducing release.

**Phase 3 — determinism transfer** (the clamp registry behind determinative routing) is specified as
a minimum, concrete spec in
[`plugins/scheduler/knowledge/determinism-transfer.md`](plugins/scheduler/knowledge/determinism-transfer.md);
it stays a spec until a real clamp candidate appears.

## Build & test

```sh
cargo build --release          # the tf binary (→ plugins/scheduler/bin/tf-<arch>-<os>)
cargo test                     # self-contained frozen-vector conformance (no bash needed)
BASH_DIR=… TF=… bash tests/conformance.sh   # live differential proof vs the pinned bash oracle
bash scripts/verify-prereqs.sh # single-plugin marketplace soundness (manifests, bin↔build parity)
```

### A note on floating point

`tf` runs the same EWMA arithmetic as the bash/jq original. After many accumulated folds the two
can differ in the final ULP of an internal learned coefficient — this changes **no** observable
estimate, band, or verdict (all integer / 1-decimal / 4-decimal outputs are byte-identical). The
conformance gate compares the full observable contract exactly and collapses sub-12-sig-fig FP
noise, flagging any such case `ulp`.

## Licence

MIT OR Apache-2.0
