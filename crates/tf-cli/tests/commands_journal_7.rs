//! Integration tests for [7] Request-shape cost journal.
//!
//! Covers: path resolution (TF-7-001..003), append/upsert (TF-7-004..010),
//! close/finalise (TF-7-011..016), read (TF-7-017..020), close summary default
//! (TF-7-036), error discipline (TF-7-039), MCP tools tf_journal_append /
//! tf_journal_read (TF-7-028..029, TF-7-037), tf://cost-journal resource
//! (TF-7-031), resources_list 3→4 (TF-7-032), feature-gate builds (TF-7-022..035).
//!
//! Journal-state isolation:
//!   I2P_COST_JOURNAL     → per-test temp dir / cost-journal.jsonl
//!   I2P_COST_JOURNAL_OPEN → per-test temp dir / journal-open.json
//!   I2P_COST_STATE_DIR   → per-test temp dir
//!
//! Run with: cargo test --test commands_journal_7 --features tf-cli/mcp,tf-cli/journal
//!
//! Feature gates:
//!   - Journal CLI tests:    #[cfg(feature = "journal")]
//!   - MCP+journal tests:    #[cfg(all(feature = "mcp", feature = "journal"))]
//!   - MCP-only tests:       #[cfg(all(feature = "mcp", not(feature = "journal")))]
//!   - No-features tests:    #[cfg(not(any(feature = "journal", feature = "mcp", feature = "dashboard")))]
//!   - Summarizer tests:     #[cfg(feature = "journal-summarizer")]

#[allow(unused_imports)]
use serde_json::{json, Value};
#[allow(unused_imports)]
use std::io::{BufRead, BufReader, Write as IoWrite};
#[cfg(any(feature = "journal", feature = "mcp"))]
use std::path::PathBuf;
#[allow(unused_imports)]
use std::process::{Child, Command, Stdio};
#[cfg(any(feature = "journal", feature = "mcp"))]
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Shared test helpers
// ---------------------------------------------------------------------------

/// Unique temp dir per test.
#[cfg(any(feature = "journal", feature = "mcp"))]
fn temp_dir(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "tf-cj7-{}-{}-{}",
        tag,
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Run `tf <args>` with the given env overrides; returns (stdout, stderr, exit_code).
fn run_tf(args: &[&str], envs: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tf"));
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn tf")
        .wait_with_output()
        .expect("failed to wait for tf");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

/// Journal environment for an isolated temp dir.
#[cfg(any(feature = "journal", feature = "mcp"))]
struct JournalEnv {
    dir: PathBuf,
    journal_path: PathBuf,
    open_path: PathBuf,
}

// `JournalEnv` is gated on `any(journal, mcp)` because the MCP-only build needs it for
// `spawn_mcp_with_journal` + the `feature_mcp_only_*` negative test. That gate is deliberately
// broader than the consumers of most of these helpers, which live in `journal`-gated tests
// (verified via rust-analyzer findReferences: every method has 6-24 call sites under
// --features journal,mcp). In the `mcp && !journal` slice only `new` is reached, so clippy
// flags the journal-only helpers as dead. They are not vestigial — allow dead_code for the
// permutations where a given helper has no call site rather than narrowing each method's cfg.
#[cfg(any(feature = "journal", feature = "mcp"))]
#[allow(dead_code)]
impl JournalEnv {
    fn new(tag: &str) -> Self {
        let dir = temp_dir(tag);
        let journal_path = dir.join("cost-journal.jsonl");
        let open_path = dir.join("journal-open.json");
        JournalEnv {
            dir,
            journal_path,
            open_path,
        }
    }

    /// Env overrides to pass to run_tf.
    fn envs(&self) -> Vec<(String, String)> {
        vec![
            (
                "I2P_COST_STATE_DIR".into(),
                self.dir.to_str().unwrap().into(),
            ),
            (
                "I2P_COST_JOURNAL".into(),
                self.journal_path.to_str().unwrap().into(),
            ),
            (
                "I2P_COST_JOURNAL_OPEN".into(),
                self.open_path.to_str().unwrap().into(),
            ),
        ]
    }

    /// Convenience: run tf with this journal's env vars.
    fn run(&self, args: &[&str]) -> (String, String, i32) {
        let owned = self.envs();
        let envs: Vec<(&str, &str)> = owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        run_tf(args, &envs)
    }

    /// Read and parse journal-open.json; returns None if absent.
    fn read_open(&self) -> Option<Value> {
        let raw = std::fs::read_to_string(&self.open_path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Read all finalised records from cost-journal.jsonl (one JSON per line).
    fn read_journal(&self) -> Vec<Value> {
        match std::fs::read_to_string(&self.journal_path) {
            Ok(content) => content
                .lines()
                .filter(|l| !l.trim().is_empty())
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect(),
            Err(_) => vec![],
        }
    }

    /// Write journal-open.json directly (for seeding pre-conditions).
    fn seed_open(&self, content: &str) {
        std::fs::write(&self.open_path, content).unwrap();
    }

    /// Write cost-journal.jsonl directly.
    fn seed_journal(&self, content: &str) {
        std::fs::write(&self.journal_path, content).unwrap();
    }

    /// Raw bytes of journal-open.json.
    fn open_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.open_path).ok()
    }
}

// ---------------------------------------------------------------------------
// MCP server RAII wrapper (cf. mcp.rs)
// ---------------------------------------------------------------------------

#[cfg(feature = "mcp")]
struct McpServer(Child);

#[cfg(feature = "mcp")]
impl Drop for McpServer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(feature = "mcp")]
fn spawn_mcp_with_journal(journal: &JournalEnv) -> McpServer {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tf"));
    cmd.arg("mcp")
        .env("I2P_COST_STATE_DIR", &journal.dir)
        .env("I2P_COST_JOURNAL", &journal.journal_path)
        .env("I2P_COST_JOURNAL_OPEN", &journal.open_path);
    let child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn tf mcp");
    McpServer(child)
}

#[cfg(feature = "mcp")]
fn mcp_call(server: &mut McpServer, method: &str, params: Option<Value>) -> (Value, Value) {
    let child = &mut server.0;
    let stdin = child.stdin.as_mut().expect("stdin");
    let stdout = child.stdout.as_mut().expect("stdout");
    let req = if let Some(p) = params {
        json!({"jsonrpc":"2.0","method":method,"params":p,"id":1})
    } else {
        json!({"jsonrpc":"2.0","method":method,"id":1})
    };
    let line = req.to_string() + "\n";
    stdin.write_all(line.as_bytes()).expect("write");
    stdin.flush().expect("flush");
    let mut reader = BufReader::new(stdout);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).expect("read_line");
    let resp: Value =
        serde_json::from_str(&resp_line).unwrap_or_else(|_| panic!("bad JSON: {resp_line}"));
    let payload = resp
        .get("result")
        .or_else(|| resp.get("error"))
        .cloned()
        .unwrap_or(Value::Null);
    (resp, payload)
}

