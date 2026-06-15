# Feature behaviour contracts — Cycle [6] (slash commands + budget controls)
# and Cycle [7] (request-shape cost journal).
#
# Traceability: every scenario is tagged @EARS-TF-6-NNN / @EARS-TF-7-NNN against
# doc/SPECIFICATION.ears.md (the TF-6-* / TF-7-* block, lines 368–815) and the
# acceptance-criteria matrices [6] AC1–7 and [7] AC1–6 therein.
#
# Domain language is sourced from doc/SUBJECT_MATTER_UNDERSTANDING.md §1 (actors:
# Operator, Claude skill runtime, MCP client, Dashboard browser, FOUNDRY recorder,
# Hook binary, Summarizer subprocess) and §2 (budget key, re-baseline, cost journal,
# open entry, upsert, by_model, fails-open, write surface).
#
# Test isolation:
#   - [6] scenarios that touch budget.json redirect the state directory to a
#     per-scenario temp dir via I2P_COST_STATE_DIR (see the [6] Background; matches the
#     MCP test harness in crates/tf-cli/tests/mcp.rs:213). Every "budget.json" assertion
#     refers to the file in that temp dir.
#   - [7] scenarios that touch journal state redirect the journal paths to a per-scenario
#     temp dir via the I2P_COST_JOURNAL and I2P_COST_JOURNAL_OPEN env overrides
#     (TF-7-001..003), so each starts from a clean cost-journal.jsonl / journal-open.json.
#   No scenario depends on state left behind by another (independently runnable).
#
# Build profiles: behaviour that differs across cargo feature sets lives in the third
# feature block, "[7] Cost journal — feature-gated builds". Each scenario there carries a
# @build:* tag and CANNOT share a binary with another profile. STEP-3 must compile one
# binary per profile and run only the matching @build:* subset:
#   @build:no-features  @build:journal  @build:mcp-only  @build:mcp+journal
#   @build:journal-summarizer
#
# Scenario distribution: 48 scenarios total, across three Feature blocks.
#   [6] Slash commands + budget controls:        17 scenarios (covers TF-6-001..019)
#   [7] Cost journal (single --features journal): 19 scenarios (covers TF-7-001..021, 036, 039)
#   [7] Cost journal — feature-gated builds:      12 scenarios (covers TF-7-022..035, 037)
#   Overall: 26 happy / 7 unhappy / 15 abuse-boundary.
#   One UI gesture path (TF-6-016/017) is @deferred-to-story / @ui-playwright: at STEP 3
#   it contracts only the HTTP exchange; the live-DOM gestures move to the STORY playwright
#   path (see UI INTERACTION HANDOFF).
#
# Deliberately NOT given a Gherkin scenario (non-behavioural process gates, per the
# spec's own AC matrices, lines 782–800):
#   - TF-7-040 (CI coverage/test/clippy/release feature-list must include `journal`):
#     a CI/process gate, asserted in workflow YAML, not user-observable behaviour.
#   - GATE-PR-SEQ (PR-A→PR-B sequencing): a governance gate, explicitly stated in the
#     spec (line 814) as NOT generating Gherkin scenarios in STEP 2.
# All other TF-6-* / TF-7-* statements are tagged on ≥1 scenario above.


# =============================================================================
# [6] FLEXIBLE BUDGET CONTROLS + SLASH COMMANDS
# =============================================================================

