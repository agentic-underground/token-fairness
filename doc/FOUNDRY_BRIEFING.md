# FOUNDRY_BRIEFING — Cycle: Roadmap [6] + [7]

> Author: FOUNDER_COO (founder agent)
> Date: 2026-06-14
> Audience: builder-lead (FOUNDRY LEAD ENGINEER) — this is a **handoff**, not a plan.
> Approved plan (READ THIS FIRST): `/home/user/.claude/plans/yes-roadmap-the-token-sparkling-liskov.md`
> Roadmap source: `/home/user/Code/token-fairness/doc/ROADMAP.md`

---

## 0 · Topology readout

```
FOUNDER_COO · topology readout
foundry (BUILD_SYSTEM): present — Rust workspace (cargo), value-flow conveyor (ds-step-* agents)
frontend design system:  present — dashboard surface is vanilla JS + Chart.js (assets/dashboard.html)
test contract: unit[✓] module[✓] boundary[✓] system[✓] STORY[✓]
               (unit = inline #[test]; module/boundary = crates/*/tests/*.rs;
                system = tf-cli/tests/{cli,mcp,dashboard,stateful}.rs;
                STORY = doc/*.feature + tests/conformance.sh byte-differential vs bash oracle)
coverage gate: 83% llvm-cov floor (currently ~84.8% PASS) — treat as the perf/quality gate
stations → handlers:  SPEC→roadmapper/ds-step-1+2   IMPLEMENT→handler-rust (+vanilla-js, +github-actions)
                       STORY→ds-step-story-tests (+handler-rust for CLI e2e)
                       HARDEN→reviewer (CORRECTNESS/REGRESSION/SECURITY) + sentinel security-gate
merge governance: pr-approval (.foundry/governance.md) — code rides a PR; agent never self-merges
current project station: SPEC (ready to begin) — [1]–[5] COMPLETE; [6]/[7] are PENDING
next gate: EARS spec + .feature files for [6] and [7], reviewed by EARS-REVIEWER + BDD-REVIEWER
```

The test contract is **satisfiable** — no `CONTRACT UNMET` halt. All five levels exist with a
working coverage gate. Proceed.

---

## 1 · Scope (HARD boundary)

**IN SCOPE — build these two items only:**

- **[6]** Flexible session budget controls + `/tf:help`, `/tf:report`, `/tf:reset` slash commands
  (new slash commands; `tf_budget_set`/`tf_budget_read` MCP key expansion; dashboard Budget-Controls
  card; `POST /api/budget`; **dashboard re-bind to `127.0.0.1`** when any write endpoint is active).
- **[7]** Request-shape cost journal + mini-haiku summarizer — **CRITICAL PATH** (persistent
  `cost-journal.jsonl`; `tf journal append|close|read` behind a new `journal` feature gate;
  `tf_journal_append`/`tf_journal_read` MCP tools; `tf://cost-journal` MCP resource; opt-in
  `--summarize` via curl subprocess, fails-open).

**HARD_PAUSED — do NOT plan, spec, or build:**

- **[8]** Model fleet comparison engine (depends on [7] — will be a later cycle)
- **[9A]** Knobs/dials dashboard tab
- **[9B]** MCP-first vs bash-hook comparison report