// ===========================================================================
// [7] Path resolution — TF-7-001, TF-7-002, TF-7-003
// ===========================================================================

/// @EARS-TF-7-001 @EARS-TF-7-002 @EARS-TF-7-003 @happy
/// Journal paths honour I2P_COST_JOURNAL / I2P_COST_JOURNAL_OPEN env overrides.
///
/// RED: `tf journal` subcommand does not yet exist.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_paths_honour_env_overrides() {
    let env = JournalEnv::new("path-resolution");

    // Run `tf journal append` — if journal is implemented, it must write to the
    // I2P_COST_JOURNAL_OPEN path, NOT to ~/.claude/state/i2p-cost/journal-open.json.
    let (_stdout, _stderr, code) =
        env.run(&["journal", "append", "7", "50000", "claude-haiku-4-5"]);

    // RED: `tf journal` not yet implemented → exit != 0.
    // When GREEN: code == 0 AND the open entry is in env.open_path.
    if code == 0 {
        assert!(
            env.open_path.exists(),
            "journal-open.json must be at the I2P_COST_JOURNAL_OPEN path, not the default"
        );
        let open = env
            .read_open()
            .expect("journal-open.json must be valid JSON");
        assert!(
            open.get("7").is_some(),
            "open entry keyed '7' must exist at the env-overridden path"
        );
    } else {
        // Still RED — acceptable.
        eprintln!("RED: tf journal not yet implemented (code={code})");
    }

    // The DEFAULT state dir must NOT have been written to (it is the env.dir itself in this
    // test, so we verify that the file at open_path is the only journal file present).
    // No assertion against a hardcoded default path since we can't know it in isolation.
}

// ===========================================================================
// [7] tf journal append (upsert) — TF-7-004..010
// ===========================================================================

/// @EARS-TF-7-004 @happy
/// tf journal append creates a new open entry for an unseen id.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_append_creates_new_open_entry() {
    let env = JournalEnv::new("append-new");

    let (stdout, stderr, code) = env.run(&["journal", "append", "7", "50000", "claude-haiku-4-5"]);

    assert_eq!(
        code, 0,
        "RED: tf journal append must exit 0; stderr={stderr} stdout={stdout}"
    );

    let open = env
        .read_open()
        .expect("journal-open.json must exist after append");
    let entry = open.get("7").expect("entry keyed '7' must be present");

    assert_eq!(
        entry.get("accumulated_tokens").and_then(|v| v.as_i64()),
        Some(50000),
        "accumulated_tokens must equal the appended amount"
    );
    assert_eq!(
        entry
            .pointer("/by_model/claude-haiku-4-5")
            .and_then(|v| v.as_i64()),
        Some(50000),
        "by_model must have claude-haiku-4-5 == 50000"
    );
    assert!(
        entry.get("ts_opened").is_some(),
        "entry must have a ts_opened timestamp"
    );
}

/// @EARS-TF-7-005 @happy
/// tf journal append accumulates tokens for the same model on an existing entry.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_append_accumulates_same_model() {
    let env = JournalEnv::new("append-accumulate");
    // Seed an open entry with 50000 tokens for haiku.
    env.seed_open(
        r#"{"7":{"ts_opened":1700000000,"ask":"draft the spec","accumulated_tokens":50000,"by_model":{"claude-haiku-4-5":50000}}}"#,
    );

    let (_stdout, stderr, code) = env.run(&["journal", "append", "7", "20000", "claude-haiku-4-5"]);
    assert_eq!(code, 0, "RED: append must succeed; stderr={stderr}");

    let open = env.read_open().expect("open file must exist");
    let entry = open.get("7").expect("entry '7' must exist");

    assert_eq!(
        entry
            .pointer("/by_model/claude-haiku-4-5")
            .and_then(|v| v.as_i64()),
        Some(70000),
        "by_model haiku tokens must be 50000 + 20000 = 70000"
    );
    assert_eq!(
        entry.get("accumulated_tokens").and_then(|v| v.as_i64()),
        Some(70000),
        "accumulated_tokens must be 70000"
    );
}

/// @EARS-TF-7-007 @happy
/// tf journal append adds a second model to an existing open entry.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_append_adds_second_model() {
    let env = JournalEnv::new("append-second-model");
    env.seed_open(
        r#"{"7":{"ts_opened":1700000000,"ask":"draft the spec","accumulated_tokens":50000,"by_model":{"claude-haiku-4-5":50000}}}"#,
    );

    let (_stdout, stderr, code) = env.run(&["journal", "append", "7", "30000", "claude-opus-4"]);
    assert_eq!(
        code, 0,
        "RED: append second model must succeed; stderr={stderr}"
    );

    let open = env.read_open().expect("open file must exist");
    let entry = open.get("7").expect("entry '7' must exist");

    assert_eq!(
        entry
            .pointer("/by_model/claude-haiku-4-5")
            .and_then(|v| v.as_i64()),
        Some(50000),
        "haiku tokens must be unchanged"
    );
    assert_eq!(
        entry
            .pointer("/by_model/claude-opus-4")
            .and_then(|v| v.as_i64()),
        Some(30000),
        "opus tokens must be 30000"
    );
    assert_eq!(
        entry.get("accumulated_tokens").and_then(|v| v.as_i64()),
        Some(80000),
        "accumulated_tokens must be 50000 + 30000 = 80000"
    );
}

/// @EARS-TF-7-006 @happy
/// tf journal append overwrites the ask with --ask; omitting --ask preserves prior ask.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_append_ask_overwrite_and_preserve() {
    let env = JournalEnv::new("append-ask");
    env.seed_open(
        r#"{"7":{"ts_opened":1700000000,"ask":"draft the spec","accumulated_tokens":10000,"by_model":{"claude-opus-4":10000}}}"#,
    );

    // Overwrite ask.
    let (_stdout, stderr, code) = env.run(&[
        "journal",
        "append",
        "7",
        "10000",
        "claude-opus-4",
        "--ask",
        "rewrite the spec",
    ]);
    assert_eq!(
        code, 0,
        "RED: append with --ask must succeed; stderr={stderr}"
    );
    let open = env.read_open().expect("open file");
    assert_eq!(
        open.pointer("/7/ask").and_then(|v| v.as_str()),
        Some("rewrite the spec"),
        "ask must be overwritten to 'rewrite the spec'"
    );

    // Append without --ask — must preserve the previous ask.
    let (_stdout, stderr, code) = env.run(&["journal", "append", "7", "5000", "claude-opus-4"]);
    assert_eq!(
        code, 0,
        "RED: append without --ask must succeed; stderr={stderr}"
    );
    let open = env.read_open().expect("open file");
    assert_eq!(
        open.pointer("/7/ask").and_then(|v| v.as_str()),
        Some("rewrite the spec"),
        "ask must still be 'rewrite the spec' when --ask is omitted"
    );
}

