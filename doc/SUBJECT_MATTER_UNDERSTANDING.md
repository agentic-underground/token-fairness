# Subject Matter Understanding — token-fairness — Cycle [6] + [7]

**Created:** 2026-06-14
**Status:** ACTIVE
**Scope:** Roadmap [6] (flexible budget controls + slash commands) and [7] (request-shape cost journal).
**Continuity inputs:**
- Plan (STEP 0): `doc/[6-7]_SLASH_COMMANDS_JOURNAL_PLAN.md`
- Approved design: `/home/user/.claude/plans/yes-roadmap-the-token-sparkling-liskov.md`
- Founder briefing: `doc/FOUNDRY_BRIEFING.md`
- Roadmap: `doc/ROADMAP.md` (ends at [5] COMPLETE; [6]/[7] transcribed in STEP 1)
- EARS spec: `doc/SPECIFICATION.ears.md` (this cycle appends the `TF-6-*` / `TF-7-*` block)

This is a living document. Per the KAIZEN covenant it is updated whenever a new domain concept,
actor, or constraint is introduced. The `journal` domain and the `127.0.0.1` write-surface posture
are introduced here for the first time.

---

## 1. Actors

| Actor | Description | Surfaces used |
|---|---|---|
| **Operator (human)** | The engineer running Claude Code who wants to discover, read, and adjust their token-budget posture. | `/tf:help`, `/tf:report`, `/tf:reset`; the dashboard Budget-Controls card. |
| **Claude (agent/skill runtime)** | The Claude Code session that executes the slash-command skills, which shell out to the `tf` binary. | Skills under `plugins/scheduler/skills/*`; `tf --help`, `tf report`, `tf budget set --reset`, `tf session-boundary`. |
| **MCP client** | Any MCP-protocol consumer (Claude, automated harness) calling tools/resources over stdio JSON-RPC. | `tf_budget_set`, `tf_budget_read`, `tf_journal_append`, `tf_journal_read`, `tf://cost-journal`. |
| **Dashboard browser** | The local browser viewing the dashboard SPA. | `POST /api/budget`; reads the response body. Reaches the server ONLY over loopback (`127.0.0.1`). |
| **FOUNDRY recorder** | The CLI caller (human or automation) that records what a roadmap item cost. | `tf journal append|close|read`. |
| **Hook binary** | The no-features `tf` build lazy-downloaded and invoked by `tf-hook.sh`. MUST NOT contain `journal`/`mcp`/`dashboard` code (size budget). | `tf --help` (must not list `journal`). |
| **Summarizer subprocess** | The opt-in `curl` child process that compresses an ask via the Anthropic API. Fails-open. | Spawned by `tf journal close --summarize` under `journal-summarizer`. |

Every actor above is represented by ≥ 1 EARS statement in the `TF-6-*` / `TF-7-*` block.

---

## 2. Domain terms

- **Budget key** — a caller-facing budget control name. Allow-list this cycle:
  `{ session_cap, per_fanout_cap, weekly_cap, headroom_pct }`. Maps to on-disk keys
  `session_cap_tokens`, `per_fanout_cap_tokens`, `weekly_cap_tokens`, `headroom_pct` in `budget.json`.
- **Re-baseline / reset** — re-anchoring the legacy spent-since baseline to the current cumulative
  session token total (`tf budget set --reset`, then `tf session-boundary`).
- **Cost journal** — the durable, append-only record of what each roadmap item cost.
  Final records live in `cost-journal.jsonl` (one finalised record per line).
- **Open entry** — an in-progress (not-yet-closed) journal entry, staged in `journal-open.json`,
  a JSON object keyed by `roadmap_id`. Created/accumulated by `append`, finalised+removed by `close`.
- **Upsert** — `append` creates the open entry on first call for an id, and on subsequent calls
  accumulates `tokens` into `by_model[model]` and the running total; `--ask` overwrites the stored ask.
- **`by_model`** — per-model token accumulation map inside an open entry, priced at `close` via
  `spend::default_prices`.
- **Blended rate** — per-model USD/token rate derived from `default_prices`. (Definition recorded
  here only so the [7] schema does NOT leak it; blended-rate math is [8] scope — see §5.)
- **Fails-open** — a degraded-but-functional fallback path: when summarization cannot run, the ask
  is truncated to 100 chars rather than erroring.
- **Write surface** — any HTTP endpoint that mutates state (`POST /api/budget`). Its presence is what
  forces the loopback bind.

---

## 3. The six founder blockers and how [6]/[7] resolve them

