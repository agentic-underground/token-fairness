# EARS Specification — token-fairness

**Edition:** 1.0  
**Created:** 2026-06-13  
**Last updated:** 2026-06-13  
**Status:** ACTIVE (Phase B: MCP Server, Phase C: Telemetry + Dashboard)

## Ubiquitous Statements

**US-001: Backward-compatible CLI**  
The system SHALL NOT modify any existing CLI verbs, options, or output format. New features are opt-in surfaces (`tf mcp`, `tf dashboard`) accessible via new top-level verbs.

**US-002: Feature-gated dependencies**  
The system SHALL gate heavy dependencies (`rmcp`, `tokio`, `axum`, `notify`) behind Cargo features (`mcp`, `dashboard`) so that the default hook build excludes them entirely. The default `cargo build --release` (no features) SHALL remain within AC#8's binary-size budget (≤105% of pre-change size).

**US-003: Stdio MCP transport**  
The system SHALL expose token-scheduler operations as MCP tools over stdio transport (JSON-RPC 2.0), implemented via the `rmcp` crate (0.2.x), invoked as `tf mcp`.

**US-004: Pure core, no I/O**  
All logic in `crates/tf-core` SHALL be pure domain logic with no I/O, no platform code, no `unwrap()`/`expect()`/`panic!()` outside tests, and no unchecked indexing. Errors are typed `thiserror` enums.

**US-005: 100% test coverage**  
The system SHALL achieve 100% line coverage and 100% branch coverage. Every function and branch has a test; every error path is deliberately triggered and asserted. The gate `cargo test --workspace` SHALL pass with all tests green.

---

## MCP-specific Statements (Phase B)

### Tool Contracts

**MCP-001: tf_gate tool**  
The `tf_gate` MCP tool SHALL accept a JSON-RPC request with inputSchema `{ ceiling: { "used_pct": number, "headroom": number } }` and return a result object with fields:
- `verdict` (string): one of `"allow"` or `"deny"`
- `reason` (string, always present): a human-readable justification (e.g., `"ceiling exceeded"`, `"no live signal"`)
- `ceiling` (object): the ceiling object from the scheduler's native verdict (for client reference)

The verdict mapping is:
- Scheduler `"CONTINUE"` → MCP `"allow"`
- Scheduler `"HALT"` | `"DEFER"` | `"ASK"` | `"NO_SIGNAL"` → MCP `"deny"`

**MCP-002: tf_budget_read tool**  
The `tf_budget_read` MCP tool SHALL accept an empty JSON-RPC request (no inputSchema required) and return a result object with fields:
- `session_cap` (integer): the session budget ceiling in tokens
- `per_fanout_cap` (integer): the per-fan-out budget ceiling in tokens
- `current_spend` (integer): total tokens spent in the current session
- `fanout_spend` (integer): tokens spent in the current fan-out window (resets on boundary)

**MCP-003: tf_budget_set tool**  
The `tf_budget_set` MCP tool SHALL accept a JSON-RPC request with inputSchema `{ key: string, value: integer }` where key ∈ `{"session_cap", "per_fanout_cap"}` and update the corresponding field in the budget state file. On success, return `{ "success": true, "key": <key>, "new_value": <value> }`.

**MCP-004: tf_report tool**  
The `tf_report` MCP tool SHALL accept an optional JSON-RPC request with inputSchema `{ window: "hour" | "day" | "month" | "ytd" }` (default: `"day"`) and return a result object with fields:
- `window` (string): the requested time window
- `window_open` (integer, unix seconds): the start timestamp of the window
- `window_close` (integer, unix seconds): the end timestamp of the window
- `spend_total` (integer): total tokens spent in the window
- `gate_denials` (integer): count of denials by the gate (verdict = `"HALT"` | `"DEFER"` | `"ASK"`)

**MCP-005: tf_observe tool**  
The `tf_observe` MCP tool SHALL accept an optional JSON-RPC request with inputSchema `{ window: "hour" | "day" | "month" | "ytd" }` (default: `"day"`) and return a result array of snapshot objects, one per deduplicated observation, with fields:
- `span_id` (string): the unique span token
- `cost_tokens` (integer): the token cost of this span
- `model` (string): the model invoked (e.g., `"claude-opus-4"`)
- `role` (string): the role that invoked it (e.g., `"researcher"`)

The array is sorted chronologically by first observation timestamp.

**MCP-006: tf_spend tool**  
The `tf_spend` MCP tool SHALL accept an optional JSON-RPC request with inputSchema `{ span_id: string, cost: integer, model: string, role: string }` and record a new spend event to the ledger. On success, return `{ "success": true, "span_id": <span_id>, "cost": <cost> }`.

**MCP-007: tf_signal tool**  
The `tf_signal` MCP tool SHALL accept a JSON-RPC request with inputSchema `{ name: "gate" | "budget" | "observability", status: "OK" | "ERROR" }` and record a signal event to the signals registry. On success, return `{ "success": true, "signal": <name>, "status": <status> }`.

**MCP-008: tf_plan_open tool**  
The `tf_plan_open` MCP tool SHALL accept a JSON-RPC request with inputSchema `{ title: string, budget_tokens: integer }` and create a new open-ended budget plan. On success, return `{ "success": true, "plan_id": string, "title": string, "budget_tokens": integer }`.

**MCP-009: tf_plan_close tool**  
The `tf_plan_close` MCP tool SHALL accept a JSON-RPC request with inputSchema `{ plan_id: string }` and mark the plan as closed (no further spend against it). On success, return `{ "success": true, "plan_id": string, "closed_at": integer }`.

**MCP-010: tf_schedule_toggle tool**  
The `tf_schedule_toggle` MCP tool SHALL accept a JSON-RPC request with inputSchema `{ enabled: boolean }` and toggle the window-aware schedule gate's enabled state. On success, return `{ "success": true, "enabled": <enabled> }`.

### Resource Contracts

**Resource-001: tf://status**  
The `tf://status` MCP resource SHALL return a JSON object with a live snapshot of all current state files:
- `session_budget`: session budget state (session_cap, per_fanout_cap, current_spend, fanout_spend)
- `signals`: current signal states (gate, budget, observability)
- `timestamp`: unix seconds of the snapshot

The resource is read-only and updated on each request (always fresh).

**Resource-002: tf://calibration**  
The `tf://calibration` MCP resource SHALL return a JSON object with calibration data:
- `rolling_windows`: array of window definitions (hour, day, month, ytd with open/close times)
- `current_window`: the current active window name
- `next_boundary`: unix seconds of the next window boundary

The resource is read-only.

**Resource-003: tf://events**  
The `tf://events` MCP resource SHALL return a JSON array of recent event summaries (last 100 events, JSONL deserialized):
- Each element: `{ "timestamp": int, "kind": string, "details": object }`
- Kinds: `"spend"`, `"signal"`, `"plan_open"`, `"plan_close"`, `"gate_verdict"`

The resource is read-only and truncated to recent 100 events for efficiency.

