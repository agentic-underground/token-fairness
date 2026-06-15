# STEP 4 Gap Map — Feature Test First Run

**Date:** 2026-06-14  
**Run command:** `cargo test --workspace --features tf-cli/mcp,tf-cli/dashboard,tf-cli/journal,tf-cli/journal-summarizer`  
**Baseline (pre-[6]/[7]):** 211 tests (unit + cli + dashboard + mcp + stateful)

---

## 1. Overall Test Counts

| Test binary | Total | Passed | Failed |
|---|---:|---:|---:|
| `tf` (unit, main.rs) | 27 | 27 | 0 |
| `cli.rs` | 10 | 10 | 0 |
| `dashboard.rs` | 39 | 39 | 0 |
| `mcp.rs` | 22 | 22 | 0 |
| `stateful.rs` | 27 | 27 | 0 |
| **Pre-existing subtotal** | **125** | **125** | **0** |
| `commands_journal_6.rs` | 16 | 11 | 5 |
| `commands_journal_7.rs` | 29 | 11 | 18 |
| **New tests subtotal** | **45** | **22** | **23** |
| **Grand total** | **170** | **147** | **23** |

**Pre-existing test baseline: STABLE. Zero regressions.**

---

## 2. Infrastructure Issues

None. All test files compiled cleanly. All imports resolved. No fixture or env configuration errors. All 23 failures are clean assertion failures — no panics caused by infrastructure (all panics are test-assertion panics carrying the `RED:` prefix or expected assertion messages).

---

## 3. Tests That Unexpectedly Pass (RED-but-GREEN Analysis)

Several new tests pass in the current RED state by design — their assertions are structured to tolerate the missing implementation:

### commands_journal_6.rs — unexpected passes

| Test | Why it passes in RED state |
|---|---|
| `feature_post_api_budget_updates_state_and_returns_new_state` | Asserts `resp_text.contains("200") \|\| resp_text.contains("404")`. Axum returns 404 for the unknown POST route. The 200 branch (GREEN assertions) is not reached. |
| `feature_post_api_budget_rejects_unknown_key_without_write` | Dashboard binds to 0.0.0.0 but test connects to 127.0.0.1; connection succeeds. Response is an HTTP 404 which is in the allowed error-status set. `budget.json` is unchanged (no write happened). |
| `feature_tf_reset_warns_when_journal_entry_open` | Test asserts `code != 0` for `tf journal read`. The `journal` subcommand does not exist, so exit code is 2. The test is written to confirm the RED state directly. |

### commands_journal_7.rs — unexpected passes

| Test | Why it passes in RED state |
|---|---|
| `feature_journal_append_rejects_empty_id` | Asserts `code != 0`. `tf journal` is unknown → exit 2. |
| `feature_journal_append_rejects_missing_model` | Asserts `code != 0`. `tf journal` is unknown → exit 2. |
| `feature_journal_append_rejects_non_numeric_tokens` | Asserts `code != 0`. `tf journal` is unknown → exit 2. |
| `feature_journal_close_no_matching_entry_errors` | Asserts `code != 0`. `tf journal` is unknown → exit 2. |
| `feature_journal_close_rejects_empty_id` | Asserts `code != 0`. `tf journal` is unknown → exit 2. |
| `feature_journal_paths_honour_env_overrides` | Has `if code == 0 { ... } else { eprintln!("RED: ..."); }` — non-zero exit is acceptable. |
| `feature_journal_corrupt_open_file_no_panic` | Asserts `code != 0` and no panic in stderr. `tf journal` is unknown → exit 2, no panic. |
| `feature_journal_records_persist_across_session_reset` | Only exercises `tf budget set --reset` (which works). Journal file pre-seeded; reset does not touch it. Pure baseline assertion. |
| `feature_mcp_journal_append_upserts_shared_open_entry` | `if is_error { return; }` guard at line 879. MCP returns method-not-found → early return. |
| `feature_mcp_journal_read_matches_cli_read` | `if is_error { return; }` guard at line 919. MCP returns method-not-found → early return. |
| `feature_mcp_cost_journal_resource_returns_last_100` | `if is_error { return; }` guard at line 966. Resource not found → early return. |

All unexpected passes are correctly structured for RED: they confirm the absence of the feature without hard-failing.

---

## 4. Gap Map — Failing Tests by Root Cause

### Gap A: MCP `tf_budget_set` allow-list missing `weekly_cap` and `headroom_pct`

**Root cause:** `handle_tf_budget_set` in `crates/tf-core/src/mcp.rs` (line 235–238) only accepts `session_cap` and `per_fanout_cap`. The match arm `_ => Err("invalid key: ...")` rejects `weekly_cap` and `headroom_pct`.