| # | Blocker | Resolution | EARS |
|---|---|---|---|
| 1 | **Journal state path** must not hardcode `~/.claude/state/i2p-cost/cost-journal.jsonl` (breaks test isolation + `testutil::ENV_LOCK`). | `journal::journal_path()` honours `I2P_COST_JOURNAL` else `{state::state_dir()}/cost-journal.jsonl`; `journal::journal_open_path()` honours `I2P_COST_JOURNAL_OPEN` else `{state_dir}/journal-open.json`. Exact precedent: `observe::events_path()` / `observe::mcp_invocations_path()`. | TF-7-001..003 |
| 2 | **`/tf:reset` ↔ open-journal interaction** — a reset re-anchors the baseline and would corrupt an open entry's token delta. | Ship the **SKILL.md warning (v1)**: `tf-reset/SKILL.md` carries this exact warning text verbatim — "Do not run while a cost-journal entry is open — the baseline re-anchor will corrupt the entry's token delta. Close the open journal entry first with \`tf journal close <id>\`." (plan lines 150–152). Reset-safe delta-at-open/close is DEFERRED (v1.1; plan lines 153–154). | TF-6-005, TF-6-006, TF-7-021 |
| 3 | **`append` upsert / open-entry storage model** — where does an in-progress entry live and how does it accumulate? | Open entries in `journal-open.json` (object keyed by `roadmap_id`); `append` upserts (`ts_opened`, `ask`, `accumulated_tokens`, `by_model`); `close` prices it, appends one finalised line to `cost-journal.jsonl` via `state::append_line`, then removes the key. | TF-7-006..016 |
| 4 | **`--summarize` test strategy** — must be deterministic, no live network. | Fails-open path is the test seam: with `$ANTHROPIC_API_KEY` unset (under `ENV_LOCK`), summarize falls back to 100-char truncation — asserted with no network. | TF-7-030..034 |
| 5 | **Sequencing within the cycle** — both [6] and [7] touch `mcp.rs`. | **Two sequential PRs**: PR-A = [6], merged green; PR-B = [7] rebased on PR-A's merge. See §6. | (process; not an EARS) |
| 6 | **Coverage on feature-gated code** — `journal.rs` is invisible to the floor unless the coverage job builds with the gate. | All CI jobs (clippy/test/llvm-cov/release in `verify.yml`; cross/native in `release.yml`) MUST pass `journal` in their feature list. The llvm-cov job currently has NO `--features` at all — a REPORTABLE FINDING, not a silent fix. | TF-7-040 |

---

## 4. Feature-gate discipline (NON-NEGOTIABLE)

The hot-hook binary is compiled with **NO features**. `tf journal` must NEVER link into it: it would
bloat the lazy-downloaded hook binary and break the size budget honoured in [1]/[4]. Therefore EVERY
appearance of `journal` is wrapped in `#[cfg(feature = "journal")]`, mirroring the verified live
`mcp` / `dashboard` pattern. There are **no exceptions**.

Required gate sites (the spec encodes each as an invariant; the implementation must satisfy all):

| File | Change | Gate |
|---|---|---|
| `crates/tf-core/Cargo.toml` | add `journal = []` and `journal-summarizer = ["journal"]` to `[features]` | declares the gate |
| `crates/tf-cli/Cargo.toml` | add `journal = ["tf-core/journal"]` and `journal-summarizer = ["tf-core/journal-summarizer","journal"]` | declares the gate |
| `crates/tf-core/src/lib.rs` | `#[cfg(feature = "journal")] pub mod journal;` | module decl gated |
| `crates/tf-core/src/journal.rs` | entire NEW module | implicitly gated via the gated `mod` decl |
| `crates/tf-cli/src/main.rs` | `#[cfg(feature = "journal")] "journal" => …` match arm | dispatch arm gated |
| `crates/tf-cli/src/main.rs:188–200` | the `Journal:` `--help` line | **genuinely** cfg-gated (see below) |
| `crates/tf-core/src/mcp.rs` | journal MCP handlers + `tf://cost-journal` resource | gated behind BOTH `mcp` AND `journal` |

**Help-text nuance (critical):** the existing `MCP:` / `Dashboard:` `--help` lines are hardcoded
*unconditionally* today (they print even in a no-features binary). Copying that pattern for the
journal line would FAIL AC[7].4. The new `Journal:` line MUST be a genuinely `#[cfg(feature="journal")]`
fragment that appends only when the feature is on. AC[7].4 is the proof: a no-features `tf --help`
must NOT list `journal`, and there must be no `journal` subcommand in that binary.

**Summarizer second gate:** the curl summarizer rides a SECOND gate, `journal-summarizer`
(which implies `journal`). It adds NO new crate dependency — `curl` subprocess only. Adding `reqwest`
or any HTTP client crate is forbidden without re-budgeting both Cargo.tomls and re-confirming the
hook binary ≤105% of baseline.

---

## 5. Three Opus-adversarial-review blockers critical to correctness

These three are load-bearing for correctness and are encoded as EARS / invariants:

1. **Dashboard bind to `127.0.0.1` (not `0.0.0.0`).** Introducing `POST /api/budget` makes the
   dashboard a *write surface*. Binding to `0.0.0.0` would expose budget-cap mutation to any
   network-adjacent host on a shared machine — a gate-ceiling-manipulation vector. The bind in
   `crates/tf-cli/src/dashboard_run.rs:378` changes `([0,0,0,0], port)` → `([127,0,0,1], port)`,
   and the startup banner (the `println!` in `dashboard_run::run`, currently at `dashboard_run.rs:100`,
   today printing `"Dashboard running on 0.0.0.0:{} (all interfaces) …"`) must state `127.0.0.1`
   and why. Defence-in-depth
   pairs with the budget-key allow-list (below). Encoded: **TF-6-013, TF-6-014, TF-6-019**.