Feature: [6] Slash commands surface live tf state to the Operator

  Background:
    Given the tf binary is resolvable by the Claude skill runtime
    And the scheduler plugin's slash-command skills are registered in the session
    And I2P_COST_STATE_DIR points to a temporary directory unique to this scenario
    And every "budget.json" assertion below refers to the file in that temporary I2P_COST_STATE_DIR

  # --- /tf:help — TF-6-001, TF-6-002 -----------------------------------------

  @EARS-TF-6-001 @happy
  Scenario: tf:help renders live tf --help output plus the slash-command list
    When the Operator invokes "/tf:help"
    Then the conversation contains the verbatim output of "tf --help"
    And the conversation lists the slash commands "/tf:help", "/tf:report", "/tf:reset", and "/schedule"
    And the listed tf subcommands match the subcommands the binary actually exposes

  @EARS-TF-6-002 @abuse
  Scenario: tf:help reflects a newly added subcommand without a skill edit
    Given the tf binary is rebuilt with an additional subcommand "frobnicate"
    When the Operator invokes "/tf:help"
    Then the conversation contains "frobnicate" because the list is captured from "tf --help"
    And no static subcommand list inside the skill had to be updated

  @EARS-TF-6-002 @unhappy
  Scenario: tf:help never emits a subcommand list that diverges from the binary
    Given the skill is asked to render help while the binary lists subcommands "report" and "mcp"
    When the Operator invokes "/tf:help"
    Then the displayed subcommand list is exactly the captured "tf --help" output
    And it does not contain any subcommand absent from "tf --help"

  # --- /tf:report — TF-6-003 -------------------------------------------------

  @EARS-TF-6-003 @happy
  Scenario: tf:report renders the honesty report and links the live dashboard
    Given the tf binary produces an honesty report for the current project
    When the Operator invokes "/tf:report"
    Then the conversation contains the verbatim output of "tf report . --honesty"
    And the conversation contains a link to the live dashboard URL

  @EARS-TF-6-003 @unhappy
  Scenario: tf:report surfaces the report tool's own error rather than fabricating a report
    Given "tf report . --honesty" exits non-zero with a stderr message
    When the Operator invokes "/tf:report"
    Then the conversation shows the report tool's error message
    And the conversation does not contain a paraphrased or invented honesty report

  # --- /tf:reset — TF-6-004, TF-6-005, TF-6-006 ------------------------------

  @EARS-TF-6-004 @happy
  Scenario: tf:reset re-baselines the session and confirms the new baseline
    Given budget.json exists with cumulative session spend of 120000 tokens
    And no cost-journal entry is currently open
    When the Operator invokes "/tf:reset"
    Then "tf budget set --reset" is run followed by "tf session-boundary"
    And budget.json field "baseline_tokens" equals 120000
    And the conversation states the new baseline
    And the caps "session_cap", "weekly_cap", "per_fanout_cap", and "headroom_pct" are unchanged

  @EARS-TF-6-005 @EARS-TF-6-006 @EARS-TF-7-021 @unhappy
  Scenario: tf:reset warns the Operator while a cost-journal entry is open
    Given a cost-journal entry for roadmap id "7" is open in journal-open.json
    When the Operator invokes "/tf:reset"
    Then the conversation shows the verbatim warning "Do not run while a cost-journal entry is open — the baseline re-anchor will corrupt the entry's token delta. Close the open journal entry first with `tf journal close <id>`."
    And the corruption warning is surfaced before any re-baseline occurs

  @EARS-TF-6-004 @abuse
  Scenario: tf:reset on a fresh session with no prior baseline still confirms cleanly
    Given the session has never been re-baselined
    And no cost-journal entry is open
    When the Operator invokes "/tf:reset"
    Then the re-baseline completes without error
    And the conversation states the new baseline

  # --- budget-key expansion via MCP — TF-6-007..012 --------------------------

  @EARS-TF-6-007 @happy
  Scenario: MCP tf_budget_set accepts weekly_cap and persists it
    Given an MCP client driving "tf mcp" (built with --features mcp) as a subprocess over JSON-RPC 2.0 stdio
    When the client calls tf_budget_set with { "key": "weekly_cap", "value": 2000000 }
    Then the result is { "success": true, "key": "weekly_cap", "new_value": 2000000 }
    And budget.json field "weekly_cap_tokens" equals 2000000

  @EARS-TF-6-008 @EARS-TF-6-012 @happy
  Scenario: MCP tf_budget_read returns headroom_pct round-tripped from tf_budget_set
    Given an MCP client driving "tf mcp" (built with --features mcp) as a subprocess over JSON-RPC 2.0 stdio
    When the client calls tf_budget_set with { "key": "headroom_pct", "value": 15 }
    And the client then calls tf_budget_read
    Then the read result contains "headroom_pct" equal to 15
    And the read result still contains "session_cap", "per_fanout_cap", "current_spend", and "fanout_spend"

  @EARS-TF-6-011 @happy
  Scenario: MCP tf_budget_read returns weekly_cap alongside the existing keys
    Given an MCP client driving "tf mcp" (built with --features mcp) as a subprocess over JSON-RPC 2.0 stdio
    And budget.json has "weekly_cap_tokens" set to 2000000
    When the MCP client calls tf_budget_read
    Then the read result contains "weekly_cap" equal to 2000000
    And no previously returned key is removed from the response

  @EARS-TF-6-009 @happy
  Scenario: MCP tf_budget_set continues to accept the legacy session_cap key
    Given an MCP client driving "tf mcp" (built with --features mcp) as a subprocess over JSON-RPC 2.0 stdio
    When the client calls tf_budget_set with { "key": "session_cap", "value": 60000 }
    Then the result is { "success": true, "key": "session_cap", "new_value": 60000 }
    And budget.json field "session_cap_tokens" equals 60000

  @EARS-TF-6-010 @EARS-TF-6-014 @abuse
  Scenario: MCP tf_budget_set rejects a key outside the allow-list without writing
    Given an MCP client driving "tf mcp" (built with --features mcp) as a subprocess over JSON-RPC 2.0 stdio
    And budget.json before the call has "session_cap_tokens" equal to 50000
    When the MCP client calls tf_budget_set with { "key": "max_temperature", "value": 9000 }
    Then the call returns an error
    And the error message indicates an invalid or unknown key
    And budget.json is byte-for-byte unchanged

  # --- dashboard write surface — TF-6-013..019 -------------------------------

  @EARS-TF-6-013 @EARS-TF-6-015 @EARS-TF-6-016 @happy
  Scenario: POST /api/budget delegates to the single write path and returns new state
    Given the dashboard server is running and bound to 127.0.0.1
    And budget.json has "weekly_cap_tokens" equal to 1000000
    When the Dashboard browser POSTs /api/budget with { "key": "weekly_cap", "value": 1500000 }
    Then the response status is 200
    And budget.json field "weekly_cap_tokens" equals 1500000
    And the response body is the full new budget state with "weekly_cap" equal to 1500000

  # The live-DOM gestures (no-reload re-render, value sourced from the response body
  # rather than a re-fetch) are browser-driver assertions and are deferred to the STORY
  # playwright path — see the @ui-playwright entry in the UI INTERACTION HANDOFF below.
  # At STEP 3, this contracts only the observable HTTP exchange the card relies on.
  @EARS-TF-6-016 @EARS-TF-6-017 @ui-playwright @deferred-to-story @happy
  Scenario: The Budget-Controls card's Set action persists the new weekly cap over HTTP
    Given the dashboard server is running on 127.0.0.1:8088 with the write endpoint active
    And budget.json has "weekly_cap_tokens" equal to 1000000
    When the card submits POST /api/budget with { "key": "weekly_cap", "value": 1500000 }
    Then the response status is 200
    And the response body is the full new budget state with "weekly_cap" equal to 1500000
    And budget.json field "weekly_cap_tokens" equals 1500000
    When a subsequent GET of the budget state is made
    Then the returned "weekly_cap" is still 1500000

  @EARS-TF-6-018 @EARS-TF-6-014 @abuse
  Scenario: POST /api/budget rejects a non-allow-listed key without modifying state
    Given the dashboard server is running and bound to 127.0.0.1
    And budget.json before the request has "session_cap_tokens" equal to 50000
    When the Dashboard browser POSTs /api/budget with { "key": "admin_override", "value": 1 }
    Then the response is an error status
    And budget.json is byte-for-byte unchanged

  @EARS-TF-6-019 @abuse
  Scenario: The write-surface dashboard binds only to loopback and states why
    When the dashboard server starts with the write endpoint active
    Then the server is reachable on 127.0.0.1 only and not on any non-loopback interface
    And the startup banner contains the substring "127.0.0.1"
    And the startup banner contains the substring "network-adjacent gate-ceiling manipulation"