### Feature Gate

**Feature-001: `mcp` Cargo feature**  
The system SHALL define a Cargo feature `mcp` that gates the `rmcp` dependency and all MCP server code. When disabled (the default), `cargo build --release` SHALL NOT include `rmcp` or `tokio` symbols.

**Feature-002: Workspace feature centralization**  
The workspace root `Cargo.toml` SHALL define `[features]` with `mcp = ["rmcp"]` so that all crates inherit the gate consistently. When enabled, `tf-core` and `tf-cli` both gain access to MCP modules.

### Dispatch & Registration

**MCP-011: CLI dispatch for `tf mcp`**  
The CLI `crates/tf-cli/src/main.rs` SHALL recognize the `mcp` verb and spawn the MCP server via `rmcp::Server` when invoked as `tf mcp`. The server reads JSON-RPC 2.0 from stdin and writes responses to stdout. The process terminates on stdin EOF.

**MCP-012: MCP server initialization**  
On startup, the MCP server SHALL:
1. Register all 10 tools (MCP-001 through MCP-010) with `rmcp::Server`
2. Register all 3 resources (Resource-001 through Resource-003) with `rmcp::Server`
3. Enter the JSON-RPC request/response loop via `rmcp::Server::run()`

The server is **not** a daemon; it runs in the foreground and terminates on parent exit (Claude Code controls the subprocess lifecycle).

### Error Handling

**MCP-013: JSON-RPC error responses**  
When an MCP tool handler encounters an error:
1. Serialize it as a JSON-RPC 2.0 error response: `{ "jsonrpc": "2.0", "error": { "code": <int>, "message": <string> }, "id": <id> }`
2. Use error codes per JSON-RPC spec (e.g., `-32600` for invalid request, `-32601` for method not found)
3. Never panic or return an unstructured string error

**MCP-014: Tool handler fallibility**  
Every MCP tool handler (MCP-001 through MCP-010) is fallible. Handlers that fail to read state files, parse JSON, or compute results SHALL return a JSON-RPC error response. No unwrap/expect/panic in production code outside tests.

### Testing

**Test-MCP-001: Verdict-mapping adapter unit tests**  
The test suite SHALL include unit tests that feed each of the five scheduler verdicts (`CONTINUE`, `HALT`, `DEFER`, `ASK`, `NO_SIGNAL`) into the adapter and assert the mapped MCP verdict (`allow` or `deny`), reason presence, and ceiling presence.

**Test-MCP-002: Tool round-trip integration tests**  
The test suite SHALL include integration tests that spawn `tf mcp` as a subprocess, send valid JSON-RPC requests (e.g., `tf_gate` with a ceiling payload), read the responses, and assert on the result structure (verdict, reason, ceiling fields).