2. **Fleet-savings blended-rate math is OUT OF SCOPE — no schema leak.** The [7] `cost-journal.jsonl`
   record carries TOTAL cost per item only (`total_tokens`, `total_cost_usd`, `by_model` breakdown).
   It must NOT carry `projections`, `opus_only_cost_usd`, per-phase `phases`, or any blended-rate
   field — those belong to [8]/[7b]. A leaked field smells like [8] and the reviewer must reject it.
   Recorded so the spec does not accidentally over-specify. Encoded as an invariant on **TF-7-013**
   and a negative requirement **TF-7-038**.

3. **Journal ↔ reset interaction (correctness, not just UX).** Because v1 does NOT make close
   reset-safe, a reset performed while an entry is open silently corrupts that entry's token
   accounting. The only correctness guarantee this cycle is the *warning* (BLOCKER #2) plus the
   `close`-clears-the-key invariant: once `close` runs, the open key is removed, so a later reset
   cannot corrupt it. Encoded: **TF-6-005/006, TF-7-016, TF-7-021**.

A fourth security control (allow-list) supports #1: `POST /api/budget` / `tf_budget_set` validate
`key` against `{session_cap, per_fanout_cap, weekly_cap, headroom_pct}` inside `budget::set_field`;
an unknown key returns an error and NEVER writes (**TF-6-010, TF-6-018**).

---

## 6. Why two PRs (PR-A [6], PR-B [7] rebased on A)

Both items modify the shared surface `crates/tf-core/src/mcp.rs` (budget-key map for [6]; journal
tools/resource for [7]). [6] additionally touches `dashboard.rs` / `dashboard_run.rs` /
`assets/dashboard.html`. Shipping them as one PR would mix two unrelated review concerns
(budget-UX/security vs journal-infrastructure) and risk a self-inflicted merge conflict on `mcp.rs`.

Decision: **PR-A = [6]**, fully green and merged first; **PR-B = [7]** rebased on PR-A's merge.
This gives the reviewer two coherent diffs and serialises the `mcp.rs` edits. The STEP 0–9 loop
runs once per PR. The agent NEVER self-merges (governance: `.foundry/governance.md`, `pr-approval`).

---

## 7. Reused infrastructure (do not reinvent)

- `state::state_dir()`, `state::append_line()`, `state::write_json()`, `state::read_json()` — path +
  IO helpers; journal paths follow the `observe::events_path()` env-override precedent.
- `spend::price_by_model(by_model: &BTreeMap<String, i64>)` — **NEW public helper** (this cycle).
  `spend::default_prices` is private and `spend::aggregate` consumes *transcript JSONL lines*, not the
  journal's model→token map, so neither is directly reusable by `close`. Rather than leak the private
  table or build a brittle line-shaped adapter, the cycle adds one public function that prices the
  open-entry `by_model` map via the same in-crate `load_prices`/`price_of` machinery `aggregate` uses,
  returning `(per_model: Vec<(model, tokens, cost_usd)>, total_tokens, total_cost_usd)`. This keeps a
  SINGLE pricing source of truth across the spend audit and the journal. Encoded: TF-7-011, TF-7-011a.
- **`tf --help` refactor (this cycle):** the help text is presently ONE `&'static str` literal passed
  to `Out::ok(...)` (main.rs `"--help"` arm, lines 188–200). A single literal cannot be `#[cfg]`-gated
  per line, so the arm is refactored to build a `String` (`String::from(base) ; #[cfg(feature="journal")]
  help.push_str("Journal: …")`). The pre-`Journal` text stays byte-for-byte identical. Encoded: TF-7-025.
- `budget.rs` set-arm (`budget.rs:294-336`) — the existing write path; this cycle extracts a pure
  `budget::set_field(key, value)` so CLI and `POST /api/budget` share ONE path.
- `mcp.rs` `dispatch_tool` / `tools_list` / `dispatch_resource` / `resources_list` — extension points
  for the new journal tools/resource.
- `observe::mcp_invocations_path()` — precedent for an ungated path shared across features.

---

## 8. Invariants that span both items

- **INV-GATE:** No `journal` symbol is reachable from a no-features build. (§4)
- **INV-ONE-WRITE-PATH:** `budget.json` is mutated through exactly one function (`budget::set_field`),
  used by both the CLI and `POST /api/budget`.
- **INV-ALLOWLIST:** Only allow-listed budget keys are ever written; unknown keys error without write.
- **INV-LOOPBACK:** Whenever a write endpoint is active, the dashboard binds to loopback only.
- **INV-CLOSE-CLEARS:** After `close <id>`, the `<id>` key is absent from `journal-open.json`.
- **INV-NO-LEAK:** `cost-journal.jsonl` records carry total-cost fields only; no [8] projection fields.
- **INV-ENV-PATH:** Every journal path is overridable via its `I2P_COST_JOURNAL*` env var.
</content>
</invoke>
