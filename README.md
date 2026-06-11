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

**Phase 1 — faithful 1:1 port (in progress).** The port reproduces the bash behaviour
bit-for-bit; the original bash test inputs are the conformance gate.

Ported & conformance-proven (`tf` ⇔ bash, byte-exact output + exit codes):

| Module | bash origin | `tf` verb(s) |
|---|---|---|
| calibrate (EWMA + Welford convergence) | `calibrate.sh` | `calibrate {ratio,close,confidence}` |
| ceiling guard (live rate-limit window) | `ceiling-check.sh` | `ceiling-check` |
| pre-flight estimator | `scheduler-estimate.sh` | `estimate` |
| off-peak window clock | `offpeak-window.sh` | `offpeak-window` |
| overnight budget calculator | `offpeak-budget.sh` | `offpeak-budget` |

Remaining Phase 1: the stateful modules (ledger, registry, snapshot, signal, report), the
`scheduler.sh` plan/gate orchestration, oscron, then plugin packaging + cutover.

Phases 2–3 then add cognition-class **model routing** (determinative → mechanical → discernment
→ thought-intensive) and the **determinism-transfer** registry — *automate once, clamp the cost
of deterministic processes permanently.*

## Build & test

```sh
cargo build --release          # the tf binary
cargo test                     # self-contained frozen-vector conformance (no bash needed)
BASH_DIR=… TF=… bash tests/conformance.sh   # live differential proof vs the bash original
```

### A note on floating point

`tf` runs the same EWMA arithmetic as the bash/jq original. After many accumulated folds the two
can differ in the final ULP of an internal learned coefficient — this changes **no** observable
estimate, band, or verdict (all integer / 1-decimal / 4-decimal outputs are byte-identical). The
conformance gate compares the full observable contract exactly and collapses sub-12-sig-fig FP
noise, flagging any such case `ulp`.

## Licence

MIT OR Apache-2.0