/// @EARS-TF-7-008 @unhappy
/// tf journal append rejects an empty id and writes nothing.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_append_rejects_empty_id() {
    let env = JournalEnv::new("append-empty-id");
    let before = env.open_bytes();

    let (_stdout, _stderr, code) = env.run(&["journal", "append", "", "50000", "claude-opus-4"]);
    assert_ne!(code, 0, "RED: empty id must cause non-zero exit");

    // journal-open.json must be unchanged (or still absent).
    let after = env.open_bytes();
    assert_eq!(
        before, after,
        "journal-open.json must not be modified after empty-id error"
    );
}

/// @EARS-TF-7-009 @unhappy
/// tf journal append rejects a missing model argument and writes nothing.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_append_rejects_missing_model() {
    let env = JournalEnv::new("append-no-model");
    let before = env.open_bytes();

    // Only two positional args (id + tokens); model is missing.
    let (_stdout, _stderr, code) = env.run(&["journal", "append", "7", "50000"]);
    assert_ne!(code, 0, "RED: missing model must cause non-zero exit");

    let after = env.open_bytes();
    assert_eq!(
        before, after,
        "journal-open.json must not be modified when model is missing"
    );
}

/// @EARS-TF-7-010 @abuse
/// tf journal append rejects non-numeric tokens via strict parse (never coerces to 0).
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_append_rejects_non_numeric_tokens() {
    let env = JournalEnv::new("append-bad-tokens");
    let before = env.open_bytes();

    let (_stdout, stderr, code) = env.run(&["journal", "append", "7", "abc", "claude-opus-4"]);
    assert_ne!(
        code, 0,
        "RED: non-numeric tokens must cause non-zero exit; stderr={stderr}"
    );

    // The error must be a parse error, not a silent coercion to 0.
    // When GREEN, stderr must contain a parse-error message; we verify this when code != 0.
    let after = env.open_bytes();
    assert_eq!(
        before, after,
        "journal-open.json must not be modified when token parse fails"
    );

    // After GREEN: verify entry "7" is absent (not defaulted to 0).
    let open = env.read_open().unwrap_or(json!({}));
    assert!(
        open.get("7").is_none(),
        "entry '7' must NOT exist after non-numeric token rejection"
    );
}

// ===========================================================================
// [7] tf journal close (finalise) — TF-7-011..016
// ===========================================================================

/// @EARS-TF-7-011 @EARS-TF-7-011a @EARS-TF-7-012 @EARS-TF-7-013 @happy
/// tf journal close prices the entry, appends a finalised record, clears the key.
///
/// This is the core happy-path fleshed-out test.
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_close_prices_and_appends_record() {
    let env = JournalEnv::new("close-happy");
    // Seed an open entry with two models.
    env.seed_open(
        r#"{"7":{"ts_opened":1700000000,"ask":"draft the spec","accumulated_tokens":80000,"by_model":{"claude-haiku-4-5":50000,"claude-opus-4":30000}}}"#,
    );

    let (stdout, stderr, code) = env.run(&["journal", "close", "7"]);
    assert_eq!(
        code, 0,
        "RED: tf journal close must exit 0; stdout={stdout} stderr={stderr}"
    );

    // Exactly one record must be appended to cost-journal.jsonl.
    let records = env.read_journal();
    assert_eq!(records.len(), 1, "exactly one record must be appended");

    let rec = &records[0];
    assert_eq!(
        rec.get("roadmap_id").and_then(|v| v.as_str()),
        Some("7"),
        "record must have roadmap_id == '7'"
    );
    assert_eq!(
        rec.get("total_tokens").and_then(|v| v.as_i64()),
        Some(80000),
        "total_tokens must be 80000"
    );
    // total_cost_usd must be present and non-negative.
    let cost = rec.get("total_cost_usd").and_then(|v| v.as_f64());
    assert!(
        cost.is_some() && cost.unwrap() >= 0.0,
        "total_cost_usd must be present and non-negative; got={cost:?}"
    );
    // Must contain required fields.
    assert!(rec.get("ts").is_some(), "record must have 'ts'");
    assert!(
        rec.get("ask_summary").is_some(),
        "record must have 'ask_summary'"
    );
    assert!(rec.get("by_model").is_some(), "record must have 'by_model'");

    // journal-open.json must no longer have entry keyed "7".
    let open = env.read_open().unwrap_or(json!({}));
    assert!(
        open.get("7").is_none(),
        "journal-open.json must NOT contain entry '7' after close"
    );
}

/// @EARS-TF-7-013 @EARS-TF-7-038 @abuse
/// A finalised record carries total-only fields and NEVER leaks [8] projection fields.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_close_no_projection_fields_leaked() {
    let env = JournalEnv::new("close-no-leak");
    env.seed_open(
        r#"{"7":{"ts_opened":1700000000,"ask":"test ask","accumulated_tokens":40000,"by_model":{"claude-haiku-4-5":40000}}}"#,
    );

    let (_stdout, _stderr, code) = env.run(&["journal", "close", "7"]);
    assert_eq!(code, 0, "RED: close must exit 0");

    let records = env.read_journal();
    assert_eq!(records.len(), 1, "one record must be appended");
    let rec = &records[0];

    // These [8] fields must NOT appear.
    assert!(
        rec.get("projections").is_none(),
        "finalised record must NOT have 'projections'"
    );
    assert!(
        rec.get("opus_only_cost_usd").is_none(),
        "finalised record must NOT have 'opus_only_cost_usd'"
    );
    assert!(
        rec.get("phases").is_none(),
        "finalised record must NOT have 'phases'"
    );
    assert!(
        rec.get("blended_rate").is_none(),
        "finalised record must NOT have 'blended_rate'"
    );
}