# =============================================================================
# [7] REQUEST-SHAPE COST JOURNAL
# =============================================================================

Feature: [7] Cost journal records what each roadmap item cost

  Background:
    Given the tf binary is built with --features journal
    And I2P_COST_JOURNAL points at an empty cost-journal.jsonl in a per-scenario temp directory
    And I2P_COST_JOURNAL_OPEN points at a non-existent journal-open.json in that directory

  # --- path resolution — TF-7-001..003 --------------------------------------

  @EARS-TF-7-001 @EARS-TF-7-002 @EARS-TF-7-003 @happy
  Scenario: Journal paths honour the I2P_COST_JOURNAL env overrides
    Given I2P_COST_JOURNAL is set to "/tmp/scn/cost-journal.jsonl"
    And I2P_COST_JOURNAL_OPEN is set to "/tmp/scn/journal-open.json"
    When the FOUNDRY recorder runs "tf journal append 7 50000 claude-haiku-4-5"
    Then the open entry is written to "/tmp/scn/journal-open.json"
    And no file is created under the default state directory

  # --- append (upsert) — TF-7-004..010 --------------------------------------

  @EARS-TF-7-004 @happy
  Scenario: tf journal append creates a new open entry for an unseen id
    Given journal-open.json has no entry keyed "7"
    When the recorder runs "tf journal append 7 50000 claude-haiku-4-5"
    Then journal-open.json has an entry keyed "7"
    And that entry has "accumulated_tokens" equal to 50000
    And that entry has "by_model" equal to { "claude-haiku-4-5": 50000 }
    And that entry has a "ts_opened" timestamp

  @EARS-TF-7-005 @happy
  Scenario: tf journal append accumulates tokens for the same model on an open entry
    Given an open entry keyed "7" has "accumulated_tokens" equal to 50000 and "by_model" { "claude-haiku-4-5": 50000 }
    When the recorder runs "tf journal append 7 20000 claude-haiku-4-5"
    Then the entry's "by_model"."claude-haiku-4-5" equals 70000
    And the entry's "accumulated_tokens" equals 70000

  @EARS-TF-7-007 @happy
  Scenario: tf journal append adds a second model to an existing open entry
    Given an open entry keyed "7" has "by_model" { "claude-haiku-4-5": 50000 } and "accumulated_tokens" 50000
    When the recorder runs "tf journal append 7 30000 claude-opus-4"
    Then the entry's "by_model" equals { "claude-haiku-4-5": 50000, "claude-opus-4": 30000 }
    And the entry's "accumulated_tokens" equals 80000

  @EARS-TF-7-006 @happy
  Scenario: tf journal append overwrites the ask but leaves it intact when omitted
    Given an open entry keyed "7" has ask "draft the spec"
    When the recorder runs "tf journal append 7 10000 claude-opus-4 --ask \"rewrite the spec\""
    Then the entry's "ask" equals "rewrite the spec"
    When the recorder runs "tf journal append 7 5000 claude-opus-4"
    Then the entry's "ask" still equals "rewrite the spec"

  @EARS-TF-7-008 @unhappy
  Scenario: tf journal append rejects an empty id and writes nothing
    When the recorder runs "tf journal append \"\" 50000 claude-opus-4"
    Then the command exits non-zero with an error
    And journal-open.json contains no entry keyed by the empty string
    And journal-open.json is unchanged

  @EARS-TF-7-009 @unhappy
  Scenario: tf journal append rejects a missing model argument and writes nothing
    When the recorder runs "tf journal append 7 50000"
    Then the command exits non-zero with an error
    And journal-open.json is unchanged

  @EARS-TF-7-010 @abuse
  Scenario: tf journal append rejects non-numeric tokens via strict parse, never coercing to a default
    Given journal-open.json has no entry keyed "7"
    When the recorder runs "tf journal append 7 abc claude-opus-4"
    Then the command exits non-zero with a parse error
    And journal-open.json has no entry keyed "7"
    And the token value is not silently defaulted to 0

  # --- close (finalise) — TF-7-011..016 -------------------------------------

  @EARS-TF-7-011 @EARS-TF-7-011a @EARS-TF-7-012 @EARS-TF-7-013 @happy
  Scenario: tf journal close prices the entry, appends a finalised record, and clears the key
    Given an open entry keyed "7" has "by_model" { "claude-haiku-4-5": 50000, "claude-opus-4": 30000 }
    When the recorder runs "tf journal close 7"
    Then exactly one record is appended to cost-journal.jsonl
    And the record has "roadmap_id" equal to "7"
    And the record "total_tokens" equals 80000
    And the record "total_cost_usd" equals the sum of the per-model costs priced through the shared price table
    And the record contains "ts", "ask_summary", "by_model", "total_tokens", and "total_cost_usd"
    And journal-open.json no longer has an entry keyed "7"

  @EARS-TF-7-013 @EARS-TF-7-038 @abuse
  Scenario: A finalised record carries total-only fields and never leaks [8] projection fields
    Given an open entry keyed "7" exists
    When the recorder runs "tf journal close 7"
    Then the appended record has no "projections" field
    And the appended record has no "opus_only_cost_usd" field
    And the appended record has no per-phase "phases" field
    And the appended record has no blended-rate field

  @EARS-TF-7-011a @abuse
  Scenario: Closing an entry with an unpriced model lists it at zero cost rather than failing
    Given an open entry keyed "7" has "by_model" { "claude-haiku-4-5": 40000, "unknown-future-model": 10000 }
    When the recorder runs "tf journal close 7"
    Then the record "total_tokens" equals 50000
    And the per-model breakdown lists "unknown-future-model" with 10000 tokens and 0.0 cost
    And the close succeeds

  @EARS-TF-7-014 @unhappy
  Scenario: tf journal close with no matching open entry errors and writes no record
    Given journal-open.json has no entry keyed "9"
    When the recorder runs "tf journal close 9"
    Then the command exits non-zero with an error
    And no record is appended to cost-journal.jsonl

  @EARS-TF-7-015 @unhappy
  Scenario: tf journal close rejects an empty id and writes no record
    When the recorder runs "tf journal close \"\""
    Then the command exits non-zero with an error
    And no record is appended to cost-journal.jsonl

  @EARS-TF-7-016 @happy
  Scenario: Finalised records persist across a session boundary and a reset
    Given cost-journal.jsonl contains one finalised record for roadmap id "7"
    When a session boundary occurs and the Operator runs "/tf:reset"
    Then cost-journal.jsonl still contains the finalised record for roadmap id "7" unchanged
    And the record was neither mutated nor removed

  # --- read — TF-7-017..020 -------------------------------------------------

  @EARS-TF-7-017 @happy
  Scenario: tf journal read outputs all finalised entries as a JSON array
    Given cost-journal.jsonl contains finalised records for roadmap ids "5", "6", and "7"
    When the recorder runs "tf journal read"
    Then stdout is a JSON array of 3 entries
    And the array is valid JSON

  @EARS-TF-7-018 @happy
  Scenario: tf journal read --id filters to a single roadmap id
    Given cost-journal.jsonl contains finalised records for roadmap ids "5", "6", and "7"
    When the recorder runs "tf journal read --id 6"
    Then stdout is a JSON array containing only the entry whose "roadmap_id" is "6"
    And the cost-journal.jsonl file is not mutated by the read

  @EARS-TF-7-019 @happy
  Scenario: tf journal read --last N returns at most the N most recent entries in append order
    Given cost-journal.jsonl contains finalised records appended in order for ids "1", "2", "3", "4", "5"
    When the recorder runs "tf journal read --last 2"
    Then stdout is a JSON array of 2 entries
    And the entries are "4" then "5" in chronological append order
    And the entries are ordered by JSONL append order, not by a timestamp sort

  @EARS-TF-7-020 @abuse
  Scenario: tf journal read on an empty or absent journal returns an empty array without error
    Given cost-journal.jsonl is absent
    When the recorder runs "tf journal read"
    Then stdout is exactly "[]"
    And the command exits zero

  # --- close summary default (network-free) — TF-7-036 ----------------------

  @EARS-TF-7-036 @happy
  Scenario: close without --summarize truncates the ask to 100 chars with no subprocess
    Given an open entry keyed "7" has an ask of 250 characters
    When the recorder runs "tf journal close 7"
    Then the record "ask_summary" is the first 100 characters of the stored ask
    And no curl subprocess is spawned

  # --- error discipline — TF-7-039 ------------------------------------------

  @EARS-TF-7-039 @abuse
  Scenario: A corrupt journal-open.json yields a typed error, never a panic
    Given journal-open.json contains bytes that are not valid JSON
    When the recorder runs "tf journal append 7 50000 claude-opus-4"
    Then the command exits non-zero with a typed parse error
    And no panic message or stack trace is emitted on stderr