**Test-MCP-003: Resource serving tests**  
The test suite SHALL include tests that request each MCP resource (tf://status, tf://calibration, tf://events) and assert the response is valid JSON with expected keys.

**Test-MCP-004: Error-path tests**  
The test suite SHALL include tests that send malformed JSON-RPC requests (missing params, unknown method, invalid JSON) and assert the server responds with appropriate JSON-RPC errors.

**Test-MCP-005: Coverage requirement**  
All new code in `crates/tf-core/src/mcp.rs` and the MCP dispatch in `crates/tf-cli/src/main.rs` SHALL achieve 100% line and branch coverage. The test-suite gate `cargo test --workspace` is mandatory.

### No Flaky Tests

**Test-MCP-006: Deterministic tests**  
The test suite SHALL be run 3 times in a row with `cargo test --workspace mcp` and pass all 3 times without failures. Tests that depend on timing SHALL use deterministic waits or mocks, never sleep loops. Tests that interact with temp files SHALL use `testutil::temp_dir()` with proper cleanup.

---

## Dashboard-specific Statements (Phase C)

### HTTP Server & Static Assets

**DASH-001: HTTP server on port 8080**  
The `tf dashboard` command SHALL spawn an HTTP server (axum-based) listening on `127.0.0.1:8080`. The server SHALL:
1. Serve an embedded Chart.js dashboard HTML at `GET /` (no server-side templating)
2. Provide REST JSON endpoints for real-time state snapshots (see DASH-002)
3. Broadcast WebSocket events at `ws://127.0.0.1:8080/ws` (see TELEM-002)
4. Emit Prometheus metrics at `GET /metrics` (optional, see PROM-001)

**DASH-002: REST state endpoints**  
The HTTP server SHALL provide the following JSON endpoints (all GET, no auth):
- `GET /api/session-budget` → JSON object with `{ session_cap, per_fanout_cap, current_spend, ceiling_pct }`
- `GET /api/spend-by-model` → JSON array of `{ model, tokens, count }` for all models in current session
- `GET /api/guard-efficacy` → JSON object with `{ saves_count, blown_count, save_rate_pct }`
- `GET /api/estimator-accuracy` → JSON object with `{ mean_absolute_percentage_error, min_error_pct, max_error_pct }`

All responses are snapshots of the most recent fold of the event logs (see TELEM-003).

**DASH-003: Embedded HTML & assets**  
The HTML asset at `assets/dashboard.html` SHALL be embedded in the binary at compile time (no external HTTP fetch). The page SHALL:
1. Load three Chart.js charts: a gauge (spend %), a pie (by model), and a line trend (SAVES vs BLOWN over time)
2. Establish a WebSocket connection to `ws://127.0.0.1:8080/ws` on page load
3. Replay the live fold state from REST `/api/…` endpoints on first load
4. Update charts incrementally as WebSocket events arrive (see ADR-004 for parity requirement)
5. Tolerate WebSocket disconnection and reconnect on demand

---

### Telemetry Pipeline & File Watcher

**TELEM-001: File-watcher initialization**  
On `tf dashboard` startup, the system SHALL:
1. Resolve the path to `honesty-events.jsonl` via `observe::events_path()` (NOT hardcoded, NOT from CLI arg). Path resolution respects `TF_*` env overrides per ADR-003.
2. If the file does not exist, create it as an empty file (0 bytes).
3. Open the file and record the current byte offset (EOF).
4. Spawn an inotify-based watcher (via `notify` crate) to monitor the file for appends (see TELEM-002).

**TELEM-002: Real-time WebSocket broadcast**  
When a watched file is appended to:
1. The watcher detects the append event (inotify on Linux, FSEvents on macOS, platform-native elsewhere).
2. The system reads the new bytes from the recorded offset to the current EOF.
3. Each appended line (JSONL) is parsed and broadcast to all connected WebSocket clients in a single JSON message: `{ "type": "event", "data": <parsed JSONL object> }`
4. The offset is advanced to the new EOF.
5. Broadcast latency SHALL be within 100 ms (best-effort, no buffering on client disconnect).

**TELEM-003: Telemetry fold semantics**  
The system SHALL maintain a live fold of all JSONL events (from all source files: `honesty-events.jsonl`, `estimator-accuracy.jsonl`) using the same semantics as `observe.rs:fold_events()`:

1. **Session spend (gauge):** Latest cumulative value per session (dedup to most recent event). Resets on session boundary.
2. **Spend by model (pie):** Sum tokens per unique model across all observations in current session.
3. **Guard saves/blown (counter trend):** Bin events by their period window (`hour`, `day`, etc.), count saves/blown per bin, emit as time-series.
4. **Estimator accuracy (MAPE):** Fold the estimator-accuracy JSONL ledger, calculate mean absolute percentage error, emit per-period breakdown.

This fold is computed on every REST request and on every WebSocket broadcast. The fold logic SHALL EXACTLY replicate `observe.rs:fold_events()` — see ADR-004 (fold-parity invariant, AC#9).

---

### Telemetry Robustness

**TELEM-004: Truncation handling**  
If the watched file is truncated (size < current offset), the system SHALL:
1. Reset the offset to 0.
2. Seek to EOF to record the new offset.
3. Continue watching for appends (no panic, no silent failure).

This handles log rotation gracefully without requiring restart.

---

## Prometheus Metrics (Phase C)

**PROM-001: Prometheus endpoint & flag**  
When the `tf dashboard` command is invoked with `--prometheus` flag:
1. The HTTP server SHALL emit Prometheus-format metrics at `GET /metrics`.
2. Format is plain text, one metric per line, per Prometheus docs.
3. Metrics are current snapshots of the folded event state (see TELEM-003).

**PROM-002: Gauge metrics (resettable)**  
The system SHALL emit the following **gauge** metrics (allow decrease):
- `tf_session_spend_tokens` — current session cumulative spend (integer). Resets to 0 on session boundary.
- `tf_session_ceiling_percent` — spend as % of session ceiling (0–100, floating-point). Updated on every new event.
- `tf_weekly_ceiling_percent` — spend as % of 7-day rolling window ceiling (0–100, floating-point).

**PROM-003: Counter metrics (monotonic)**  
The system SHALL emit the following **counter** metrics (never decrease, always increase):
- `tf_guard_saves_total` — total count of guard SAVE verdicts across all time (integer, monotonic).
- `tf_guard_blown_total` — total count of guard BLOWN verdicts across all time (integer, monotonic).
- `tf_guard_procedural_denies_total` — total count of procedural denials (verdict = NOT CONTINUE) across all time (integer, monotonic).

**PROM-004: Metric help strings**  
Each metric in PROM-002 and PROM-003 SHALL include a help comment line (prefixed with `# HELP`) explaining its meaning and units.

---

### Feature Gate for Dashboard

**Feature-003: `dashboard` Cargo feature**  
The system SHALL define a Cargo feature `dashboard` that gates the `axum`, `tokio`, and `notify` dependencies and all dashboard/telemetry code. When disabled (the default), `cargo build --release` SHALL NOT include these symbols. The default build SHALL remain within AC#8's binary-size budget (≤105% of pre-change size).

**Feature-004: Workspace feature centralization**  
The workspace root `Cargo.toml` SHALL define `[features]` with `dashboard = ["axum", "tokio", "notify"]` so that all crates inherit the gate consistently.

---

### CLI Dispatch & Registration

**DASH-004: CLI dispatch for `tf dashboard`**  
The CLI `crates/tf-cli/src/main.rs` SHALL recognize the `dashboard` verb and dispatch to a handler that:
1. Parses optional `--prometheus` flag
2. Calls the dashboard server initialization (see DASH-001)
3. Binds to `127.0.0.1:8080` and runs the server in the foreground

The server is a foreground process; it terminates on SIGINT (Ctrl+C) or parent exit.

---

### Testing

**Test-DASH-001: File-watcher path resolution**  
The test suite SHALL include a test that sets `TF_EVENTS_DIR` env var and verifies the watcher resolves the correct path via `observe::events_path()`, not a hardcoded path.

**Test-DASH-002: Truncation robustness**  
The test suite SHALL include a test that truncates the watched file mid-stream and verifies:
1. The watcher does NOT panic.
2. The offset is reset correctly.
3. New appends are read after truncation.

**Test-DASH-003: WebSocket event ordering**  
The test suite SHALL include an integration test that:
1. Starts the dashboard server with a temp JSONL file.
2. Appends a sequence of events to the file.
3. Opens a WebSocket client and collects all broadcast events.
4. Asserts events are received in order and match the appended JSONL exactly.

**Test-DASH-004: REST endpoint snapshots**  
The test suite SHALL include tests for each REST endpoint (DASH-002) that:
1. Set up a known event state in a temp JSONL file.
2. Query the endpoint.
3. Assert the response JSON matches the expected fold (see TELEM-003).

**Test-DASH-005: Fold-parity invariant (CRITICAL)**  
The test suite SHALL include a property-based test (proptest) that:
1. Generates a fixed sequence of 50 synthetic JSONL events (spend, saves, blown, etc.).
2. Feeds the sequence into `observe.rs:fold_events()` (Rust) and the embedded JavaScript fold function (see assets/dashboard.html).
3. Asserts the final fold state (spend, saves_count, blown_count, MAPE) is identical in both implementations.
4. This test is CRITICAL (AC#9): if it fails, live charts (WS feed) diverge from reloaded charts (REST feed).

**Test-DASH-006: Prometheus format validation**  
The test suite SHALL include a test that:
1. Starts the dashboard with `--prometheus` flag.
2. Queries `GET /metrics`.
3. Parses the response as Prometheus text format.
4. Asserts exactly 6 metrics are present with correct types (gauges, counters).

**Test-DASH-007: Binary-size constraint (AC#8)**  
The test suite SHALL include a test that:
1. Builds `cargo build --release` (default, no features).
2. Measures binary size.
3. Asserts size is ≤105% of pre-change baseline.

**Test-DASH-008: Coverage requirement**  
All new code in `crates/tf-core/src/telemetry.rs`, `crates/tf-core/src/dashboard.rs`, and `crates/tf-cli/src/dashboard_run.rs` SHALL achieve 100% line and branch coverage. The gate `cargo test --workspace` is mandatory.

**Test-DASH-009: No flaky tests**  
The test suite SHALL be run 3 times in a row with `cargo test --workspace dashboard` and pass all 3 times without failures.

---

## Non-Dashboard Statements (Inherited from Prior Phases)

**S-001 through S-100:** [Omitted — see ROADMAP.md AC#1–10, prior iteration commits for pre-MCP requirements]

---

## Acceptance Criteria Traceability

| AC # | Requirement | EARS Statement(s) |
|------|-----------|----------|
| 1 | `tf mcp` starts without error | MCP-011, Feature-001 |
| 2 | `tf_gate` returns {verdict, reason, ceiling} | MCP-001, Test-MCP-001 |
| 3 | `tf_budget_read`/`tf_budget_set` work, state persists | MCP-002, MCP-003 |
| 4 | `tf_report`, `tf_observe`, `tf_spend` return JSON | MCP-004, MCP-005, MCP-006 |
| 5 | `tf dashboard` serves HTML with Chart.js | DASH-001, DASH-003, DASH-004, Test-DASH-003 |
| 6 | WebSocket receives events within 1s | TELEM-002, Test-DASH-003 |
| 7 | `GET /metrics` valid Prometheus format | PROM-001, PROM-002, PROM-003, Test-DASH-006 |
| 8 | Binary size ≤105% of pre-change | US-002, Feature-003, Feature-004, Test-DASH-007 |
| 9 | Fold-parity invariant (JS ↔ Rust match) | TELEM-003, Test-DASH-005 (CRITICAL) |
| 10 | `tf --help` lists `mcp` and `dashboard` | US-001 (new verbs visible) |

---

## Revision History

| Date | Editor | Change |
|------|--------|--------|
| 2026-06-13 | Handler | Initial: MCP-001 through MCP-010, Resource-001–003, Feature gates, error handling, test contracts |
| 2026-06-13 | Handler | Phase C: DASH-001–004, TELEM-001–004, PROM-001–004, Feature-003–004, Test-DASH-001–009, AC mapping updated |
| 2026-06-14 | ds-step-1-ears | Cycle [6]/[7]: TF-6-001..019 (slash commands + budget keys + write surface) and TF-7-001..040 (cost journal + feature gate + MCP tools/resource + summarizer); SMU created; [6]/[7] traceability matrices added. |
| 2026-06-14 | ds-step-1-ears | EARS-REVIEWER remediation: TF-7-011 now cites public `spend::price_by_model` + new TF-7-011a (HIGH-1); TF-7-025 authorises help-text String restructuring (HIGH-2); GATE-PR-SEQ process gate added for PR-A→PR-B sequencing (MEDIUM-1); SMU banner cite → `dashboard_run::run` and TF-6-005/SMU §3 warning text inlined verbatim (MEDIUM-2); TF-7-010 strict-parse Inv vs `state::digits_or` (LOW-1). |

---

# Cycle [6] + [7] — Slash Commands, Budget Controls, Cost Journal

> **Source of truth:** plan `doc/[6-7]_SLASH_COMMANDS_JOURNAL_PLAN.md`; approved design
> `/home/user/.claude/plans/yes-roadmap-the-token-sparkling-liskov.md`; SMU
> `doc/SUBJECT_MATTER_UNDERSTANDING.md`.
>
> Each `TF-{item}-{NNN}` statement is a **requirement**, not a test (tests are authored in STEP 2/3).
> Every statement carries explicit **Pre / Post / Inv** clauses. EARS form is tagged per statement:
> *Ubiquitous* (the system shall), *Event* (when…), *State* (while…), *Unwanted* (if…then…),
> *Optional* (where…enabled…). Numbering is contiguous within each item.

## [6] Flexible budget controls + slash commands

### Slash commands

**TF-6-001 (Event) — `/tf:help` renders live help.**
WHEN the Operator invokes `/tf:help`, THE SYSTEM SHALL render the verbatim output of `tf --help`
followed by the available slash-command list (`/tf:help`, `/tf:report`, `/tf:reset`, `/schedule`).
- **Pre:** the `tf` binary is resolvable by the skill runtime.
- **Post:** the conversation contains the unmodified `tf --help` text plus the slash-command list.
- **Inv:** the slash-command HELP text is produced by invoking `tf --help`, never hardcoded, so it
  cannot drift as subcommands are added/removed.

**TF-6-002 (Unwanted) — help command list is never hardcoded.**
IF the `/tf:help` skill would emit a subcommand list not sourced from `tf --help`, THEN THE SYSTEM
SHALL instead emit the captured `tf --help` output, so the displayed subcommands always match the binary.
- **Pre:** none. **Post:** displayed subcommands == binary subcommands. **Inv:** no static subcommand list in the skill.

**TF-6-003 (Event) — `/tf:report` renders the honesty report.**
WHEN the Operator invokes `/tf:report`, THE SYSTEM SHALL run `tf report . --honesty`, render its
output in the conversation, AND include a link to the live dashboard URL.
- **Pre:** the `tf` binary is resolvable. **Post:** the honesty report output and a dashboard link appear in the conversation.
- **Inv:** the report content is the verbatim `tf report . --honesty` output, not a paraphrase.

**TF-6-004 (Event) — `/tf:reset` re-baselines the session.**
WHEN the Operator invokes `/tf:reset`, THE SYSTEM SHALL run `tf budget set --reset` then
`tf session-boundary`, AND confirm the new baseline in the conversation.
- **Pre:** the `tf` binary is resolvable. **Post:** `budget.json` `baseline_tokens` equals the current cumulative session total; the conversation states the new baseline.
- **Inv:** reset re-anchors the baseline only; caps (`session_cap`, `weekly_cap`, etc.) are unchanged.

**TF-6-005 (State) — reset skill warns while an entry is open.**
WHILE a cost-journal entry is open, the `/tf:reset` SKILL SHALL warn the Operator to close the open
entry first (with `tf journal close <id>`) before resetting.
- **Pre:** an entry may or may not be open. **Post:** if open, the warning text is shown before reset proceeds.
- **Inv:** the warning text carried verbatim in `tf-reset/SKILL.md` is exactly:
  > "Do not run while a cost-journal entry is open — the baseline re-anchor will corrupt the entry's
  > token delta. Close the open journal entry first with `tf journal close <id>`."
  This is the testable string (sourced from the approved design, plan lines 150–152). The reset-safe
  delta-at-open/close alternative (plan lines 153–154) is DEFERRED to v1.1. (BLOCKER #2)

**TF-6-006 (Unwanted) — reset corruption mitigation.**
IF the Operator resets while an entry is open, THEN THE SYSTEM SHALL have surfaced the corruption
warning (TF-6-005) prior to the reset, this cycle's only mitigation (reset-safe delta is deferred to v1.1).
- **Pre:** an entry is open. **Post:** the warning was shown. **Inv:** no silent corruption without a prior warning.

### Budget-key expansion (MCP)

**TF-6-007 (Event) — `tf_budget_set` accepts `weekly_cap`.**
WHEN `tf_budget_set` receives `{ key: "weekly_cap", value: <int> }`, THE SYSTEM SHALL persist it to
`budget.json` under `weekly_cap_tokens` and return a success object.
- **Pre:** `value` is a valid integer. **Post:** `budget.json.weekly_cap_tokens == value`; success returned.
- **Inv:** the write goes through `budget::set_field` (INV-ONE-WRITE-PATH).

**TF-6-008 (Event) — `tf_budget_set` accepts `headroom_pct`.**
WHEN `tf_budget_set` receives `{ key: "headroom_pct", value: <int> }`, THE SYSTEM SHALL persist it
to `budget.json` under `headroom_pct` and return a success object.
- **Pre:** `value` is a valid integer. **Post:** `budget.json.headroom_pct == value`; success returned.
- **Inv:** through `budget::set_field`.

**TF-6-009 (Event) — `tf_budget_set` continues to accept legacy keys.**
WHEN `tf_budget_set` receives key `session_cap` or `per_fanout_cap`, THE SYSTEM SHALL persist it to
`session_cap_tokens` / `per_fanout_cap_tokens` as before, with no behaviour change.
- **Pre:** valid integer value. **Post:** corresponding on-disk key updated. **Inv:** backward compatible with MCP-003.

**TF-6-010 (Unwanted) — `tf_budget_set` rejects unknown keys.**
IF `tf_budget_set` receives a key not in `{session_cap, per_fanout_cap, weekly_cap, headroom_pct}`,
THEN THE SYSTEM SHALL return an error AND SHALL NOT write to `budget.json`.
- **Pre:** key ∉ allow-list. **Post:** error returned; `budget.json` unchanged.
- **Inv:** INV-ALLOWLIST — no arbitrary key write is ever possible.

**TF-6-011 (Event) — `tf_budget_read` returns `weekly_cap`.**
WHEN `tf_budget_read` is called, THE SYSTEM SHALL include `weekly_cap` (sourced from
`weekly_cap_tokens`) in the returned object alongside the existing keys.
- **Pre:** none. **Post:** response contains `weekly_cap`. **Inv:** value round-trips with TF-6-007.

**TF-6-012 (Event) — `tf_budget_read` returns `headroom_pct`.**
WHEN `tf_budget_read` is called, THE SYSTEM SHALL include `headroom_pct` in the returned object
alongside the existing keys (`session_cap`, `per_fanout_cap`, `current_spend`, `fanout_spend`).
- **Pre:** none. **Post:** response contains `headroom_pct`. **Inv:** value round-trips with TF-6-008; no existing key removed.

### Pure write path

**TF-6-013 (Ubiquitous) — single budget write path.**
THE SYSTEM SHALL expose a pure function `budget::set_field(key, value) -> Result<Value>` that is the
sole code path mutating `budget.json`, used by both the CLI set-arm and `POST /api/budget`.
- **Pre:** none. **Post:** all budget mutations route through one function. **Inv:** INV-ONE-WRITE-PATH.

**TF-6-014 (Ubiquitous) — `set_field` enforces the allow-list.**
THE `budget::set_field` function SHALL accept only keys in
`{session_cap, per_fanout_cap, weekly_cap, headroom_pct}` and return an error for any other key
without performing a write.
- **Pre:** none. **Post:** unknown key → error, no write. **Inv:** INV-ALLOWLIST (shared by CLI and HTTP).

**TF-6-015 (Event) — `set_field` returns the new state.**
WHEN `budget::set_field` succeeds, THE SYSTEM SHALL return the updated budget state as JSON.
- **Pre:** allow-listed key, valid value. **Post:** returned JSON reflects the post-write `budget.json`. **Inv:** returned state == persisted state.

### Dashboard write surface + security

**TF-6-016 (Event) — `POST /api/budget` updates the budget.**
WHEN the Dashboard browser POSTs `/api/budget` with `{ key, value }`, THE SYSTEM SHALL delegate to
`budget::set_field`, persist the change, AND return the new budget state in the response body.
- **Pre:** body parses to `{key, value}`. **Post:** `budget.json` updated; response body is the new state.
- **Inv:** the client updates its display from the response body — no WebSocket broadcast is required.

**TF-6-017 (Event) — dashboard card reflects response.**
WHEN `POST /api/budget` returns successfully, THE SYSTEM (browser card) SHALL update the displayed
budget values from the response body without a full page reload.
- **Pre:** a successful POST response. **Post:** card shows the new values. **Inv:** display source is the response body, not a re-fetch.

**TF-6-018 (Unwanted) — `POST /api/budget` rejects invalid keys.**
IF `POST /api/budget` receives a key not on the allow-list, THEN THE SYSTEM SHALL return an error
response AND SHALL NOT modify `budget.json`.
- **Pre:** key ∉ allow-list. **Post:** error response; `budget.json` unchanged. **Inv:** INV-ALLOWLIST (same `set_field` guard as TF-6-014).

**TF-6-019 (Ubiquitous) — loopback bind for the write surface.**
THE SYSTEM SHALL bind the dashboard HTTP server to `127.0.0.1` (never `0.0.0.0`) whenever a write
endpoint is active, AND the startup banner SHALL state the `127.0.0.1` bind address and the reason
(prevent network-adjacent gate-ceiling manipulation).
- **Pre:** the dashboard is started. **Post:** server is reachable only over loopback; banner states `127.0.0.1`.
- **Inv:** INV-LOOPBACK — no non-loopback interface ever serves `POST /api/budget`. (Opus-review blocker #1)

---

## [7] Request-shape cost journal

### Path resolution (BLOCKER #1)

**TF-7-001 (Ubiquitous) — `journal_path()` resolution.**
THE SYSTEM SHALL resolve the finalised-journal path via `journal::journal_path()`, returning
`$I2P_COST_JOURNAL` when set, else `{state::state_dir()}/cost-journal.jsonl`.
- **Pre:** none. **Post:** path honours the env override. **Inv:** INV-ENV-PATH; mirrors `observe::events_path()`; never a hardcoded `~`.

**TF-7-002 (Ubiquitous) — `journal_open_path()` resolution.**
THE SYSTEM SHALL resolve the open-entry staging path via `journal::journal_open_path()`, returning
`$I2P_COST_JOURNAL_OPEN` when set, else `{state::state_dir()}/journal-open.json`.
- **Pre:** none. **Post:** path honours the env override. **Inv:** INV-ENV-PATH; enables `testutil::ENV_LOCK` test isolation.

**TF-7-003 (State) — env overrides take precedence.**
WHILE `$I2P_COST_JOURNAL` or `$I2P_COST_JOURNAL_OPEN` is set, THE SYSTEM SHALL use the env value in
preference to the default state-dir path for the respective file.
- **Pre:** the env var is set. **Post:** the override path is used. **Inv:** override wins deterministically.

### `tf journal append` — upsert (BLOCKER #3)

**TF-7-004 (Event) — append creates an open entry.**
WHEN `tf journal append <id> <tokens> <model>` runs and no open entry exists for `<id>`, THE SYSTEM
SHALL create one in `journal-open.json` keyed by `<id>` with `ts_opened`, `accumulated_tokens=<tokens>`,
and `by_model={<model>: <tokens>}`.
- **Pre:** `journal-open.json` has no `<id>` key. **Post:** the keyed entry exists with the initial values.
- **Inv:** the staging file is a JSON object keyed by `roadmap_id`.

**TF-7-005 (Event) — append accumulates on existing entry.**
WHEN `tf journal append <id> <tokens> <model>` runs and an open entry for `<id>` exists, THE SYSTEM
SHALL add `<tokens>` to both `by_model[<model>]` and `accumulated_tokens`.
- **Pre:** `<id>` key exists. **Post:** per-model and total accumulators increased by `<tokens>`.
- **Inv:** existing accumulators are never reset by a subsequent append.

**TF-7-006 (Event) — append upserts the ask.**
WHEN `tf journal append <id> ... --ask "<text>"` runs, THE SYSTEM SHALL store/overwrite the entry's
`ask` field with `<text>`.
- **Pre:** `<id>` open entry exists or is being created. **Post:** `ask == <text>`. **Inv:** `--ask` overwrites; omitting `--ask` leaves the prior ask unchanged.

**TF-7-007 (Event) — append accumulates a new model on an existing entry.**
WHEN `tf journal append <id> <tokens> <model>` runs with a `<model>` not yet present in the entry's
`by_model`, THE SYSTEM SHALL add the `<model>` key with `<tokens>` and add `<tokens>` to the total.
- **Pre:** entry exists; `<model>` absent from `by_model`. **Post:** new `by_model` key present; total increased.
- **Inv:** model breakdown is additive and complete.

**TF-7-008 (Unwanted) — append rejects an empty id.**
IF `tf journal append` is invoked with an empty `<id>`, THEN THE SYSTEM SHALL return an error AND
SHALL NOT modify `journal-open.json`.
- **Pre:** `<id>` is empty. **Post:** error; staging file unchanged. **Inv:** no entry is keyed by the empty string.

**TF-7-009 (Unwanted) — append rejects a missing model.**
IF `tf journal append` is invoked without a `<model>` argument, THEN THE SYSTEM SHALL return an error
AND SHALL NOT modify `journal-open.json`.
- **Pre:** `<model>` absent. **Post:** error; staging file unchanged. **Inv:** every accumulated token is attributable to a model.

**TF-7-010 (Unwanted) — append rejects non-numeric tokens.**
IF `tf journal append` is invoked with a non-numeric `<tokens>` argument, THEN THE SYSTEM SHALL
return an error AND SHALL NOT modify `journal-open.json`.
- **Pre:** `<tokens>` is not a non-negative integer. **Post:** error; staging file unchanged.
- **Inv:** accumulators stay integer-valued.
- **Inv (strict parse):** token parsing SHALL use a strict, error-returning parse (`i64::from_str` or
  equivalent), NOT `state::digits_or` (which defaults silently on bad input). A non-numeric token must
  surface an error, never be coerced to a default.

### `tf journal close` — finalise (BLOCKER #3)

**TF-7-011 (Event) — close prices and finalises.**
WHEN `tf journal close <id>` runs and an open entry exists, THE SYSTEM SHALL compute `total_tokens`
and `total_cost_usd` by pricing the entry's `by_model` breakdown via the **public** helper
`spend::price_by_model`, AND append one finalised record line to `cost-journal.jsonl` via
`state::append_line`.
- **Pre:** `<id>` open entry exists. **Post:** exactly one record appended to `cost-journal.jsonl`.
- **Inv:** `total_tokens == sum(by_model values)`; cost is priced through the shared price table
  (`spend::load_prices` honoured by `price_by_model`); the journal SHALL NOT reach into the private
  `spend::default_prices`.

**TF-7-011a (Ubiquitous) — public pricing helper for `by_model`.**
THE SYSTEM SHALL expose a public function
`spend::price_by_model(by_model: &std::collections::BTreeMap<String, i64>) -> (Vec<(String, i64, f64)>, i64, f64)`
that, for an open entry's per-model token map, returns (a) a per-model vector of
`(model, tokens, cost_usd)`, (b) the summed `total_tokens`, and (c) the summed `total_cost_usd`.
Pricing SHALL reuse the same in-crate rate table the existing `spend::aggregate` uses
(`spend::load_prices` / `spend::price_of`), so the journal and the spend audit price identically.
- **Pre:** `by_model` is a model→token map (the open-entry shape, not transcript lines). **Post:** the
  returned `total_tokens` equals `sum(by_model values)` and `total_cost_usd` equals the sum of the
  per-model costs. **Inv:** unpriced models (no prefix match in the table) contribute `0.0` cost and
  are still listed with their token count; this is the sole public pricing entry point the journal uses.

**TF-7-012 (Event) — close clears the open entry.**
WHEN `tf journal close <id>` succeeds, THE SYSTEM SHALL remove the `<id>` key from `journal-open.json`.
- **Pre:** finalised record appended. **Post:** `<id>` key absent from `journal-open.json`.
- **Inv:** INV-CLOSE-CLEARS — a closed id cannot be later corrupted by a reset.

**TF-7-013 (Ubiquitous) — finalised record schema (total-only).**
THE finalised `cost-journal.jsonl` record SHALL contain `ts`, `roadmap_id`, `ask_summary`,
`by_model` (per-model tokens+cost), `total_tokens`, and `total_cost_usd`, AND SHALL NOT contain any
`projections`, per-phase `phases`, or blended-rate fields.
- **Pre:** a close occurs. **Post:** the record matches the total-only schema. **Inv:** INV-NO-LEAK (Opus-review blocker #2 — [8] schema must not leak).

**TF-7-014 (Unwanted) — close with no open entry.**
IF `tf journal close <id>` runs and no open entry exists for `<id>`, THEN THE SYSTEM SHALL return an
error AND SHALL NOT append to `cost-journal.jsonl`.
- **Pre:** `<id>` key absent from `journal-open.json`. **Post:** error; no record appended. **Inv:** no empty/phantom record is ever written.

**TF-7-015 (Unwanted) — close rejects an empty id.**
IF `tf journal close` is invoked with an empty `<id>`, THEN THE SYSTEM SHALL return an error AND
SHALL NOT append to `cost-journal.jsonl`.
- **Pre:** `<id>` empty. **Post:** error; no append. **Inv:** id is required.

**TF-7-016 (State) — entries persist across sessions.**
WHILE finalised records exist in `cost-journal.jsonl`, THE SYSTEM SHALL retain them across sessions
(the file is HOME-rooted via `journal_path()` and append-only).
- **Pre:** ≥1 record written. **Post:** records survive session boundaries and resets. **Inv:** append-only; no record is mutated or removed by close.

### `tf journal read`

**TF-7-017 (Event) — read outputs a JSON array.**
WHEN `tf journal read` runs with no filters, THE SYSTEM SHALL output a JSON array of all finalised
journal entries.
- **Pre:** none. **Post:** stdout is a JSON array (possibly empty). **Inv:** output is valid JSON.

**TF-7-018 (Event) — read filters by id.**
WHEN `tf journal read --id <id>` runs, THE SYSTEM SHALL output only entries whose `roadmap_id`
equals `<id>`.
- **Pre:** none. **Post:** array contains only matching entries. **Inv:** filtering does not mutate the file.

**TF-7-019 (Event) — read limits by count.**
WHEN `tf journal read --last N` runs, THE SYSTEM SHALL output at most the last `N` entries
(most recent last).
- **Pre:** `N` ≥ 0. **Post:** array length ≤ N. **Inv:** ordering is chronological by append order.

**TF-7-020 (State) — read on empty journal.**
WHILE `cost-journal.jsonl` is absent or empty, THE SYSTEM SHALL output an empty JSON array `[]`
without error.
- **Pre:** file absent/empty. **Post:** stdout is `[]`; exit success. **Inv:** read never fails on an empty journal.

**TF-7-021 (Unwanted) — reset note for open entries (cross-item).**
IF the Operator resets the session while an entry is open, THEN the warning of TF-6-005 SHALL apply;
the journal SHALL NOT, this cycle, auto-protect the open entry's token delta (deferred to v1.1).
- **Pre:** an entry is open. **Post:** warning shown; no auto-protection. **Inv:** the v1 contract is warning-only (Opus-review blocker #3).

### Feature gate (AC[7].4 — NON-NEGOTIABLE)

**TF-7-022 (Ubiquitous) — `journal` Cargo feature.**
THE SYSTEM SHALL define a Cargo feature `journal` (in both `tf-core` and `tf-cli`) that gates the
entire journal module and the `tf journal` dispatch.
- **Pre:** none. **Post:** feature declared in both crates. **Inv:** `tf-cli/journal = ["tf-core/journal"]`.

**TF-7-023 (Ubiquitous) — gated module declaration.**
THE `journal` module SHALL be declared `#[cfg(feature = "journal")] pub mod journal;` in
`crates/tf-core/src/lib.rs`, so no journal code compiles without the feature.
- **Pre:** none. **Post:** module reachable only under the feature. **Inv:** INV-GATE.

**TF-7-024 (Ubiquitous) — gated CLI dispatch arm.**
THE `"journal" => …` match arm in `crates/tf-cli/src/main.rs` SHALL be `#[cfg(feature = "journal")]`,
mirroring the `mcp`/`dashboard` arms.
- **Pre:** none. **Post:** the `journal` verb exists only under the feature. **Inv:** INV-GATE.

**TF-7-025 (Ubiquitous) — genuinely-gated help line (requires help-text restructuring).**
THE `tf --help` text SHALL be restructured from its current single `&'static str` literal (one
string passed to `Out::ok(...)` in `crates/tf-cli/src/main.rs`, the `"--help" | "-h" | "help"` arm)
into a runtime-assembled `String`, so that the `Journal:` line can be appended conditionally:
```rust
let mut help = String::from("usage: tf <command> [args]\n\n…MCP: mcp\nDashboard: dashboard\n");
#[cfg(feature = "journal")]
help.push_str("             Journal:   journal\n");
help.push_str("\n             Run `tf <command>` with no args for per-command usage.\n");
Out::ok(help)
```
The `Journal:` fragment SHALL be appended only under `#[cfg(feature = "journal")]` (NOT the
unconditional pattern used by the existing `MCP:`/`Dashboard:` lines, which print even in a
no-features binary).
- **Pre:** none. **Post:** the help arm builds a `String` and the `Journal:` line appears only in
  feature-enabled builds; the existing pre-`Journal` text (verbs, `MCP:`, `Dashboard:` lines) is
  byte-for-byte unchanged from today's literal.
- **Inv:** a single `&str` literal cannot be `#[cfg]`-gated per-line, so the literal MUST become a
  built `String`; the help text is feature-accurate (AC[7].4: no-features `tf --help` omits `journal`).

**TF-7-026 (Unwanted) — no-features binary hides journal.**
IF the binary is built with no features (`cargo build --release -p tf-cli`), THEN `tf --help` SHALL
NOT list `journal` AND the binary SHALL have no `journal` subcommand.
- **Pre:** no-features build. **Post:** no `journal` in help; invoking `tf journal` is unrecognised.
- **Inv:** INV-GATE — the hot-hook binary never links journal code (AC[7].4).

**TF-7-027 (Ubiquitous) — summarizer second gate.**
THE summarizer code SHALL ride a second Cargo feature `journal-summarizer` that implies `journal`,
adding NO new crate dependency (curl subprocess only).
- **Pre:** none. **Post:** `journal-summarizer = ["journal"]` declared; no HTTP-client crate added.
- **Inv:** hook binary size unaffected; INV-GATE extended to the summarizer.

### MCP tools + resource

**TF-7-028 (Event) — `tf_journal_append` mirrors the CLI.**
WHEN the `tf_journal_append` MCP tool is called with `{ roadmap_id, tokens, model, ask? }`, THE
SYSTEM SHALL perform the same upsert as `tf journal append` against the shared journal state.
- **Pre:** valid params. **Post:** open entry upserted identically to the CLI. **Inv:** CLI and MCP operate on the same `journal-open.json`.

**TF-7-029 (Event) — `tf_journal_read` mirrors the CLI.**
WHEN the `tf_journal_read` MCP tool is called with `{ last?, roadmap_id? }`, THE SYSTEM SHALL return
the same entries `tf journal read` would output for those filters.
- **Pre:** none. **Post:** returned entries match the CLI for equivalent filters. **Inv:** one read semantics across surfaces.

**TF-7-030 (Ubiquitous) — journal MCP handlers double-gated.**
THE journal MCP tool handlers and the `tf://cost-journal` resource SHALL be gated behind BOTH the
`mcp` AND `journal` features.
- **Pre:** none. **Post:** present only when both features are on. **Inv:** INV-GATE; no journal symbol leaks via the mcp-only build.

**TF-7-031 (Event) — `tf://cost-journal` returns last 100.**
WHEN the `tf://cost-journal` MCP resource is read, THE SYSTEM SHALL return a JSON array of the last
100 finalised journal entries.
- **Pre:** none. **Post:** array length ≤ 100, most-recent entries. **Inv:** read-only; mirrors the `tf://events` last-100 pattern.

**TF-7-032 (Event) — `resources_list` enumerates four.**
WHEN `resources_list` is called (with `journal` enabled), THE SYSTEM SHALL enumerate exactly 4
resources (`tf://status`, `tf://calibration`, `tf://events`, `tf://cost-journal`).
- **Pre:** `mcp` + `journal` enabled. **Post:** list length == 4. **Inv:** the existing `== 3` assertion becomes `== 4`.

### Summarizer (opt-in, fails-open — BLOCKER #4)

**TF-7-033 (Optional) — `--summarize` compresses the ask.**
WHERE `journal-summarizer` is enabled AND `tf journal close --summarize` is invoked AND
`$ANTHROPIC_API_KEY` is set AND `curl` is available, THE SYSTEM SHALL summarize the stored ask via a
curl subprocess and store the result as `ask_summary`.
- **Pre:** feature on, key set, curl present, call succeeds. **Post:** `ask_summary` is the model summary.
- **Inv:** no Rust HTTP-client crate is used; only a `curl` subprocess.

**TF-7-034 (Unwanted) — summarize fails open on missing key.**
IF `--summarize` is requested but `$ANTHROPIC_API_KEY` is unset, THEN THE SYSTEM SHALL fall back to
`ask_summary` = the raw ask truncated to 100 characters, AND the close SHALL still succeed.
- **Pre:** `--summarize` requested, key unset. **Post:** `ask_summary` is a ≤100-char truncation; close succeeds.
- **Inv:** fails-open — summarization never blocks a close. (Deterministic test seam.)

**TF-7-035 (Unwanted) — summarize fails open on absent curl / call failure.**
IF `--summarize` is requested but `curl` is absent OR the subprocess call fails, THEN THE SYSTEM
SHALL fall back to the 100-char truncation AND the close SHALL still succeed.
- **Pre:** `--summarize` requested; curl missing or call non-zero. **Post:** ≤100-char `ask_summary`; close succeeds.
- **Inv:** every summarizer failure mode degrades to the same fallback; no failure mode aborts the close.

**TF-7-036 (Ubiquitous) — close without `--summarize`.**
THE SYSTEM SHALL, when `tf journal close <id>` runs without `--summarize`, set `ask_summary` to the
stored ask truncated to 100 characters (no subprocess invoked).
- **Pre:** `--summarize` not passed. **Post:** `ask_summary` is the truncation; no subprocess spawned.
- **Inv:** the default path is network-free and deterministic.

### Cross-cutting

**TF-7-037 (Ubiquitous) — CLI/MCP share one journal state.**
THE SYSTEM SHALL ensure `tf journal *` (CLI) and the `tf_journal_*` MCP tools operate on the same
`journal-open.json` and `cost-journal.jsonl` files (as resolved by TF-7-001/002).
- **Pre:** none. **Post:** a CLI append is visible to an MCP read and vice versa. **Inv:** single source of truth per env-resolved path.

**TF-7-038 (Unwanted) — reviewer rejects schema leak.**
IF any journal record field resembles [8]/[7b] scope (`projections`, `opus_only_cost_usd`,
per-phase `phases`, blended-rate), THEN THE SYSTEM SHALL be considered non-conformant and the field
SHALL NOT ship this cycle.
- **Pre:** a candidate record. **Post:** only total-only fields are present. **Inv:** INV-NO-LEAK (negative requirement).

**TF-7-039 (Ubiquitous) — no panics in journal code.**
THE journal module SHALL contain no `unwrap()`/`expect()`/`panic!()` outside tests; all I/O and parse
failures return typed errors.
- **Pre:** none. **Post:** error paths return `Result::Err`. **Inv:** consistent with US-004 (pure-core discipline).

**TF-7-040 (Ubiquitous) — coverage sees journal code (BLOCKER #6).**
THE CI coverage, test, clippy, and release jobs SHALL build with `journal` in their feature list so
the `journal` module is visible to the 83% llvm-cov floor.
- **Pre:** none. **Post:** `verify.yml` (clippy/test/llvm-cov/release) and `release.yml` (cross/native) all include `journal`; the llvm-cov job — which currently has no `--features` — is corrected.
- **Inv:** the feature list passed to coverage equals the contract under test; the inconsistency is a reportable finding, not a silent fix.

---

## [6] Acceptance Criteria Traceability

| AC | Acceptance criterion | EARS-IDs |
|----|----------------------|----------|
| AC[6].1 | `/tf:help` renders `tf --help` verbatim + slash list, not hardcoded | TF-6-001, TF-6-002 |
| AC[6].2 | `/tf:report` runs honesty report, renders it, links dashboard | TF-6-003 |
| AC[6].3 | `/tf:reset` rebaselines + confirms; SKILL.md open-journal warning | TF-6-004, TF-6-005, TF-6-006 |
| AC[6].4 | `tf_budget_set` accepts `weekly_cap` / `headroom_pct` | TF-6-007, TF-6-008, TF-6-009, TF-6-010 |
| AC[6].5 | `tf_budget_read` returns `weekly_cap` / `headroom_pct` (round-trip) | TF-6-011, TF-6-012 |
| AC[6].6 | Dashboard binds `127.0.0.1`; card interactive; `POST /api/budget` updates + returns state; client reads body | TF-6-013, TF-6-014, TF-6-015, TF-6-016, TF-6-017, TF-6-018, TF-6-019 |
| AC[6].7 | All tests green; fmt/clippy clean | (process gate — covered by US-005 + per-statement Post clauses; not a behavioural EARS) |

**[6] AC coverage: 6/6 behavioural ACs mapped to ≥1 EARS-ID. AC[6].7 is a process/CI gate.**

## [7] Acceptance Criteria Traceability

| AC | Acceptance criterion | EARS-IDs |
|----|----------------------|----------|
| AC[7].1 | `tf journal append|close|read` end-to-end; persist across sessions; HOME-rooted | TF-7-001, TF-7-002, TF-7-003, TF-7-004..012, TF-7-011a, TF-7-013, TF-7-016, TF-7-017..020 |
| AC[7].2 | `tf_journal_append` / `tf_journal_read` MCP tools round-trip with CLI state | TF-7-028, TF-7-029, TF-7-037 |
| AC[7].3 | `tf://cost-journal` returns last 100; `resources_list` == 4 | TF-7-031, TF-7-032 |
| AC[7].4 | No-features `tf --help` omits `journal`; no `journal` subcommand | TF-7-022, TF-7-023, TF-7-024, TF-7-025, TF-7-026, TF-7-030 |
| AC[7].5 | `--summarize` curl subprocess; fails-open to 100-char truncation | TF-7-027, TF-7-033, TF-7-034, TF-7-035, TF-7-036 |
| AC[7].6 | All tests green; fmt/clippy clean on `--features mcp,dashboard,journal` | TF-7-039, TF-7-040 (+ process gate) |

**[7] AC coverage: 6/6 behavioural ACs mapped to ≥1 EARS-ID. Supporting invariants TF-7-013/021/038 (no-leak, reset interaction) trace to SMU §5 Opus-review blockers.**

**Uncovered ACs: 0** (AC[6].7 and AC[7].6 are CI/process gates, not behavioural requirements; their
behavioural component — no panics, coverage visibility — is covered by TF-7-039/040 and US-005.)

### [7] Process gate — PR sequencing (BLOCKER #5, non-behavioural)

> **GATE-PR-SEQ (process, not a behavioural EARS).** Both [6] and [7] edit the shared surface
> `crates/tf-core/src/mcp.rs` (budget-key map for [6]; journal tools/resource for [7]). To serialise
> those edits and keep two coherent review diffs (SMU §6), the cycle ships as two sequential PRs:
> **PR-A = [6]**, **PR-B = [7] rebased on PR-A's merge**.
>
> **PR-B Pre-condition:** PR-A MUST be merged to `master` and `master` MUST be passing
> `cargo test --workspace --features tf-cli/mcp,tf-cli/dashboard` **before** PR-B's rebase begins.
>
> This is a *process gate*, enforced via `.foundry/governance.md` `pr-approval` (the agent never
> self-merges). It is recorded here for traceability of SMU §3 blocker #5; it does not generate
> Gherkin scenarios in STEP 2.