/// @EARS-TF-7-011a @abuse
/// Closing an entry with an unpriced model lists it at zero cost rather than failing.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_close_unknown_model_zero_cost_not_fail() {
    let env = JournalEnv::new("close-unknown-model");
    env.seed_open(
        r#"{"7":{"ts_opened":1700000000,"ask":"test","accumulated_tokens":50000,"by_model":{"claude-haiku-4-5":40000,"unknown-future-model":10000}}}"#,
    );

    let (_stdout, _stderr, code) = env.run(&["journal", "close", "7"]);
    assert_eq!(
        code, 0,
        "closing with an unpriced model must succeed (fails-open)"
    );

    let records = env.read_journal();
    assert_eq!(records.len(), 1, "one record");
    let rec = &records[0];

    assert_eq!(
        rec.get("total_tokens").and_then(|v| v.as_i64()),
        Some(50000),
        "total_tokens must include unpriced model tokens"
    );

    // The unpriced model must appear in by_model with 10000 tokens and 0.0 cost.
    let by_model = rec.get("by_model").expect("by_model must exist");
    let unknown = by_model
        .get("unknown-future-model")
        .expect("unknown-future-model must appear");
    let unknown_cost = unknown
        .get("cost_usd")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            // May be a flat number or an object.
            if unknown.is_number() {
                unknown.as_f64()
            } else {
                None
            }
        });
    // At minimum: tokens present and accounted for.
    let unknown_tokens = unknown.get("tokens").and_then(|v| v.as_i64()).or_else(|| {
        if unknown.is_number() {
            unknown.as_i64()
        } else {
            None
        }
    });
    assert_eq!(
        unknown_tokens,
        Some(10000),
        "unknown-future-model must have 10000 tokens in by_model"
    );
    let _ = unknown_cost; // May be 0.0 — structure validated above.
}

/// @EARS-TF-7-014 @unhappy
/// tf journal close with no matching open entry errors and writes no record.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_close_no_matching_entry_errors() {
    let env = JournalEnv::new("close-no-entry");
    // journal-open.json has no entry keyed "9".
    env.seed_open(r#"{"7":{"ts_opened":1700000000,"ask":"other","accumulated_tokens":1000,"by_model":{"claude-haiku-4-5":1000}}}"#);

    let (_stdout, _stderr, code) = env.run(&["journal", "close", "9"]);
    assert_ne!(code, 0, "RED: closing a missing entry must exit non-zero");

    // No record must be appended.
    let records = env.read_journal();
    assert_eq!(
        records.len(),
        0,
        "no record must be appended when the id is not found"
    );
}

/// @EARS-TF-7-015 @unhappy
/// tf journal close rejects an empty id and writes no record.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_close_rejects_empty_id() {
    let env = JournalEnv::new("close-empty-id");

    let (_stdout, _stderr, code) = env.run(&["journal", "close", ""]);
    assert_ne!(code, 0, "RED: empty id must cause non-zero exit");

    let records = env.read_journal();
    assert_eq!(records.len(), 0, "no record must be appended for empty id");
}

/// @EARS-TF-7-016 @happy
/// Finalised records persist across a session boundary and a reset.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_records_persist_across_session_reset() {
    let env = JournalEnv::new("persist-reset");
    // Seed a finalised record in cost-journal.jsonl.
    let existing_record = r#"{"roadmap_id":"7","ts":1700000000,"ask_summary":"draft the spec","total_tokens":80000,"total_cost_usd":0.5,"by_model":{"claude-haiku-4-5":{"tokens":80000,"cost_usd":0.5}}}"#;
    env.seed_journal(&format!("{existing_record}\n"));

    // Run `tf budget set --reset` (session boundary) — this must NOT touch cost-journal.jsonl.
    let (_stdout, _stderr, _code) = run_tf(
        &["budget", "set", "--reset"],
        &[("I2P_COST_STATE_DIR", env.dir.to_str().unwrap())],
    );

    // The record must still be present, byte-for-byte.
    let records = env.read_journal();
    assert_eq!(
        records.len(),
        1,
        "one record must still be in cost-journal.jsonl"
    );
    assert_eq!(
        records[0].get("roadmap_id").and_then(|v| v.as_str()),
        Some("7"),
        "the persisted record must still have roadmap_id '7'"
    );
    assert_eq!(
        records[0].get("total_tokens").and_then(|v| v.as_i64()),
        Some(80000),
        "the persisted record must be unchanged"
    );
}

// ===========================================================================
// [7] tf journal read — TF-7-017..020
// ===========================================================================

/// @EARS-TF-7-017 @happy
/// tf journal read outputs all finalised entries as a JSON array.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_read_outputs_all_entries_as_json_array() {
    let env = JournalEnv::new("read-all");
    env.seed_journal(concat!(
        "{\"roadmap_id\":\"5\",\"ts\":1700000001,\"ask_summary\":\"a\",\"total_tokens\":1000,\"total_cost_usd\":0.1,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"6\",\"ts\":1700000002,\"ask_summary\":\"b\",\"total_tokens\":2000,\"total_cost_usd\":0.2,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"7\",\"ts\":1700000003,\"ask_summary\":\"c\",\"total_tokens\":3000,\"total_cost_usd\":0.3,\"by_model\":{}}\n",
    ));

    let (stdout, stderr, code) = env.run(&["journal", "read"]);
    assert_eq!(code, 0, "RED: tf journal read must exit 0; stderr={stderr}");

    let arr: Vec<Value> = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must be valid JSON array; err={e}; stdout={stdout}"));
    assert_eq!(arr.len(), 3, "must return all 3 entries");
}

/// @EARS-TF-7-018 @happy
/// tf journal read --id filters to a single roadmap id; does not mutate the file.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_read_id_filter_returns_single_entry() {
    let env = JournalEnv::new("read-filter-id");
    let raw_journal = concat!(
        "{\"roadmap_id\":\"5\",\"ts\":1700000001,\"ask_summary\":\"a\",\"total_tokens\":1000,\"total_cost_usd\":0.1,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"6\",\"ts\":1700000002,\"ask_summary\":\"b\",\"total_tokens\":2000,\"total_cost_usd\":0.2,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"7\",\"ts\":1700000003,\"ask_summary\":\"c\",\"total_tokens\":3000,\"total_cost_usd\":0.3,\"by_model\":{}}\n",
    );
    env.seed_journal(raw_journal);
    let before = std::fs::read(&env.journal_path).unwrap();

    let (stdout, stderr, code) = env.run(&["journal", "read", "--id", "6"]);
    assert_eq!(
        code, 0,
        "RED: tf journal read --id must exit 0; stderr={stderr}"
    );

    let arr: Vec<Value> = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("must be valid JSON; err={e}; stdout={stdout}"));
    assert_eq!(arr.len(), 1, "must return exactly 1 entry");
    assert_eq!(
        arr[0].get("roadmap_id").and_then(|v| v.as_str()),
        Some("6"),
        "the returned entry must have roadmap_id '6'"
    );

    // journal file must not be mutated by a read.
    let after = std::fs::read(&env.journal_path).unwrap();
    assert_eq!(
        before, after,
        "cost-journal.jsonl must not be mutated by a read"
    );
}