# =============================================================================
# [7] COST JOURNAL — FEATURE-GATED BUILD MATRIX
# =============================================================================
#
# The scenarios above run against ONE binary (--features journal). The scenarios
# below assert behaviour that DIFFERS across cargo feature sets, so each carries a
# @build:* tag and CANNOT share a single compiled binary. The STEP-3 harness must
# compile one binary per profile and run only the matching @build:* subset against it:
#
#   @build:no-features        cargo build --release -p tf-cli            (no --features)
#   @build:journal            cargo build --release -p tf-cli --features journal
#   @build:mcp-only           cargo build --release -p tf-cli --features mcp
#   @build:mcp+journal        cargo build --release -p tf-cli --features mcp,journal
#   @build:journal-summarizer cargo build --release -p tf-cli --features journal-summarizer
#
# Journal-state isolation still applies: where a scenario touches journal files, the
# I2P_COST_JOURNAL / I2P_COST_JOURNAL_OPEN overrides point at a per-scenario temp dir
# (same discipline as the main [7] Background).

Feature: [7] Cost journal — feature-gated builds

  Background:
    Given journal-state files are redirected to a per-scenario temp directory via
      I2P_COST_JOURNAL and I2P_COST_JOURNAL_OPEN
    And the tf binary under test is the one compiled for this scenario's @build:* profile

  # --- feature gate — TF-7-022..026 -----------------------------------------

  @EARS-TF-7-026 @EARS-TF-7-025 @EARS-TF-7-023 @EARS-TF-7-024 @build:no-features @abuse
  Scenario: A no-features binary hides the journal subcommand from tf --help
    Given the tf binary is built with "cargo build --release -p tf-cli" and no features
    When the Operator runs "tf --help"
    Then the output does not contain a "Journal:" line
    And running "tf journal read" reports an unrecognised command

  @EARS-TF-7-022 @EARS-TF-7-025 @build:journal @happy
  Scenario: A journal-feature binary lists the journal subcommand in tf --help
    Given the tf binary is built with --features journal
    When the Operator runs "tf --help"
    Then the output contains a "Journal:" line listing "journal"
    And the pre-Journal help text is byte-for-byte identical to the no-features help text

  # --- MCP tools + resource — TF-7-028..032, TF-7-037 -----------------------

  @EARS-TF-7-028 @EARS-TF-7-037 @build:mcp+journal @happy
  Scenario: MCP tf_journal_append upserts the same shared open entry as the CLI
    Given an MCP client driving "tf mcp" (built with --features mcp,journal) as a subprocess over JSON-RPC 2.0 stdio
    When the client calls tf_journal_append with { "roadmap_id": "7", "tokens": 50000, "model": "claude-opus-4" }
    Then journal-open.json has an entry keyed "7" with "accumulated_tokens" equal to 50000
    When the recorder runs "tf journal read --id 7" after closing the entry via the CLI
    Then the entry recorded by the MCP append is visible to the CLI read

  @EARS-TF-7-029 @EARS-TF-7-037 @build:mcp+journal @happy
  Scenario: MCP tf_journal_read returns the same entries the CLI read would for equivalent filters
    Given an MCP client driving "tf mcp" (built with --features mcp,journal) as a subprocess over JSON-RPC 2.0 stdio
    And cost-journal.jsonl contains finalised records for roadmap ids "6" and "7"
    When the MCP client calls tf_journal_read with { "roadmap_id": "7" }
    Then the result is an array containing only the entry whose "roadmap_id" is "7"
    And the result matches "tf journal read --id 7"

  @EARS-TF-7-031 @build:mcp+journal @happy
  Scenario: tf://cost-journal resource returns the last 100 finalised entries
    Given an MCP client driving "tf mcp" (built with --features mcp,journal) as a subprocess over JSON-RPC 2.0 stdio
    And cost-journal.jsonl contains 150 finalised records appended in order for roadmap ids "1" through "150"
    When the MCP client reads the resource tf://cost-journal
    Then the response is a JSON array of length 100
    And the array contains the 100 most recent records
    And the most recent entry has "roadmap_id" equal to "150"
    And the oldest entry in the array has "roadmap_id" equal to "51"

  @EARS-TF-7-032 @build:mcp+journal @happy
  Scenario: resources_list enumerates four resources when journal is enabled
    Given an MCP client driving "tf mcp" (built with --features mcp,journal) as a subprocess over JSON-RPC 2.0 stdio
    When the client calls resources_list
    Then the list contains exactly 4 resources
    And the resources are "tf://status", "tf://calibration", "tf://events", and "tf://cost-journal"

  @EARS-TF-7-030 @build:mcp-only @abuse
  Scenario: The journal MCP handlers are absent when journal is not enabled
    Given an MCP client driving "tf mcp" (built with --features mcp only) as a subprocess over JSON-RPC 2.0 stdio
    When the client calls tf_journal_append with { "roadmap_id": "7", "tokens": 1, "model": "x" }
    Then the call returns a method-not-found error
    And reading tf://cost-journal returns a not-found error

  # --- summarizer (opt-in, fails-open) — TF-7-033..035 ----------------------
  #
  # The happy path is pinned to a deterministic curl stub on PATH (no live network);
  # the failure paths are split per EARS TF-7-034 / TF-7-035 failure mode. Whether a
  # crate-level HTTP client is involved is a dependency fact (TF-7-027 feature gate /
  # `cargo tree`), NOT a runtime Gherkin assertion, so it is asserted there, not here.

  @EARS-TF-7-033 @EARS-TF-7-027 @build:journal-summarizer @happy
  Scenario: close --summarize compresses the ask when the key and a working curl are available
    Given $ANTHROPIC_API_KEY is set
    And a stub "curl" earlier on PATH echoes a fixed summary "compressed: rewrite the spec" and exits zero
    And an open entry keyed "7" has a long ask
    When the recorder runs "tf journal close 7 --summarize"
    Then the record "ask_summary" equals "compressed: rewrite the spec"
    And the close exits zero
    And the record is appended to cost-journal.jsonl

  @EARS-TF-7-034 @build:journal-summarizer @abuse
  Scenario: close --summarize fails open to a 100-char truncation when the API key is absent
    Given $ANTHROPIC_API_KEY is unset
    And an open entry keyed "7" has an ask of 250 characters
    When the recorder runs "tf journal close 7 --summarize"
    Then the record "ask_summary" is the first 100 characters of the stored ask
    And the close exits zero
    And the record is appended to cost-journal.jsonl

  @EARS-TF-7-035 @build:journal-summarizer @abuse
  Scenario: close --summarize fails open to a 100-char truncation when curl is not on PATH
    Given $ANTHROPIC_API_KEY is set
    And no "curl" executable is present on PATH
    And an open entry keyed "7" has an ask of 250 characters
    When the recorder runs "tf journal close 7 --summarize"
    Then the record "ask_summary" is the first 100 characters of the stored ask
    And the close exits zero
    And the record is appended to cost-journal.jsonl

  @EARS-TF-7-035 @build:journal-summarizer @abuse
  Scenario: close --summarize fails open to a 100-char truncation when curl returns a non-zero exit code
    Given $ANTHROPIC_API_KEY is set
    And a stub "curl" earlier on PATH exits with a non-zero status
    And an open entry keyed "7" has an ask of 250 characters
    When the recorder runs "tf journal close 7 --summarize"
    Then the record "ask_summary" is the first 100 characters of the stored ask
    And the close exits zero
    And the record is appended to cost-journal.jsonl


