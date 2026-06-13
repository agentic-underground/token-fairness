# The self-improving estimator — KAIZEN, ensemble, and the taxonomy

The estimator no longer predicts token-spend with a single fixed formula. It runs an **ensemble of
algorithms concurrently**, scores their accuracy against real outcomes after **every** job, promotes
the most accurate (the **champion**) to drive the estimate, classifies jobs in a **growing
increasing-fidelity taxonomy**, and **reports its own improvement** at every cadence. This is the SOLID
continuous-improvement covenant (KAIZEN) made executable.

## The ensemble (champion + blend)

Each algorithm predicts the next `actual/estimate` ratio for a job class from the bounded ratio history
(`samples_log`). The field today: `ewma@0.2`, `ewma@0.4` (legacy default), `ewma@0.6`, `sma@5`,
`median@7`, `last`, `linreg` (a least-squares drift fit). After each closed job, every algorithm's
standing prediction is scored against the realised ratio (absolute percentage error, smoothed into an
online **MAPE**). The lowest-error algorithm is the **champion** — its prediction is what `tf estimate`
uses. The inverse-error-weighted **blend** is reported as the "wisdom of the ensemble" cross-check.

The legacy EWMA-0.4 stays a member and is the default champion until the field has enough samples to
discriminate, so first-samples behaviour and old `calibration.json` are unchanged.

## The growing taxonomy (increasing fidelity)

Job classes are hierarchical, `/`-delimited keys (e.g. `experiment/code-gen/opus`,
`reviewer-fanout/foundry:reviewer/sonnet`). A deep node with few samples **shrinks toward its parent's**
prediction (`w = n/(n+3)`), recursively up to the global prior `1.0`. A brand-new job type inherits a
sane estimate from day one; fidelity rises automatically as the node accrues its own data.

## KAIZEN reporting — every cadence

- **Startup** (SessionStart hook): the current champion(s) + MAPE, alongside the brief.
- **Outset** (`tf plan-open`): the prediction, the champion behind it, and the ensemble spread (stderr).
- **Completion** (`tf plan-close`): actual vs estimate, the champion, and the **MAPE delta** (the
  self-review after every job) (stderr).
- **On demand**: `tf report --kaizen` (ensemble scoreboard), `tf report --taxonomy` (the class graph),
  `tf estimator <key>` (one class's champion + board), `tf estimator backtest <key>` (replay history to
  rank every formula — the bounded "best formula" hunt). An append-only `estimator-accuracy.jsonl`
  records accuracy over time.

## Self-improvement (bounded, gated)

After every job the field is re-scored and the champion re-selected automatically — that is the
per-job self-review. To add a NEW formula, register it in `ensemble::algorithms()` and prove it with
`tf estimator backtest <key>` against recorded history before adoption (on a branch, with tests). No
autonomous mutation — the REVIEW-doc-safe, interactive pattern.

## What stays byte-faithful

The **safety verbs** (ceiling/gate/offpeak/ledger/registry/snapshot/signal/oscron) remain a byte-for-
byte differential port of the bash oracle. Only the estimator family (`calibrate`/`estimate`) evolved;
it is proven by self-contained frozen-vector + unit tests instead. See `doc/completeness-audit.md`.