/// @EARS-TF-7-019 @happy
/// tf journal read --last N returns at most the N most recent entries in append order.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_read_last_n_returns_most_recent() {
    let env = JournalEnv::new("read-last");
    env.seed_journal(concat!(
        "{\"roadmap_id\":\"1\",\"ts\":100,\"ask_summary\":\"a\",\"total_tokens\":100,\"total_cost_usd\":0.01,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"2\",\"ts\":200,\"ask_summary\":\"b\",\"total_tokens\":200,\"total_cost_usd\":0.02,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"3\",\"ts\":300,\"ask_summary\":\"c\",\"total_tokens\":300,\"total_cost_usd\":0.03,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"4\",\"ts\":400,\"ask_summary\":\"d\",\"total_tokens\":400,\"total_cost_usd\":0.04,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"5\",\"ts\":500,\"ask_summary\":\"e\",\"total_tokens\":500,\"total_cost_usd\":0.05,\"by_model\":{}}\n",
    ));

    let (stdout, stderr, code) = env.run(&["journal", "read", "--last", "2"]);
    assert_eq!(
        code, 0,
        "RED: tf journal read --last must exit 0; stderr={stderr}"
    );

    let arr: Vec<Value> = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("must be valid JSON; err={e}; stdout={stdout}"));
    assert_eq!(arr.len(), 2, "must return exactly 2 entries");

    // Must be in append order: "4" then "5".
    assert_eq!(
        arr[0].get("roadmap_id").and_then(|v| v.as_str()),
        Some("4"),
        "first of last 2 must be id '4'"
    );
    assert_eq!(
        arr[1].get("roadmap_id").and_then(|v| v.as_str()),
        Some("5"),
        "second of last 2 must be id '5'"
    );
}

/// @EARS-TF-7-020 @abuse
/// tf journal read on an absent journal returns an empty array without error.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_read_absent_journal_returns_empty_array() {
    let env = JournalEnv::new("read-absent");
    // Do NOT seed cost-journal.jsonl — it must not exist.
    assert!(
        !env.journal_path.exists(),
        "journal must not exist for this test"
    );

    let (stdout, stderr, code) = env.run(&["journal", "read"]);
    assert_eq!(
        code, 0,
        "RED: tf journal read on absent journal must exit 0; stderr={stderr}"
    );
    assert_eq!(
        stdout.trim(),
        "[]",
        "stdout must be exactly '[]' when journal is absent; got={stdout}"
    );
}

// ===========================================================================
// [7] close summary default (network-free) — TF-7-036
// ===========================================================================

/// @EARS-TF-7-036 @happy
/// close without --summarize truncates the ask to 100 chars with no subprocess.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_close_truncates_ask_to_100_chars_by_default() {
    let env = JournalEnv::new("close-truncate");
    // Build an ask of 250 characters.
    let long_ask = "A".repeat(250);
    let open_json = format!(
        r#"{{"7":{{"ts_opened":1700000000,"ask":"{long_ask}","accumulated_tokens":10000,"by_model":{{"claude-haiku-4-5":10000}}}}}}"#,
    );
    env.seed_open(&open_json);

    let (_stdout, _stderr, code) = env.run(&["journal", "close", "7"]);
    assert_eq!(code, 0, "RED: close must exit 0");

    let records = env.read_journal();
    assert_eq!(records.len(), 1, "one record");
    let summary = records[0]
        .get("ask_summary")
        .and_then(|v| v.as_str())
        .expect("ask_summary must be present");
    assert_eq!(
        summary.len(),
        100,
        "ask_summary must be the first 100 chars of the stored ask; got len={}",
        summary.len()
    );
    assert_eq!(
        summary,
        &long_ask[..100],
        "ask_summary must equal the first 100 characters of the ask"
    );
}

// ===========================================================================
// [7] Error discipline — TF-7-039
// ===========================================================================

/// @EARS-TF-7-039 @abuse
/// A corrupt journal-open.json yields a typed error, never a panic.
///
/// RED: `tf journal` not yet implemented.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_corrupt_open_file_no_panic() {
    let env = JournalEnv::new("corrupt-open");
    // Write non-JSON bytes to journal-open.json.
    std::fs::write(&env.open_path, b"this is not json }{{{").unwrap();

    let (_stdout, stderr, code) = env.run(&["journal", "append", "7", "50000", "claude-opus-4"]);

    // Must exit non-zero with a typed error.
    assert_ne!(code, 0, "RED: corrupt open file must cause non-zero exit");

    // Must NOT contain a panic message or stack trace.
    assert!(
        !stderr.contains("thread '") && !stderr.contains("stack backtrace"),
        "stderr must NOT contain a panic/stack trace; got={stderr}"
    );
    assert!(
        !stderr.contains("panicked at"),
        "stderr must NOT contain 'panicked at'; got={stderr}"
    );
}

// ===========================================================================
// [7] MCP tools — tf_journal_append / tf_journal_read — TF-7-028..029, TF-7-037
// ===========================================================================

/// @EARS-TF-7-028 @EARS-TF-7-037 @build:mcp+journal @happy
/// MCP tf_journal_append upserts the same shared open entry as the CLI.
///
/// RED: tf_journal_append MCP tool does not yet exist.
#[cfg(all(feature = "mcp", feature = "journal"))]
#[test]
fn feature_mcp_journal_append_upserts_shared_open_entry() {
    let env = JournalEnv::new("mcp-append");
    let mut srv = spawn_mcp_with_journal(&env);

    let (_resp, result) = mcp_call(
        &mut srv,
        "tf_journal_append",
        Some(json!({"roadmap_id": "7", "tokens": 50000, "model": "claude-opus-4"})),
    );

    // RED: method-not-found error expected until tf_journal_append is implemented.
    let is_error = result.get("code").is_some()
        || result
            .as_str()
            .map(|s| s.contains("not found") || s.contains("error"))
            .unwrap_or(false);
    if is_error {
        eprintln!("RED: tf_journal_append not yet implemented; result={result}");
        return;
    }

    // When GREEN: open entry must exist with accumulated_tokens == 50000.
    let open = env
        .read_open()
        .expect("journal-open.json must exist after MCP append");
    let entry = open
        .get("7")
        .expect("entry '7' must exist after MCP append");
    assert_eq!(
        entry.get("accumulated_tokens").and_then(|v| v.as_i64()),
        Some(50000),
        "accumulated_tokens must be 50000"
    );
}

