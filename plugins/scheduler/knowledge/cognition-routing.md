# Cognition routing — predicting the best-fit model and the $-cost (Phase 2)

The token-aware scheduler answers *"will this fan-out blow my budget?"* in tokens. Cognition
routing extends that to *"which model should each unit run on, and what will it cost in dollars?"* —
because the cheapest safe answer is usually **not** "run everything on the biggest model," and some
units shouldn't run on a model at all.

This is the executable form of foundry's `policy/model-selection.md` tier table; it does not
duplicate that table — it adds the **class → tier mapping** and the **cost model** the scheduler
applies, and exposes them through `tf route`.

## The four cognition classes → tiers

| cognition_class | executes on | rationale |
|---|---|---|
| `determinative` | **no model** — a `determinative_handler` (a `tf` subcommand or client script), **0 tokens** | there is one correct output; transfer the work to tested code, pay once |
| `mechanical` | **haiku** | high-volume, low-judgement |
| `discernment` | **sonnet** (→ **opus** when a false PASS propagates: gates, security) | recoverable judgement |
| `thought-intensive` | **opus** | one error cascades |

**Routing rule:** `best_fit = cheapest tier whose ceiling ≥ the unit's cognition floor`. Never
downgrade below the floor to save tokens (the foundry rule). `discernment` escalates to opus when a
wrong PASS would propagate undetected — gates, security reviews — flagged with `--escalate`.

Determinative units **leave the token economy entirely**: their output is produced by a tested
handler (see [Phase 3 — determinism transfer](./determinism-transfer.md)), for 0 model tokens.

## The cost model

```
cost($) = ( in_tok · price(tier).in + out_tok · price(tier).out ) / 1e6
```

The token band is the estimator's `est_total` (and its `interval` lo/hi), **already scaled by
`ratio_ewma(profile)`** — so $ inherits the same calibration that makes the token estimate honest.
`in_tok`/`out_tok` split `est_total` by `--in-frac` (default 0.7). Pricing canon is
[`statusline/model-prices.tsv`](../statusline/model-prices.tsv)
(`prefix<TAB>in<TAB>out<TAB>cache_write<TAB>cache_read`, USD per 1M tokens); a built-in default keeps
`tf route` working with no file, overridable by `--prices <tsv>` or `$I2P_MODEL_PRICES`.

## `tf route`

```bash
tf route --cognition discernment --class large            # → sonnet, with a $-band
tf route --cognition discernment --escalate --class large # gate/security → opus
tf route --cognition mechanical  --name reviewer-fanout --width 26 --measured-unit-tokens 18000
tf route --cognition determinative --class large          # → 0 tokens, 0 $
```

It accepts every `tf estimate` flag (so the token band is identical to a pre-flight estimate) plus
`--cognition`, `--escalate`, `--in-frac`, `--prices`. The output is one JSON line:

```json
{"name":"plan:large","cognition_class":"discernment","best_fit_tier":"sonnet",
 "model":"claude-sonnet-4","est_total":250000,"interval":[100000,400000],
 "cost_usd":1.65,"cost_band":[0.66,2.64],
 "per_tier_usd":{"haiku":0.55,"sonnet":1.65,"opus":2.75},"in_frac":0.7}
```

`per_tier_usd` lets a banner show the trade-off directly — *"≈ $1.65 sonnet vs $0.55 haiku."*

## Worked example — a 26-unit reviewer fan-out

26 units; 8 are `determinative` (lint/format checks), 18 are `discernment`:

- the 8 determinative units run as `tf` handlers → **0 tokens, $0** (they left the economy);
- the 18 discernment units route to sonnet;
- the banner reads: *"≈ \$X sonnet vs \$Y haiku; 8/26 determinative → free."*

That is the whole point: the scheduler doesn't just throttle spend, it **moves work to the cheapest
tier that is still correct**, and moves the determinative slice to 0.

## Profile schema additions

A profile (or per-unit/stage entry) may now declare:

- `cognition_class` — one of the four classes above. The router fills `model` from it (the model is
  no longer hardcoded in the profile).
- `determinative_handler` (optional) — the path the router substitutes for a model when the class is
  `determinative`. Its stdout **is** the unit's output. It MUST carry a differential test (same proof
  discipline as Phase 1) — "0 tokens" must never buy a wrong answer. See
  [determinism-transfer.md](./determinism-transfer.md).