# =============================================================================
# UI INTERACTION HANDOFF
# =============================================================================
#
# UI ELEMENTS REQUIRING INTERACTION TESTS:
#
# These live-DOM gesture paths are STORY-level and require a browser driver
# (playwright). They are tagged @ui-playwright / @deferred-to-story in the scenarios
# above; STEP 3 covers only the HTTP/contract layer those gestures ride on.
#
# - [Budget-Controls card — weekly-cap input + "Set" button on the dashboard SPA]
#     @ui-playwright (STORY) → path: load dashboard on 127.0.0.1:8088 → verify
#       Budget-Controls card visible with current weekly cap → type new value into the
#       weekly-cap input → click "Set" → verify card updates to the new value WITHOUT a
#       full page reload (value sourced from the POST /api/budget response body, not a
#       re-fetch) → reload page → verify the new weekly cap still displayed (persisted
#       to budget.json). (covers TF-6-016, TF-6-017; the no-reload / response-body
#       assertions are browser-only — the STEP-3 scenario above covers the HTTP exchange)
# - [Budget-Controls card — invalid-key rejection feedback]
#     @ui-playwright (STORY) → path: attempt to set a non-allow-listed key via the
#       card → verify the card surfaces an error and the displayed value does NOT change
#       → reload → verify budget.json was never mutated. (covers TF-6-018; the STEP-3
#       abuse-path REST scenario above covers the byte-for-byte-unchanged contract)
#
# NOTE: the journal surface ([7]) has NO slash command and NO browser UI this cycle —
# it is CLI + MCP only (plan lines 228–231). No journal UI interaction tests are owed.