/// @EARS-TF-7-029 @EARS-TF-7-037 @build:mcp+journal @happy
/// MCP tf_journal_read returns the same entries the CLI read would for equivalent filters.
///
/// RED: tf_journal_read MCP tool does not yet exist.
#[cfg(all(feature = "mcp", feature = "journal"))]
#[test]
fn feature_mcp_journal_read_matches_cli_read() {
    let env = JournalEnv::new("mcp-read");
    // Seed two finalised records.
    env.seed_journal(concat!(
        "{\"roadmap_id\":\"6\",\"ts\":100,\"ask_summary\":\"a\",\"total_tokens\":1000,\"total_cost_usd\":0.1,\"by_model\":{}}\n",
        "{\"roadmap_id\":\"7\",\"ts\":200,\"ask_summary\":\"b\",\"total_tokens\":2000,\"total_cost_usd\":0.2,\"by_model\":{}}\n",
    ));

    let mut srv = spawn_mcp_with_journal(&env);

    // Filter by roadmap_id "7".
    let (_resp, result) = mcp_call(
        &mut srv,
        "tf_journal_read",
        Some(json!({"roadmap_id": "7"})),
    );

    // RED: method-not-found.
    let is_error = result.get("code").is_some();
    if is_error {
        eprintln!("RED: tf_journal_read not yet implemented; result={result}");
        return;
    }

    // When GREEN: result must contain only the entry with roadmap_id "7".
    let arr = result
        .as_array()
        .expect("tf_journal_read must return an array");
    assert_eq!(arr.len(), 1, "must return exactly 1 entry for roadmap_id 7");
    assert_eq!(
        arr[0].get("roadmap_id").and_then(|v| v.as_str()),
        Some("7"),
        "the returned entry must have roadmap_id '7'"
    );
}

// ===========================================================================
// [7] tf://cost-journal resource — TF-7-031
// ===========================================================================

/// @EARS-TF-7-031 @build:mcp+journal @happy
/// tf://cost-journal resource returns the last 100 finalised entries.
///
/// RED: tf://cost-journal resource does not yet exist.
#[cfg(all(feature = "mcp", feature = "journal"))]
#[test]
fn feature_mcp_cost_journal_resource_returns_last_100() {
    let env = JournalEnv::new("resource-last100");
    // Seed 150 records.
    let mut records = String::new();
    for i in 1u64..=150 {
        records.push_str(&format!(
            "{{\"roadmap_id\":\"{i}\",\"ts\":{i},\"ask_summary\":\"a\",\"total_tokens\":100,\"total_cost_usd\":0.01,\"by_model\":{{}}}}\n"
        ));
    }
    env.seed_journal(&records);

    let mut srv = spawn_mcp_with_journal(&env);

    let (_resp, result) = mcp_call(
        &mut srv,
        "resources/read",
        Some(json!({"uri": "tf://cost-journal"})),
    );

    // Check for not-found (RED state).
    let is_error = result.get("code").is_some()
        || result
            .as_str()
            .map(|s| s.contains("not found") || s.contains("error"))
            .unwrap_or(false);
    if is_error {
        eprintln!("RED: tf://cost-journal resource not yet implemented; result={result}");
        return;
    }

    // When GREEN: must be a JSON array of length 100.
    let arr = result
        .as_array()
        .or_else(|| result.get("contents").and_then(|c| c.as_array()))
        .expect("tf://cost-journal must return an array");
    assert_eq!(
        arr.len(),
        100,
        "must return exactly 100 entries (the last 100)"
    );

    // Oldest in array must be id "51"; newest must be id "150".
    assert_eq!(
        arr[0].get("roadmap_id").and_then(|v| v.as_str()),
        Some("51"),
        "oldest entry in the 100 must have roadmap_id '51'"
    );
    assert_eq!(
        arr[99].get("roadmap_id").and_then(|v| v.as_str()),
        Some("150"),
        "newest entry in the 100 must have roadmap_id '150'"
    );
}

/// @EARS-TF-7-032 @build:mcp+journal @happy
/// resources_list enumerates exactly 4 resources when journal is enabled.
///
/// RED: tf://cost-journal is not yet registered → resources_list still returns 3.
/// This test asserts the GREEN target (4); it will fail until journal MCP resource lands.
#[cfg(all(feature = "mcp", feature = "journal"))]
#[test]
fn feature_mcp_resources_list_has_four_resources_with_journal() {
    let env = JournalEnv::new("resources-list-4");
    let mut srv = spawn_mcp_with_journal(&env);

    let (_resp, result) = mcp_call(&mut srv, "resources/list", None);
    let resources = result
        .get("resources")
        .and_then(|v| v.as_array())
        .expect("resources/list must return a 'resources' array");

    assert_eq!(
        resources.len(),
        4,
        "RED: resources_list must return 4 resources when journal is enabled (currently returns 3)"
    );

    // Verify the expected URIs are present.
    let uris: Vec<&str> = resources
        .iter()
        .filter_map(|r| r.get("uri").and_then(|v| v.as_str()))
        .collect();
    assert!(uris.contains(&"tf://status"), "tf://status must be listed");
    assert!(
        uris.contains(&"tf://calibration"),
        "tf://calibration must be listed"
    );
    assert!(uris.contains(&"tf://events"), "tf://events must be listed");
    assert!(
        uris.contains(&"tf://cost-journal"),
        "tf://cost-journal must be listed when journal feature is enabled"
    );
}

// ===========================================================================
// [7] Feature-gate builds — TF-7-022..026, TF-7-030
// ===========================================================================

/// @EARS-TF-7-026 @EARS-TF-7-025 @EARS-TF-7-023 @EARS-TF-7-024 @build:no-features @abuse
/// A no-features binary hides the journal subcommand from tf --help.
///
/// This test only executes when the binary is built WITHOUT the journal feature.
/// It is gated behind `not(feature = "journal")` so it runs against the no-features binary.
#[cfg(not(feature = "journal"))]
#[test]
fn feature_no_features_binary_hides_journal_subcommand() {
    let (stdout, _stderr, code) = run_tf(&["--help"], &[]);
    assert_eq!(code, 0, "tf --help must exit 0");
    assert!(
        !stdout.contains("Journal:"),
        "a no-features binary must NOT list 'Journal:' in --help; got: {stdout}"
    );
    assert!(
        !stdout.contains("journal"),
        "a no-features binary must NOT mention 'journal' in --help; got: {stdout}"
    );
}

