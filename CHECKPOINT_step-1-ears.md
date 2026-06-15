# CHECKPOINT — [6]+[7] cycle — PR-A [6] — STEP 1 (EARS)

> Emitted by: foundry-lifecycle-orchestrator (per-item runner)
> Reason: orchestrator session lacks an agent-spawn (Task) tool — cannot invoke the
> isolated `ds-step-*` / `reviewer` / `handler-*` sub-agents the FOUNDRY contract requires.
> This is a CLEAN, pre-STEP-1 boundary: no artifacts written, repo untouched, `master` clean.
> Resume: re-invoke the orchestrator in a session that HAS the Task/agent-spawn tool.

## loop_state

```yaml
loop_state:
  cycle: "[6]+[7] slash-commands + cost-journal"
  active_pr: PR-A
  item_slug: "6-flexible-budget-controls-slash-commands"
  iteration: 1
  current_stage: step-1-ears
  stage_status:
    step-0: complete        # plan ingested: doc/[6-7]_SLASH_COMMANDS_JOURNAL_PLAN.md
    step-1: pending         # NEXT — spawn ds-step-1-ears
    step-2: pending
    step-3: pending
    step-4: pending
    step-5: pending
    step-6: pending
    step-story: pending
    step-7: pending
    step-8: pending
    step-9: pending
  dod_status: not-satisfied
  critical_findings_open: 0
  sentinel_chain:
    - sentinel: PLAN_COMPLETE
      stage: step-0
      note: "FOUNDRY_PLAN written by builder-lead; ingested by orchestrator"
  artifacts_index:
    - path: doc/[6-7]_SLASH_COMMANDS_JOURNAL_PLAN.md
      stage: step-0
      reviewed: true
```

## Verified live-codebase facts (no re-discovery needed)

- Branch `master`, clean working tree, no open PRs.
- `doc/SPECIFICATION.ears.md` exists (EARS spec lives HERE, not `doc/EARS.md`) — STEP 1 must
  continue ID numbering from the highest existing ID in it.
- NO `doc/SUBJECT_MATTER_UNDERSTANDING.md` — STEP 1 must create it (journal domain + 127.0.0.1
  write-surface security posture).
- NO `crates/tf-core/src/journal.rs` (correct — built in STEP 5).
- Existing `.feature` files: `doc/mcp.feature`, `doc/dashboard.feature` (precedent for STEP 2).
  Cycle adds: `doc/slash-commands.feature`, `doc/budget-write.feature`, `doc/journal.feature`.
- tf-core `[features]`: `mcp = []`, `dashboard = []` → STEP 5 adds `journal = []`,
  `journal-summarizer = ["journal"]`.
- tf-cli `[features]`: `mcp`, `dashboard` → STEP 5 adds `journal`, `journal-summarizer`.
- BLOCKER #6 confirmed live: `.github/workflows/verify.yml` uses `--features mcp,dashboard`
  at clippy (L30), test (L31), release build (L67), cross/native build (L104/L106); the
  **llvm-cov job has NO `--features` at all** → journal.rs would be invisible to the 83%
  coverage floor. `release.yml` L109/L111 cross/native build. ALL must add `journal`.
  Surface as a REPORTABLE FINDING in the PR body (same defect class as the [1] stub facade).

## next_agent_instructions (cold start, STEP 1)

1. Spawn `foundry:ds-step-1-ears` (opus). Inputs: this checkpoint, the plan file, the codebase.
   Task: (a) transcribe [6] and [7] into `doc/ROADMAP.md` with `> STATUS: IN PROGRESS`
   (roadmap-only edit → direct to master, trunk-based); (b) write the [6] + [7] EARS blocks
   (IDs [6]-H1/RP1/R3/B4/B5/S6 and [7]-J1..SUM8 per plan lines 142-175) into
   `doc/SPECIFICATION.ears.md`, continuing existing ID numbering, with traceability hooks;
   (c) create `doc/SUBJECT_MATTER_UNDERSTANDING.md` (journal domain + 127.0.0.1 security posture);
   (d) encode BLOCKERS #1/#2/#3 as EARS. NO code.
2. Gate: spawn `foundry:reviewer` as EARS-REVIEWER. Require PASS (critical_open = 0). On
   NEEDS_REVISION, remediate via ds-step-1-ears and re-run. On 2nd identical rejection,
   escalate with root-cause diagnostic (do not blind-loop).
3. On EARS-REVIEWER PASS: emit EARS_COMPLETE, overwrite this file as CHECKPOINT_step-2-feature-docs.md,
   proceed to STEP 2 (`ds-step-2-feature-docs` → BDD-REVIEWER).

## unresolved_risks

- **Structural pause for PR-B:** governance is `pr-approval` (`.foundry/governance.md`) — the agent
  NEVER self-merges. PR-B [7] is specced to rebase on PR-A's MERGE. Therefore the cycle CANNOT
  autonomously finish both PRs: it halts after PR-A's PR is opened, awaiting human merge, then PR-B's
  STEP 0–9 loop resumes. Plan AC and founder BLOCKER #5 acknowledge this.
- STORY step is mandatory: PR may not reach step-7-sync without STORY_PROVEN (9 e2e scenarios:
  3 slash + 4 journal + 2 integration).
- No `IDEA_COST.jsonl` → all budget estimates HEURISTIC; emit per-item tokens after cycle (KAIZEN).