Even though the approved plan file documents [8]–[9B] for context, they are out of scope for this
cycle. Plan **[6] and [7] only**. Do not let the journal-`projections` field, fleet-savings math,
or any [8] schema leak into the [7] journal entry — ship [7] with **total cost per item only** (the
user's Q1 decision, plan lines 147 & 297). Per-phase and projections are deferred.

---

## 2 · The three value-stations this work flows through

### SPEC station — owner: roadmapper / `ds-step-1-ears` + `ds-step-2-feature-docs`
- EARS specification for [6] and [7] (unique requirement IDs, traceability hooks).
- `.feature` Gherkin files (happy / unhappy / abuse scenarios) — follow the existing
  `doc/mcp.feature` and `doc/dashboard.feature` precedent.
- SUBJECT_MATTER_UNDERSTANDING (SMU) updates for the new `journal` domain concept and the
  budget-write-surface security posture (127.0.0.1 bind).
- **Exit gate:** EARS-REVIEWER PASS + BDD-REVIEWER PASS.

### IMPLEMENT station — owners split by value-handler (see §3)
- Rust domain layer: `budget.rs` (extract `set_field`), `mcp.rs` (key expansion + new journal
  tools/resource), new `journal.rs` module in tf-core, `dashboard.rs` (budget card endpoint).
- CLI dispatch: `crates/tf-cli/src/main.rs` (`tf journal` arm, cfg-gated; `--help` text).
- CLI bind change: `crates/tf-cli/src/dashboard_run.rs` (`0.0.0.0` → `127.0.0.1` + banner).
- Skill/command files: `plugins/scheduler/commands/tf-{help,report,reset}.md` +
  `plugins/scheduler/skills/tf-{help,report,reset}/SKILL.md`.
- Frontend: `assets/dashboard.html` (Budget-Controls card, vanilla JS + fetch).
- **Exit gate:** `cargo fmt --check` clean; `clippy -D warnings` clean; all tests green
  (RED→GREEN test-first); coverage ≥ 83% floor held.

### STORY station — owner: `ds-step-story-tests` (+ handler-rust for CLI e2e)
- End-to-end CLI: `tf journal append → close → read` round-trips; `tf budget set` round-trips
  through the new MCP keys; error/edge cases (empty id, missing model, absent `$ANTHROPIC_API_KEY`
  → fails-open to 100-char truncation).
- MCP tool calls: `tf_journal_append`/`tf_journal_read`; `tf_budget_set`/`tf_budget_read`
  round-trip on `weekly_cap` + `headroom_pct`.
- MCP resource: `tf://cost-journal` returns the entries; **`resources_list` now enumerates 4
  resources** — the existing assertion that says 3 MUST be updated to 4 (plan line 146 & 159).
- Hook-binary discipline test: `cargo build --release -p tf-cli` (no features) → `tf --help`
  MUST NOT list `journal` (plan AC[7].4).
- Skill invocations: `/tf:help` renders `tf --help` verbatim (not hardcoded); `/tf:report`;
  `/tf:reset` (with the open-journal-entry warning present in its SKILL.md).
- **Exit gate:** STORY_PROVEN sentinel; coverage gate green on the full-feature run.

---

## 3 · The three value-handlers this cycle requires

| Handler | Agent | Owns |
|---|---|---|
| **Rust domain handler** | `foundry:handler-rust` | tf-core (`journal.rs`, `budget::set_field`, `mcp.rs` tools/resource), tf-cli dispatch + bind change. Routes to **Opus** per FOUNDRY model-routing memory (Rust handler + reviewers → Opus). |
| **Vanilla-JS handler** | `foundry:handler-vanilla-js` | `assets/dashboard.html` Budget-Controls card (form fields + Set button, `POST /api/budget`, update display from response body — no WebSocket). Slash-command/skill `.md` files (`tf-help`, `tf-report`, `tf-reset`) also live here (thin wrappers). |
| **Test-harness handler** | `foundry:ds-step-story-tests` (+ `handler-rust` for inline/system tests) | The e2e/story suite, the round-trip assertions, the `resources_list` 3→4 update, the no-features hook-binary `--help` test. |

> Two adjacent handlers may also be pulled in by the builder-lead: **`handler-github-actions`** only
> if the full-feature CI invocation must be updated to add `--features tf-cli/journal` (it must — see
> §4); and **reviewer** (CORRECTNESS/REGRESSION/SECURITY roles) + **sentinel security-gate** at the
> HARDEN gate before the PR. No graphic/illustration work this cycle, so no user heads-up needed.

---

## 4 · Feature-gate discipline (CRITICAL — the Opus-review finding)

This was the single CRITICAL finding folded into the approved plan. The hot-hook binary is compiled
with **no features**; `tf journal` must never link into it (it would bloat the lazy-downloaded hook
binary and break the size budget honoured in [1]/[4]).

Mirror the existing `mcp` / `dashboard` pattern exactly (verified in the live tree):

1. **`crates/tf-core/Cargo.toml`** — add `journal = []` to `[features]` (alongside `mcp`/`dashboard`).
2. **`crates/tf-cli/Cargo.toml`** — add `journal = ["tf-core/journal"]` to `[features]`.
3. **`crates/tf-core/src/lib.rs`** — `#[cfg(feature = "journal")] pub mod journal;`
   (mirrors lines 13/19 for `dashboard`/`mcp`).
4. **`crates/tf-cli/src/main.rs`** — wrap the `"journal" =>` match arm with
   `#[cfg(feature = "journal")]` (mirrors lines 174–186). The `--help` text at lines 188–200
   hardcodes the command groups; add a cfg-gated `Journal: journal` line so the no-features binary's
   `--help` stays clean (this is what AC[7].4 verifies).
5. **The `journal-summarizer` opt-in** rides a SECOND feature (`journal-summarizer`) — curl
   subprocess only, **no new crate dependency** (no `reqwest`, no HTTP client). Fails-open.

**Full-build invocation — every place the workspace is tested/linted with features must now include
`tf-cli/journal`:**
```
cargo test --workspace --features tf-cli/mcp,tf-cli/dashboard,tf-cli/journal
```
The builder-lead MUST audit `.github/workflows/verify.yml` (the `clippy`/`test`/`coverage` jobs that
currently pass `--features mcp,dashboard`) and add `journal` there — otherwise the new code ships
untested-by-CI, the exact class of defect that let the [1] stub facade through. **An inconsistency
between the CI feature list and the test contract is itself a finding to report, not silently fix.**

---

## 5 · BLOCKERS / unknowns the builder-lead must resolve before planning

1. **State path for `cost-journal.jsonl`.** Plan says `~/.claude/state/i2p-cost/cost-journal.jsonl`,
   but every other state file resolves via `observe.rs` path helpers honouring `I2P_*` env vars (so
   tests can isolate). The builder-lead must define a `journal::journal_path()` helper following the
   `observe::events_path` / `observe::mcp_invocations_path` precedent — do NOT hardcode `~`, or the
   story tests cannot isolate state and the env-lock test discipline breaks. **Resolve before SPEC.**

2. **`/tf:reset` ↔ open-journal-entry interaction.** Plan offers two options (lines 150–154): a
   SKILL.md warning (v1) OR `tf journal close` recording the session-token delta at open/close to be
   reset-safe (preferred, v1.1+). **Builder-lead must pick one for this cycle.** Recommended: ship
   the SKILL.md warning now (cheap, satisfies AC[6].3 + AC[7] note), defer the reset-safe delta to a
   follow-up — keeps the slice thin. Confirm with the user if unsure.

3. **`tf journal append` upsert semantics.** "Upserts an entry" (open entry keyed by `roadmap_id`) —
   where does the OPEN (in-progress) entry live before `close` appends to `cost-journal.jsonl`?
   Needs a staging file (e.g. `journal-open.json` keyed by roadmap_id) vs. recomputing from events.
   Define the open-entry storage model in SPEC. **Resolve before IMPLEMENT.**

4. **`--summarize` test strategy.** The curl-subprocess summarizer cannot be hit in CI (no API key,
   no network). The story test must assert the **fails-open path** (key absent → 100-char
   truncation) deterministically. Confirm the test seam (env-unset `$ANTHROPIC_API_KEY` under
   `ENV_LOCK`) before writing the test.

5. **Sequencing within the cycle.** [6] and [7] are both standalone per the dependency graph (plan
   line 276–277), but both touch `mcp.rs` and (for [6]) `dashboard.rs`/`dashboard.html`. To avoid
   self-inflicted merge conflict, **recommend two sequential PRs: [6] first (budget UX + bind +
   MCP key expansion), then [7] (journal)** — or one combined PR if the builder-lead prefers a single
   review. Builder-lead's call; note it in FOUNDRY_PLAN.md.

6. **Coverage on feature-gated code.** New `journal.rs` only compiles under `--features journal`.
   Confirm the coverage job runs with the full feature set (see §4) so the 83% floor actually
   measures the new module — otherwise coverage passes while the new code is invisible to it.

None of these are showstoppers; all are answerable from the plan + codebase in the SPEC station.

---

## 6 · Estimated implementation scope (rough)

- **New files (5–7):**
  `crates/tf-core/src/journal.rs`; `plugins/scheduler/commands/tf-help.md`,
  `tf-report.md`, `tf-reset.md`; `plugins/scheduler/skills/tf-help/SKILL.md`,
  `tf-report/SKILL.md`, `tf-reset/SKILL.md`. (Plus new `.feature` spec files.)
- **Modified files (3–5):**
  `crates/tf-core/Cargo.toml`, `crates/tf-cli/Cargo.toml`, `crates/tf-core/src/lib.rs`,
  `crates/tf-cli/src/main.rs`, `crates/tf-core/src/mcp.rs`, `crates/tf-core/src/budget.rs`,
  `crates/tf-core/src/dashboard.rs`, `crates/tf-cli/src/dashboard_run.rs`,
  `assets/dashboard.html`, `.github/workflows/verify.yml`.
- **~800–1200 lines** of Rust (journal module + tests dominate) plus the skill/command markdown
  and the dashboard-card JS.

(The "3–4 modified" estimate in the task brief is light; the realistic count is 5–10 because the
feature gate touches both Cargo.tomls + lib.rs + main.rs + CI in addition to the domain modules.
Flagging the delta so the builder-lead budgets accordingly.)

---

## 7 · Readiness summary & go/no-go

The test contract is satisfied, governance is `pr-approval`, the `mcp`/`dashboard` feature-gate
template is verified in the live tree as the exact pattern [7]'s `journal` gate must follow, the
`journal` feature is confirmed absent (greenfield), and the six open questions above are all
resolvable inside the SPEC station — **GO: builder-lead is cleared to proceed to FOUNDRY_PLAN.md
for [6] and [7] only, resolving the §5 blockers during the SPEC station before any code is cut.**