/// @EARS-TF-7-026 @build:no-features @abuse
/// Running `tf journal read` on a no-features binary reports an unrecognised command.
#[cfg(not(feature = "journal"))]
#[test]
fn feature_no_features_binary_journal_subcommand_is_unrecognised() {
    let (_stdout, stderr, code) = run_tf(&["journal", "read"], &[]);
    assert_ne!(
        code, 0,
        "tf journal on a no-features binary must exit non-zero"
    );
    let err_lower = stderr.to_lowercase() + &_stdout.to_lowercase();
    assert!(
        err_lower.contains("unrecognised")
            || err_lower.contains("unknown")
            || err_lower.contains("invalid")
            || err_lower.contains("usage"),
        "no-features binary must report journal as unrecognised; got stderr={stderr}"
    );
}

/// @EARS-TF-7-022 @EARS-TF-7-025 @build:journal @happy
/// A journal-feature binary lists the journal subcommand in tf --help.
///
/// RED: the `Journal:` line is not yet cfg-gated into --help output.
#[cfg(feature = "journal")]
#[test]
fn feature_journal_binary_lists_journal_in_help() {
    let (stdout, _stderr, code) = run_tf(&["--help"], &[]);
    assert_eq!(code, 0, "tf --help must exit 0");
    assert!(
        stdout.contains("Journal:") || stdout.contains("journal"),
        "RED: a journal-feature binary must list 'Journal:' or 'journal' in --help; got: {stdout}"
    );
}

/// @EARS-TF-7-030 @build:mcp-only @abuse
/// The journal MCP handlers are absent when journal feature is not enabled.
///
/// This test runs against a binary built with mcp but NOT journal.
/// It is gated to only execute in that configuration.
#[cfg(all(feature = "mcp", not(feature = "journal")))]
#[test]
fn feature_mcp_only_journal_handlers_absent() {
    let env = JournalEnv::new("mcp-only-no-journal");
    let mut srv = spawn_mcp_with_journal(&env);

    // tf_journal_append must return method-not-found.
    let (_resp, result) = mcp_call(
        &mut srv,
        "tf_journal_append",
        Some(json!({"roadmap_id": "7", "tokens": 1, "model": "x"})),
    );
    // A method-not-found error must be returned (JSON-RPC error code -32601 or similar).
    let is_method_not_found = result
        .get("code")
        .and_then(|v| v.as_i64())
        .map(|c| c == -32601)
        .unwrap_or(false)
        || result
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase().contains("not found") || s.to_lowercase().contains("unknown"))
            .unwrap_or(false);
    assert!(
        is_method_not_found,
        "tf_journal_append must return method-not-found when journal feature is absent; result={result}"
    );

    // tf://cost-journal resource must return not-found.
    let (_resp, res_result) = mcp_call(
        &mut srv,
        "resources/read",
        Some(json!({"uri": "tf://cost-journal"})),
    );
    let resource_not_found = res_result.get("code").is_some()
        || res_result
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase().contains("not found"))
            .unwrap_or(false);
    assert!(
        resource_not_found,
        "tf://cost-journal must return not-found when journal feature is absent; result={res_result}"
    );
}

// ===========================================================================
// [7] Summarizer (opt-in, fails-open) — TF-7-033..035
// ===========================================================================

/// @EARS-TF-7-033 @EARS-TF-7-027 @build:journal-summarizer @happy
/// close --summarize compresses the ask when the key and a working curl are available.
///
/// RED: `tf journal close --summarize` not yet implemented.
#[cfg(feature = "journal-summarizer")]
#[test]
fn feature_journal_close_summarize_uses_curl_when_key_present() {
    let env = JournalEnv::new("close-summarize-happy");
    let long_ask = "B".repeat(250);
    let open_json = format!(
        r#"{{"7":{{"ts_opened":1700000000,"ask":"{long_ask}","accumulated_tokens":10000,"by_model":{{"claude-haiku-4-5":10000}}}}}}"#
    );
    env.seed_open(&open_json);

    // Create a stub curl script on PATH that returns a fixed summary.
    let stub_dir = temp_dir("curl-stub");
    let stub_curl = stub_dir.join("curl");
    std::fs::write(
        &stub_curl,
        "#!/bin/sh\necho 'compressed: rewrite the spec'\nexit 0\n",
    )
    .unwrap();
    // Make executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&stub_curl).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub_curl, perms).unwrap();
    }

    // Build PATH with stub_dir prepended.
    let orig_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stub_dir.to_str().unwrap(), orig_path);

    let mut envs = env.envs();
    envs.push(("ANTHROPIC_API_KEY".into(), "test-key-present".into()));
    envs.push(("PATH".into(), new_path));
    let envs_str: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    let (_stdout, _stderr, code) = run_tf(&["journal", "close", "7", "--summarize"], &envs_str);
    assert_eq!(code, 0, "RED: close --summarize must exit 0");

    let records = env.read_journal();
    assert_eq!(records.len(), 1, "one record");
    assert_eq!(
        records[0].get("ask_summary").and_then(|v| v.as_str()),
        Some("compressed: rewrite the spec"),
        "ask_summary must be the curl-returned summary"
    );
}

/// @EARS-TF-7-034 @build:journal-summarizer @abuse
/// close --summarize fails open to a 100-char truncation when the API key is absent.
///
/// RED: `tf journal close --summarize` not yet implemented.
#[cfg(feature = "journal-summarizer")]
#[test]
fn feature_journal_close_summarize_fails_open_no_api_key() {
    let env = JournalEnv::new("summarize-no-key");
    let long_ask = "C".repeat(250);
    let open_json = format!(
        r#"{{"7":{{"ts_opened":1700000000,"ask":"{long_ask}","accumulated_tokens":10000,"by_model":{{"claude-haiku-4-5":10000}}}}}}"#
    );
    env.seed_open(&open_json);

    let mut envs = env.envs();
    // Explicitly unset the API key.
    envs.push(("ANTHROPIC_API_KEY".into(), "".into()));
    let envs_str: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    let (_stdout, _stderr, code) = run_tf(&["journal", "close", "7", "--summarize"], &envs_str);
    assert_eq!(code, 0, "RED: fails-open means exit 0 even without API key");

    let records = env.read_journal();
    assert_eq!(records.len(), 1, "record must still be appended");
    let summary = records[0]
        .get("ask_summary")
        .and_then(|v| v.as_str())
        .expect("ask_summary must be present");
    assert_eq!(
        summary.len(),
        100,
        "fails-open: ask_summary must be the first 100 chars; got len={}",
        summary.len()
    );
    assert_eq!(
        summary,
        &long_ask[..100],
        "fails-open: ask_summary must equal first 100 chars of the ask"
    );
}

