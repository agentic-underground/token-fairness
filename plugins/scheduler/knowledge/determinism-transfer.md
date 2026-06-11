# Determinism transfer — the clamp registry (Phase 3, minimum spec)

This is the standing capability behind `determinative` routing: the disciplined act of taking an
operation a model does *repeatably and correctly*, proving a tested handler reproduces it, and then
running the handler instead of the model — for 0 tokens. Kept deliberately small and concrete here;
anything beyond this spec is **future direction**, not shipped behaviour.

## The registry — `clamped-processes.json`

Each entry records one operation that was promoted from a model to tested code:

```json
{
  "id": "lint-format-check",
  "input_contract": "a file path + ruleset id → {pass:bool, findings:[…]}",
  "handler_path": "tf <verb> | ./scripts/<handler>",
  "test_path": "tests/<handler>.diff.sh",
  "promoted_at": 1700000000,
  "evidence": "differential test: handler == model on a 200-case corpus, 100% match"
}
```

## Promotion gate (the only way in)

A process is promoted **only with evidence**: a differential test proving the handler reproduces the
model's output on a representative corpus. No test → no promotion. This is the same proof discipline
as the Phase 1 port (`tests/conformance.sh`): the handler's correctness is *demonstrated*, never
assumed.

## Rollback (the way out)

A clamped process that regresses is **demoted** — the handler is removed and the work returns to a
model. The registry records **both** transitions (promoted_at and, on demotion, demoted_at +
reason), so the history of every clamp is auditable.

## Metric honesty

"Per-unit spend → 0" is real **only because the work moved to verified code** — the registry's
correctness oracle (the test) is what makes the 0 trustworthy. Always show the spend-drop in the
convergence ledger **alongside** the passing oracle, never alone. A 0 with no green test is a claim,
not a result.

## Relationship to the rest of the scheduler

- The router (`tf route`, [cognition-routing.md](./cognition-routing.md)) consults this registry: a
  unit whose operation is clamped routes to `determinative` (tier `none`, the `handler_path`).
- The estimator counts clamped units as 0-token in the band and 0-$ in the cost model.
- The convergence ledger (`tf report --estimator`) is where the spend-drop is shown next to the
  oracle's pass/fail — the honest, joined view.

Until a real clamp candidate appears (an operation a model does repeatably enough to be worth
proving), this stays a spec: the schema and the gate are defined, the registry is empty by design.