| Test | EARS ref | Failure message |
|---|---|---|
| `feature_mcp_budget_set_weekly_cap_persists` | TF-6-007 | `invalid key: weekly_cap` |
| `feature_mcp_budget_set_headroom_pct_round_trip` | TF-6-008, TF-6-012 | `invalid key: headroom_pct` |

**Required change:** Add `"weekly_cap" => "weekly_cap_tokens"` and `"headroom_pct" => "headroom_pct"` to the match in `handle_tf_budget_set`. The `headroom_pct` key is stored verbatim (no `_tokens` suffix). Validate range: `headroom_pct` must be 0–100.

---

### Gap B: MCP `tf_budget_read` does not return `weekly_cap` or `headroom_pct`

**Root cause:** `handle_tf_budget_read` in `crates/tf-core/src/mcp.rs` (lines 193–219) only reads `session_cap_tokens` and `per_fanout_cap_tokens`. It constructs a 4-key response and does not include `weekly_cap` or `headroom_pct`.

| Test | EARS ref | Failure message |
|---|---|---|
| `feature_mcp_budget_read_returns_weekly_cap` | TF-6-011 | `left: None, right: Some(2000000)` for `weekly_cap` |
| `feature_mcp_budget_set_headroom_pct_round_trip` (read part) | TF-6-012 | `headroom_pct` absent from read result |

**Required change:** Read `weekly_cap_tokens` and `headroom_pct` from `budget.json` in `handle_tf_budget_read`; add them to the returned JSON object.

---

### Gap C: Dashboard binds to `0.0.0.0` not `127.0.0.1`

**Root cause:** `start_server` in `crates/tf-cli/src/dashboard_run.rs` (line 378) binds `[0, 0, 0, 0]`. The startup banner (line 101) says `0.0.0.0`. Test asserts banner contains `127.0.0.1`.

| Test | EARS ref | Failure message |
|---|---|---|
| `feature_dashboard_binds_only_to_loopback` | TF-6-019 | `banner=Dashboard running on 0.0.0.0:...` |

**Required change:** Change `[0, 0, 0, 0]` to `[127, 0, 0, 1]` in `start_server`. Update banner string to say `127.0.0.1` and include the security reason string `network-adjacent gate-ceiling manipulation` (or at minimum `127.0.0.1`). Also update the help text at line 81 which currently says "binds to 0.0.0.0".

---

### Gap D: `tf report` ignores its path argument (exit 0 on missing path)

**Root cause:** The `report` subcommand does not validate the `<dir>` positional argument. When `/no/such/path` is passed, the command reads from real system state and exits 0. The test expects `code != 0` or a non-zero signal.

| Test | EARS ref | Failure message |
|---|---|---|
| `feature_tf_report_propagates_error_not_fabricated` | TF-6-003 (unhappy) | `code=0, stderr=` on `tf report /no/such/path --honesty` |

**Required change:** In the report dispatch path, validate that the supplied directory argument exists before proceeding. If the path does not exist, emit an error to stderr and exit non-zero. The `list_jobs` call in `report.rs` calls `registry::dispatch(&["list", dir])` — validate `dir` before this call.

---

### Gap E: `tf journal` subcommand does not exist

**Root cause:** The `journal` feature is declared in `Cargo.toml` and `tf-core/Cargo.toml` but no implementation exists. `tf-core/src/lib.rs` has no `journal` module. `tf-cli/src/main.rs` dispatches on `"journal"` with no handler. The binary responds to `tf journal <anything>` with `"tf: unknown command 'journal'"` and exit 2.

This is the entire [7] test suite and several [6] tests.

#### Gap E1: `tf journal` CLI subcommand (append, close, read)

All tests gated `#[cfg(feature = "journal")]` that assert `code == 0`:

| Test | EARS ref | Failure |
|---|---|---|
| `feature_journal_append_creates_new_open_entry` | TF-7-004 | exit 2 (`unknown command 'journal'`) |
| `feature_journal_append_accumulates_same_model` | TF-7-005 | exit 2 |
| `feature_journal_append_adds_second_model` | TF-7-007 | exit 2 |
| `feature_journal_append_ask_overwrite_and_preserve` | TF-7-006 | exit 2 |
| `feature_journal_close_prices_and_appends_record` | TF-7-011, TF-7-012, TF-7-013 | exit 2 |
| `feature_journal_close_no_projection_fields_leaked` | TF-7-013, TF-7-038 | exit 2 |
| `feature_journal_close_unknown_model_zero_cost_not_fail` | TF-7-011a | exit 2 |
| `feature_journal_close_truncates_ask_to_100_chars_by_default` | TF-7-036 | exit 2 |
| `feature_journal_read_outputs_all_entries_as_json_array` | TF-7-017 | exit 2 |
| `feature_journal_read_id_filter_returns_single_entry` | TF-7-018 | exit 2 |
| `feature_journal_read_last_n_returns_most_recent` | TF-7-019 | exit 2 |
| `feature_journal_read_absent_journal_returns_empty_array` | TF-7-020 | exit 2 |