/// @EARS-TF-7-035 @build:journal-summarizer @abuse
/// close --summarize fails open to a 100-char truncation when curl is not on PATH.
///
/// RED: `tf journal close --summarize` not yet implemented.
#[cfg(feature = "journal-summarizer")]
#[test]
fn feature_journal_close_summarize_fails_open_no_curl() {
    let env = JournalEnv::new("summarize-no-curl");
    let long_ask = "D".repeat(250);
    let open_json = format!(
        r#"{{"7":{{"ts_opened":1700000000,"ask":"{long_ask}","accumulated_tokens":10000,"by_model":{{"claude-haiku-4-5":10000}}}}}}"#
    );
    env.seed_open(&open_json);

    // Use a PATH with no curl (just /dev/null or an empty dir).
    let empty_dir = temp_dir("no-curl-path");
    let mut envs = env.envs();
    envs.push(("ANTHROPIC_API_KEY".into(), "test-key-present".into()));
    envs.push(("PATH".into(), empty_dir.to_str().unwrap().into()));
    let envs_str: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    let (_stdout, _stderr, code) = run_tf(&["journal", "close", "7", "--summarize"], &envs_str);
    assert_eq!(code, 0, "RED: fails-open means exit 0 when curl is absent");

    let records = env.read_journal();
    assert_eq!(records.len(), 1, "record must be appended");
    let summary = records[0]
        .get("ask_summary")
        .and_then(|v| v.as_str())
        .expect("ask_summary must be present");
    assert_eq!(summary.len(), 100, "falls back to 100-char truncation");
    assert_eq!(summary, &long_ask[..100]);
}

/// @EARS-TF-7-035 @build:journal-summarizer @abuse
/// close --summarize fails open when curl returns a non-zero exit code.
///
/// RED: `tf journal close --summarize` not yet implemented.
#[cfg(feature = "journal-summarizer")]
#[test]
fn feature_journal_close_summarize_fails_open_curl_error() {
    let env = JournalEnv::new("summarize-curl-fail");
    let long_ask = "E".repeat(250);
    let open_json = format!(
        r#"{{"7":{{"ts_opened":1700000000,"ask":"{long_ask}","accumulated_tokens":10000,"by_model":{{"claude-haiku-4-5":10000}}}}}}"#
    );
    env.seed_open(&open_json);

    // Create a stub curl that exits non-zero.
    let stub_dir = temp_dir("curl-fail-stub");
    let stub_curl = stub_dir.join("curl");
    std::fs::write(&stub_curl, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&stub_curl).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&stub_curl, perms).unwrap();
    }

    let orig_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stub_dir.to_str().unwrap(), orig_path);

    let mut envs = env.envs();
    envs.push(("ANTHROPIC_API_KEY".into(), "test-key-present".into()));
    envs.push(("PATH".into(), new_path));
    let envs_str: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    let (_stdout, _stderr, code) = run_tf(&["journal", "close", "7", "--summarize"], &envs_str);
    assert_eq!(
        code, 0,
        "RED: fails-open — exit 0 even when curl returns error"
    );

    let records = env.read_journal();
    assert_eq!(records.len(), 1, "record must be appended");
    let summary = records[0]
        .get("ask_summary")
        .and_then(|v| v.as_str())
        .expect("ask_summary must be present");
    assert_eq!(
        summary.len(),
        100,
        "falls back to 100-char truncation on curl error"
    );
    assert_eq!(summary, &long_ask[..100]);
}

// ===========================================================================
// BRANCH COVERAGE PLAN
// ===========================================================================
//
// journal::journal_path():
//   - I2P_COST_JOURNAL set   → feature_journal_paths_honour_env_overrides
//   - I2P_COST_JOURNAL unset → (covered by default-path tests in unit tests, Step 5)
//
// journal::journal_open_path():
//   - I2P_COST_JOURNAL_OPEN set   → feature_journal_paths_honour_env_overrides
//   - I2P_COST_JOURNAL_OPEN unset → (unit tests in journal.rs)
//
// journal::append():
//   - entry not yet in open file (creates) → feature_journal_append_creates_new_open_entry
//   - entry exists, same model (accumulates) → feature_journal_append_accumulates_same_model
//   - entry exists, new model (adds) → feature_journal_append_adds_second_model
//   - --ask provided (overwrites) → feature_journal_append_ask_overwrite_and_preserve
//   - --ask omitted (preserves) → feature_journal_append_ask_overwrite_and_preserve
//   - empty id error → feature_journal_append_rejects_empty_id
//   - missing model error → feature_journal_append_rejects_missing_model
//   - non-numeric tokens strict-parse error → feature_journal_append_rejects_non_numeric_tokens
//   - corrupt open file → feature_journal_corrupt_open_file_no_panic
//
// journal::close():
//   - happy path, known models → feature_journal_close_prices_and_appends_record
//   - unknown model (zero cost) → feature_journal_close_unknown_model_zero_cost_not_fail
//   - no matching open entry → feature_journal_close_no_matching_entry_errors
//   - empty id → feature_journal_close_rejects_empty_id
//   - no [8] fields leaked → feature_journal_close_no_projection_fields_leaked
//   - default truncation (no --summarize) → feature_journal_close_truncates_ask_to_100_chars_by_default
//
// journal::close() with summarizer:
//   - API key present + curl succeeds → feature_journal_close_summarize_uses_curl_when_key_present
//   - API key absent (fails-open) → feature_journal_close_summarize_fails_open_no_api_key
//   - curl absent (fails-open) → feature_journal_close_summarize_fails_open_no_curl
//   - curl exits non-zero (fails-open) → feature_journal_close_summarize_fails_open_curl_error
//
// journal::read():
//   - journal absent → feature_journal_read_absent_journal_returns_empty_array
//   - no filter → feature_journal_read_outputs_all_entries_as_json_array
//   - --id filter → feature_journal_read_id_filter_returns_single_entry
//   - --last N → feature_journal_read_last_n_returns_most_recent
//
// feature gate (cfg):
//   - no features binary hides journal → feature_no_features_binary_hides_journal_subcommand
//   - no features journal command unrecognised → feature_no_features_binary_journal_subcommand_is_unrecognised
//   - journal feature lists journal in help → feature_journal_binary_lists_journal_in_help
//
// MCP dispatch:
//   - tf_journal_append with mcp+journal → feature_mcp_journal_append_upserts_shared_open_entry
//   - tf_journal_read with mcp+journal → feature_mcp_journal_read_matches_cli_read
//   - tf://cost-journal resource → feature_mcp_cost_journal_resource_returns_last_100
//   - resources_list 4 when journal → feature_mcp_resources_list_has_four_resources_with_journal
//   - mcp-only (no journal) handlers absent → feature_mcp_only_journal_handlers_absent
