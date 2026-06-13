# EARS Specification — token-fairness

**Edition:** 1.0  
**Created:** 2026-06-13  
**Last updated:** 2026-06-13  
**Status:** ACTIVE (Phase B: MCP Server)

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

## Non-MCP Statements (Inherited from Prior Phases)

**S-001 through S-100:** [Omitted — see ROADMAP.md AC#1–10, prior iteration commits for pre-MCP requirements]

---

## Acceptance Criteria Traceability

| AC # | Requirement | EARS Statement(s) |
|------|-----------|----------|
| 1 | `tf mcp` starts without error | MCP-011, Feature-001 |
| 2 | `tf_gate` returns {verdict, reason, ceiling} | MCP-001, Test-MCP-001 |
| 3 | `tf_budget_read`/`tf_budget_set` work, state persists | MCP-002, MCP-003 |
| 4 | `tf_report`, `tf_observe`, `tf_spend` return JSON | MCP-004, MCP-005, MCP-006 |
| 5 | (Dashboard, Phase C) | (Out of scope Phase B) |
| 6 | (WebSocket, Phase C) | (Out of scope Phase B) |
| 7 | (Prometheus, Phase C) | (Out of scope Phase B) |
| 8 | Binary size ≤105% of pre-change | US-002, Feature-001 |
| 9 | (Fold parity, Phase C) | (Out of scope Phase B) |
| 10 | `tf --help` lists `mcp` | US-001 (new verb visible) |

---

## Revision History

| Date | Editor | Change |
|------|--------|--------|
| 2026-06-13 | Handler | Initial: MCP-001 through MCP-010, Resource-001–003, Feature gates, error handling, test contracts |