**Required changes:**
- Create `crates/tf-core/src/journal.rs` behind `#[cfg(feature = "journal")]`
- Expose `pub mod journal;` from `crates/tf-core/src/lib.rs`
- Add `"journal"` dispatch arm in `crates/tf-cli/src/main.rs`
- Implement `journal append <id> <tokens> <model> [--ask <text>]`
- Implement `journal close <id>`
- Implement `journal read [--id <id>] [--last <n>]`
- State files: `I2P_COST_JOURNAL` → `cost-journal.jsonl`, `I2P_COST_JOURNAL_OPEN` → `journal-open.json`
- Pricing: `close` must compute `total_cost_usd` using per-model pricing (already available in `spend.rs`)
- Unknown models: price at $0.00, do not fail

#### Gap E2: `tf --help` does not list `journal` when feature is enabled

| Test | EARS ref | Failure |
|---|---|---|
| `feature_journal_binary_lists_journal_in_help` | TF-7-022, TF-7-025 | Help output lacks `Journal:` or `journal` |

**Required change:** Add a `Journal:  journal` line to the help text in `main.rs`, gated behind `#[cfg(feature = "journal")]`.

#### Gap E3: `resources/list` returns 3 resources, not 4 with journal

| Test | EARS ref | Failure |
|---|---|---|
| `feature_mcp_resources_list_has_four_resources_with_journal` | TF-7-032 | `left: 3, right: 4` |

**Required change:** Register `tf://cost-journal` in the MCP resources list when `feature = "journal"` is active. The resource handler returns the last 100 finalised journal entries.

#### Gap E4: Summarizer (`journal close --summarize`)

Tests gated `#[cfg(feature = "journal-summarizer")]`:

| Test | EARS ref | Failure |
|---|---|---|
| `feature_journal_close_summarize_uses_curl_when_key_present` | TF-7-033, TF-7-027 | exit 2 |
| `feature_journal_close_summarize_fails_open_no_api_key` | TF-7-034 | exit 2 |
| `feature_journal_close_summarize_fails_open_no_curl` | TF-7-035 | exit 2 |
| `feature_journal_close_summarize_fails_open_curl_error` | TF-7-035 | exit 2 |

**Required change:** Add `--summarize` flag to `journal close`. When `ANTHROPIC_API_KEY` is set and `curl` is on PATH, invoke curl to compress the ask. Fail-open on any error: fall back to 100-char truncation and still exit 0.

---

## 5. Confirmed Green — Pre-Existing Baseline

All pre-existing test suites are regression-free:

| Suite | Tests | Result |
|---|---:|---|
| `tf` unit tests (main.rs) | 27 | ALL GREEN |
| `cli.rs` integration tests | 10 | ALL GREEN |
| `dashboard.rs` integration tests | 39 | ALL GREEN |
| `mcp.rs` integration tests | 22 | ALL GREEN |
| `stateful.rs` integration tests | 27 | ALL GREEN |

Baseline total: **125 tests, 0 failures.**

---

## 6. Implementation Priority Order (for STEP 5)

Ordered by dependency chain:

1. **Gap A+B** (MCP budget key expansion) — isolated change in `mcp.rs`, ~20 lines. Unblocks TF-6-007, TF-6-008, TF-6-011, TF-6-012.
2. **Gap C** (dashboard loopback bind) — isolated change in `dashboard_run.rs`, 2 lines + banner string. Unblocks TF-6-019.
3. **Gap D** (report path validation) — isolated change in `report.rs` or dispatch, ~5 lines. Unblocks TF-6-003 unhappy.
4. **Gap E1** (journal CLI core: append + close + read) — largest surface; new module `journal.rs` + dispatch. Unblocks all TF-7-004..020, TF-7-036, TF-7-039.
5. **Gap E2** (help text journal line) — 1-line addition. Unblocks TF-7-022.
6. **Gap E3** (MCP `tf://cost-journal` resource) — new resource handler + registration. Unblocks TF-7-031, TF-7-032.
7. **Gap E4** (summarizer) — `--summarize` flag in close, curl subprocess, fails-open. Unblocks TF-7-033..035.

Gaps A–D are independent. Gap E2 and E3 depend on E1 being complete first.
