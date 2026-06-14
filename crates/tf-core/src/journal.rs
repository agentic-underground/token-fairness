//! journal — the request-shape COST JOURNAL ([7]).
//!
//! A two-file ledger that records the true token+dollar cost of a unit of work (a "roadmap id"),
//! kept entirely separate from the spend-safety gate so the hook binary is unaffected (the whole
//! module is `#[cfg(feature = "journal")]`).
//!
//!   - `journal-open.json` (`I2P_COST_JOURNAL_OPEN`) — the OPEN entries, keyed by roadmap id. Each
//!     `append` upserts an entry, accumulating tokens per model; the `ask` is overwrite-on-set,
//!     preserve-on-omit.
//!   - `cost-journal.jsonl` (`I2P_COST_JOURNAL`) — the FINALISED records, one JSON object per line.
//!     `close` prices the open entry (via [`crate::spend::price_by_model`], the same per-model rate
//!     table `tf spend` uses), appends one record, and removes the key from the open file.
//!
//! Records carry total-only fields (`total_tokens`/`total_cost_usd`/per-model breakdown). [8]
//! projection fields (opus-only cost, phase splits, blended rate) are deliberately NOT written here.
//!
//! Every path is fallible (typed errors, never a panic) and every state file honours its env
//! override so tests — and the production hook — stay isolated. The optional `--summarize` path
//! (behind `journal-summarizer`) compresses the ask via a `curl` subprocess and FAILS OPEN to the
//! 100-char truncation on any error.

use crate::{spend, state, Out};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// The finalised-records ledger path — `I2P_COST_JOURNAL` overrides the state-dir default.
fn journal_path() -> String {
    if let Ok(p) = std::env::var("I2P_COST_JOURNAL") {
        return p;
    }
    format!("{}/cost-journal.jsonl", state::state_dir())
}

/// The open-entries path — `I2P_COST_JOURNAL_OPEN` overrides the state-dir default.
fn open_path() -> String {
    if let Ok(p) = std::env::var("I2P_COST_JOURNAL_OPEN") {
        return p;
    }
    format!("{}/journal-open.json", state::state_dir())
}

/// Default summary cap: a finalised record's `ask_summary` is the first 100 chars of the ask.
const ASK_SUMMARY_CHARS: usize = 100;

/// Read the open-entries object, distinguishing ABSENT (returns an empty object) from CORRUPT
/// (returns a typed error). A corrupt file must never panic, and must never be silently treated as
/// empty (that would discard real open entries on the next write).
fn read_open() -> Result<Value, String> {
    let path = open_path();
    match std::fs::read_to_string(&path) {
        Ok(body) => serde_json::from_str::<Value>(&body)
            .map_err(|e| format!("journal: corrupt open file {}: {}", path, e)),
        // Absent file ⇒ no open entries yet.
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("journal: cannot read open file {}: {}", path, e)),
    }
}

/// Minimal flag reader over the journal sub-argv: `--key value` (space-separated). Returns the
/// first value following `--key`, or None. Presence-only flags use [`has_flag`].
fn flag_value<'a>(argv: &'a [String], key: &str) -> Option<&'a str> {
    let pfx = format!("--{}", key);
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == pfx {
            return argv.get(i + 1).map(|s| s.as_str());
        }
        i += 1;
    }
    None
}

/// True if a presence-only flag (`--summarize`) appears in the sub-argv.
fn has_flag(argv: &[String], key: &str) -> bool {
    let pfx = format!("--{}", key);
    argv.iter().any(|a| a == &pfx)
}

/// Positional args (everything that is not a flag and not a flag's value). The flags handled are
/// `--ask <v>` (takes a value) and `--summarize` (no value); `--id`/`--last` are read by `read`.
fn positionals(argv: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if let Some(name) = a.strip_prefix("--") {
            // value-taking flags consume the next token.
            if matches!(name, "ask" | "id" | "last") {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            out.push(a.as_str());
            i += 1;
        }
    }
    out
}

/// `tf journal append <id> <tokens> <model> [--ask <text>]` — upsert an open entry.
///
/// Creates the entry on first sight (stamping `ts_opened`), accumulates `accumulated_tokens` and
/// the per-model count, and sets/preserves the `ask`. Validation is strict and write-free on
/// failure: empty id, missing model, or non-numeric tokens all error WITHOUT touching the file.
fn append(argv: &[String]) -> Out {
    let pos = positionals(argv);
    let id = pos.first().copied().unwrap_or("");
    let tokens_raw = pos.get(1).copied().unwrap_or("");
    let model = pos.get(2).copied().unwrap_or("");

    if id.is_empty() {
        return Out::err("journal append: roadmap id must not be empty", 2);
    }
    if model.is_empty() {
        return Out::err("journal append: model argument is required", 2);
    }
    // Strict parse — never coerce a non-numeric string to 0 (that would corrupt the ledger).
    let tokens: i64 = match tokens_raw.parse() {
        Ok(n) => n,
        Err(_) => {
            return Out::err(
                format!(
                    "journal append: tokens must be an integer, got '{}'",
                    tokens_raw
                ),
                2,
            )
        }
    };

    let mut open = match read_open() {
        Ok(v) => v,
        Err(e) => return Out::err(e, 1),
    };
    let obj = match open.as_object_mut() {
        Some(o) => o,
        None => return Out::err("journal append: open file is not a JSON object", 1),
    };

    // Existing entry (upsert) or a fresh one stamped with ts_opened.
    let mut entry = obj.get(id).cloned().unwrap_or_else(|| {
        json!({
            "ts_opened": state::now_epoch(),
            "ask": "",
            "accumulated_tokens": 0,
            "by_model": {},
        })
    });

    let acc = state::int(&entry, "accumulated_tokens", 0) + tokens;
    let prev_model = entry
        .pointer(&format!("/by_model/{}", model))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    if let Some(eo) = entry.as_object_mut() {
        eo.insert("accumulated_tokens".into(), json!(acc));
        if let Some(bm) = eo.get_mut("by_model").and_then(|v| v.as_object_mut()) {
            bm.insert(model.to_string(), json!(prev_model + tokens));
        }
        // --ask overwrites; omitting it preserves the prior ask.
        if let Some(ask) = flag_value(argv, "ask") {
            eo.insert("ask".into(), json!(ask));
        }
    }

    obj.insert(id.to_string(), entry);
    if let Err(e) = state::write_json(&open_path(), &open) {
        return Out::err(format!("journal append: cannot write open file: {}", e), 1);
    }
    Out::ok(format!(
        "{{\"appended\":\"{}\",\"accumulated_tokens\":{}}}\n",
        id, acc
    ))
}

/// `tf journal close <id> [--summarize]` — price the open entry, append a finalised record, clear.
///
/// Errors (write-free) on an empty id or an id with no matching open entry. Pricing delegates to
/// [`crate::spend::price_by_model`]; an unpriced model is listed at $0.00, never a failure. The
/// `ask_summary` is the first 100 chars of the ask by default, or the summarizer's output when
/// `--summarize` is set and the summarizer feature is built (it fails open to the truncation).
fn close(argv: &[String]) -> Out {
    let pos = positionals(argv);
    let id = pos.first().copied().unwrap_or("");
    if id.is_empty() {
        return Out::err("journal close: roadmap id must not be empty", 2);
    }

    let mut open = match read_open() {
        Ok(v) => v,
        Err(e) => return Out::err(e, 1),
    };
    let entry = match open.get(id).cloned() {
        Some(e) => e,
        None => return Out::err(format!("journal close: no open entry for id '{}'", id), 2),
    };

    // Build the per-model token map (BTreeMap → sorted, deterministic record order).
    let mut by_model: BTreeMap<String, i64> = BTreeMap::new();
    if let Some(bm) = entry.get("by_model").and_then(|v| v.as_object()) {
        for (m, t) in bm {
            by_model.insert(m.clone(), t.as_i64().unwrap_or(0));
        }
    }
    let (per_model, total_tokens, total_cost) = spend::price_by_model(&by_model);

    // Per-model breakdown object: model → { tokens, cost_usd }.
    let mut by_model_out = serde_json::Map::new();
    for (model, tokens, cost) in per_model {
        by_model_out.insert(model, json!({ "tokens": tokens, "cost_usd": cost }));
    }

    let ask = entry.get("ask").and_then(|v| v.as_str()).unwrap_or("");
    let ask_summary = summarize(ask, has_flag(argv, "summarize"));

    let record = json!({
        "roadmap_id": id,
        "ts": state::now_epoch(),
        "ask_summary": ask_summary,
        "total_tokens": total_tokens,
        "total_cost_usd": total_cost,
        "by_model": Value::Object(by_model_out),
    });

    // Append the finalised record (compact, one line) BEFORE clearing the open key, so a write
    // failure leaves the open entry intact (no silent loss).
    if let Err(e) = state::append_line(&journal_path(), &record.to_string()) {
        return Out::err(format!("journal close: cannot append record: {}", e), 1);
    }
    if let Some(obj) = open.as_object_mut() {
        obj.remove(id);
    }
    if let Err(e) = state::write_json(&open_path(), &open) {
        return Out::err(format!("journal close: cannot rewrite open file: {}", e), 1);
    }

    Out::ok(format!(
        "{{\"closed\":\"{}\",\"total_tokens\":{},\"total_cost_usd\":{}}}\n",
        id, total_tokens, total_cost
    ))
}

/// Compress the ask to a record summary. The DEFAULT (and the fail-open fallback) is the first 100
/// characters of the ask. When `--summarize` is requested and the `journal-summarizer` feature is
/// built, [`summarize_via_curl`] may replace it; ANY failure there returns the truncation.
fn summarize(ask: &str, want_summarize: bool) -> String {
    if want_summarize {
        #[cfg(feature = "journal-summarizer")]
        if let Some(s) = summarize_via_curl(ask) {
            return s;
        }
    }
    let _ = want_summarize; // (no-op when the summarizer feature is absent)
    truncate_chars(ask, ASK_SUMMARY_CHARS)
}

/// First `n` CHARACTERS of `s` (char-boundary safe). The journal asks are ASCII in practice, but
/// slicing on chars (not bytes) keeps a multi-byte ask from panicking.
fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Opt-in ask summarizer (behind `journal-summarizer`): POST the ask to the Anthropic Messages API
/// via a `curl` subprocess, returning the compressed text. FAILS OPEN (returns None) when the API
/// key is absent/empty, `curl` is not on PATH, curl exits non-zero, or the output is empty — the
/// caller then uses the 100-char truncation. No panic, no error propagation: the summary is a
/// best-effort nicety, never a reason to fail a close.
#[cfg(feature = "journal-summarizer")]
fn summarize_via_curl(ask: &str) -> Option<String> {
    use std::process::Command;

    let key = std::env::var("ANTHROPIC_API_KEY").ok()?;
    if key.trim().is_empty() {
        return None;
    }

    let body = json!({
        "model": "claude-haiku-4-5",
        "max_tokens": 64,
        "messages": [{
            "role": "user",
            "content": format!("Summarize this work request in under 100 characters:\n\n{}", ask),
        }]
    })
    .to_string();

    let output = Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            "https://api.anthropic.com/v1/messages",
            "-H",
            &format!("x-api-key: {}", key),
            "-H",
            "anthropic-version: 2023-06-01",
            "-H",
            "content-type: application/json",
            "-d",
            &body,
        ])
        .output()
        .ok()?; // curl not on PATH ⇒ None ⇒ fail open.

    if !output.status.success() {
        return None; // curl error ⇒ fail open.
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The stub curl in tests echoes the summary directly; the real API returns a JSON envelope.
    // Try the envelope first, then fall back to the raw trimmed stdout.
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(text) = v
            .pointer("/content/0/text")
            .and_then(|t| t.as_str())
            .map(|s| s.trim().to_string())
        {
            if !text.is_empty() {
                return Some(text);
            }
        }
        // Parsed as JSON but no text field ⇒ fall open.
        return None;
    }
    Some(trimmed.to_string())
}

/// Read all finalised records (one JSON per line); absent file ⇒ empty vec.
fn read_records() -> Vec<Value> {
    match std::fs::read_to_string(journal_path()) {
        Ok(body) => body
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// `tf journal read [--id <id>] [--last <n>]` — emit finalised records as a JSON array.
///
/// `--id` filters to a single roadmap id; `--last N` keeps the N most recent (in append order).
/// An absent journal yields `[]` (exit 0) — a missing ledger is empty, not an error. A read NEVER
/// mutates the file.
fn read(argv: &[String]) -> Out {
    let mut records = read_records();

    if let Some(id) = flag_value(argv, "id") {
        records.retain(|r| r.get("roadmap_id").and_then(|v| v.as_str()) == Some(id));
    }
    if let Some(n_raw) = flag_value(argv, "last") {
        if let Ok(n) = n_raw.parse::<usize>() {
            let start = records.len().saturating_sub(n);
            records = records[start..].to_vec();
        }
    }

    let arr = Value::Array(records);
    // Exact "[]" for the empty case; compact array otherwise.
    Out::ok(serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into()))
}

// ============================================================================
// MCP-facing API — the journal MCP tools/resource delegate here so the CLI and the MCP surface
// share ONE implementation (no duplicated upsert/pricing/read logic).
// ============================================================================

/// MCP `tf_journal_append`: upsert an open entry from typed params (`roadmap_id`, `tokens`,
/// `model`, optional `ask`). Returns a typed error string on bad input or IO failure — the same
/// validation the CLI `append` enforces, expressed over the MCP param object.
#[cfg(feature = "mcp")]
pub fn mcp_append(params: &Value) -> Result<Value, String> {
    let id = params
        .get("roadmap_id")
        .and_then(|v| v.as_str())
        .ok_or("missing or invalid 'roadmap_id'")?;
    let tokens = params
        .get("tokens")
        .and_then(|v| v.as_i64())
        .ok_or("missing or invalid 'tokens'")?;
    let model = params
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or("missing or invalid 'model'")?;

    let mut argv = vec![
        "append".to_string(),
        id.to_string(),
        tokens.to_string(),
        model.to_string(),
    ];
    if let Some(ask) = params.get("ask").and_then(|v| v.as_str()) {
        argv.push("--ask".to_string());
        argv.push(ask.to_string());
    }
    let out = dispatch(&argv);
    if out.code != 0 {
        return Err(out.stderr.trim().to_string());
    }
    Ok(json!({ "success": true, "roadmap_id": id }))
}

/// MCP `tf_journal_read`: return finalised records as a JSON array, optionally filtered by
/// `roadmap_id` and/or `last`. Mirrors the CLI `read` filters so MCP and CLI reads agree.
#[cfg(feature = "mcp")]
pub fn mcp_read(params: &Value) -> Result<Value, String> {
    let mut argv = vec!["read".to_string()];
    if let Some(id) = params.get("roadmap_id").and_then(|v| v.as_str()) {
        argv.push("--id".to_string());
        argv.push(id.to_string());
    }
    if let Some(n) = params.get("last").and_then(|v| v.as_i64()) {
        argv.push("--last".to_string());
        argv.push(n.to_string());
    }
    let out = dispatch(&argv);
    serde_json::from_str::<Value>(&out.stdout)
        .map_err(|e| format!("journal read produced invalid JSON: {}", e))
}

/// The `tf://cost-journal` MCP resource: the last 100 finalised records, oldest-first. Reuses the
/// same [`read_records`] the CLI reads, so the resource view matches `tf journal read`.
#[cfg(feature = "mcp")]
pub fn mcp_resource_last_100() -> Result<Value, String> {
    let records = read_records();
    let start = records.len().saturating_sub(100);
    Ok(Value::Array(records[start..].to_vec()))
}

/// `tf journal {append|close|read}` dispatch. `argv` is everything AFTER the `journal` verb.
pub fn dispatch(argv: &[String]) -> Out {
    let sub = argv.first().map(|s| s.as_str()).unwrap_or("");
    let rest = if argv.is_empty() { &[][..] } else { &argv[1..] };
    match sub {
        "append" => append(rest),
        "close" => close(rest),
        "read" => read(rest),
        _ => Out::err(
            "usage: tf journal {append <id> <tokens> <model> [--ask <text>]|close <id> [--summarize]|read [--id <id>] [--last <n>]}",
            2,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{temp_dir, ENV_LOCK};

    /// Point the two journal files at a fresh temp dir for the duration of a test.
    fn with_env(tag: &str) -> std::path::PathBuf {
        let dir = temp_dir(tag);
        std::env::set_var("I2P_COST_STATE_DIR", &dir);
        std::env::set_var("I2P_COST_JOURNAL", dir.join("cost-journal.jsonl"));
        std::env::set_var("I2P_COST_JOURNAL_OPEN", dir.join("journal-open.json"));
        dir
    }

    fn clear_env() {
        for k in [
            "I2P_COST_STATE_DIR",
            "I2P_COST_JOURNAL",
            "I2P_COST_JOURNAL_OPEN",
        ] {
            std::env::remove_var(k);
        }
    }

    fn s(args: &[&str]) -> Out {
        dispatch(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn append_creates_accumulates_and_preserves_ask() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-append");

        // create
        assert_eq!(
            s(&["append", "7", "100", "claude-opus-4-8", "--ask", "draft"]).code,
            0
        );
        // accumulate same model + add second model
        assert_eq!(s(&["append", "7", "50", "claude-opus-4-8"]).code, 0);
        assert_eq!(s(&["append", "7", "25", "claude-haiku-4-5"]).code, 0);

        let open: Value = read_open().unwrap();
        assert_eq!(
            open.pointer("/7/accumulated_tokens").unwrap().as_i64(),
            Some(175)
        );
        assert_eq!(
            open.pointer("/7/by_model/claude-opus-4-8")
                .unwrap()
                .as_i64(),
            Some(150)
        );
        assert_eq!(
            open.pointer("/7/by_model/claude-haiku-4-5")
                .unwrap()
                .as_i64(),
            Some(25)
        );
        // ask preserved when --ask omitted
        assert_eq!(open.pointer("/7/ask").unwrap().as_str(), Some("draft"));

        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_rejects_bad_input_without_writing() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-append-bad");
        assert_ne!(s(&["append", "", "100", "m"]).code, 0); // empty id
        assert_ne!(s(&["append", "7", "100"]).code, 0); // missing model
        assert_ne!(s(&["append", "7", "abc", "m"]).code, 0); // non-numeric tokens
        assert!(
            read_open().unwrap().get("7").is_none(),
            "no entry written on error"
        );
        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_corrupt_open_file_is_typed_error() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-corrupt");
        std::fs::write(dir.join("journal-open.json"), b"not json }{{").unwrap();
        let out = s(&["append", "7", "100", "m"]);
        assert_ne!(out.code, 0);
        assert!(out.stderr.contains("corrupt"));
        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn close_prices_appends_and_clears() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-close");
        std::fs::write(
            dir.join("journal-open.json"),
            r#"{"7":{"ts_opened":1,"ask":"x","accumulated_tokens":1000000,"by_model":{"claude-opus-4-8":1000000,"unknown-x":10}}}"#,
        )
        .unwrap();
        let out = s(&["close", "7"]);
        assert_eq!(out.code, 0);

        let recs = read_records();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].get("roadmap_id").unwrap().as_str(), Some("7"));
        assert_eq!(
            recs[0].get("total_tokens").unwrap().as_i64(),
            Some(1_000_010)
        );
        // opus priced at input rate $5/1M → ~$5.00; unknown contributes 0.
        let cost = recs[0].get("total_cost_usd").unwrap().as_f64().unwrap();
        assert!((cost - 5.0).abs() < 1e-6);
        assert_eq!(
            recs[0]
                .pointer("/by_model/unknown-x/tokens")
                .unwrap()
                .as_i64(),
            Some(10)
        );
        // open key cleared
        assert!(read_open().unwrap().get("7").is_none());
        // no [8] projection fields
        assert!(recs[0].get("projections").is_none());

        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn close_errors_on_empty_or_missing_id() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-close-err");
        std::fs::write(dir.join("journal-open.json"), r#"{"7":{"by_model":{}}}"#).unwrap();
        assert_ne!(s(&["close", ""]).code, 0);
        assert_ne!(s(&["close", "9"]).code, 0);
        assert!(read_records().is_empty(), "no record on a failed close");
        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn close_truncates_ask_to_100_chars() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-trunc");
        let long = "A".repeat(250);
        std::fs::write(
            dir.join("journal-open.json"),
            format!(r#"{{"7":{{"ask":"{}","accumulated_tokens":10,"by_model":{{"claude-haiku-4-5":10}}}}}}"#, long),
        )
        .unwrap();
        assert_eq!(s(&["close", "7"]).code, 0);
        let recs = read_records();
        let sum = recs[0].get("ask_summary").unwrap().as_str().unwrap();
        assert_eq!(sum.len(), 100);
        assert_eq!(sum, &long[..100]);
        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_filters_limits_and_handles_absent() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-read");
        // absent journal ⇒ "[]"
        assert_eq!(s(&["read"]).stdout.trim(), "[]");
        std::fs::write(
            dir.join("cost-journal.jsonl"),
            concat!(
                "{\"roadmap_id\":\"5\",\"ts\":1}\n",
                "{\"roadmap_id\":\"6\",\"ts\":2}\n",
                "{\"roadmap_id\":\"7\",\"ts\":3}\n",
            ),
        )
        .unwrap();
        // all
        let all: Vec<Value> = serde_json::from_str(&s(&["read"]).stdout).unwrap();
        assert_eq!(all.len(), 3);
        // --id filter
        let one: Vec<Value> = serde_json::from_str(&s(&["read", "--id", "6"]).stdout).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].get("roadmap_id").unwrap().as_str(), Some("6"));
        // --last 2 (append order)
        let last: Vec<Value> = serde_json::from_str(&s(&["read", "--last", "2"]).stdout).unwrap();
        assert_eq!(last.len(), 2);
        assert_eq!(last[0].get("roadmap_id").unwrap().as_str(), Some("6"));
        assert_eq!(last[1].get("roadmap_id").unwrap().as_str(), Some("7"));
        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn append_rejects_non_object_open_file() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-nonobj");
        // Valid JSON, but an ARRAY — not the object-keyed-by-id shape append requires.
        std::fs::write(dir.join("journal-open.json"), b"[1,2,3]").unwrap();
        let out = s(&["append", "7", "100", "m"]);
        assert_ne!(out.code, 0);
        assert!(out.stderr.contains("not a JSON object"));
        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_subcommand_is_usage_error() {
        assert_eq!(s(&["wat"]).code, 2);
        assert_eq!(s(&[]).code, 2);
    }

    // ---- MCP-facing API (in-process so llvm-cov sees the branches) ----

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_append_read_resource_round_trip() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-mcp");

        // append via the MCP entry point (with the optional ask branch).
        let r = mcp_append(&json!({
            "roadmap_id": "7", "tokens": 50000, "model": "claude-opus-4-8", "ask": "draft"
        }))
        .expect("mcp_append ok");
        assert_eq!(r.get("success").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            read_open()
                .unwrap()
                .pointer("/7/accumulated_tokens")
                .unwrap()
                .as_i64(),
            Some(50000)
        );

        // finalise so there is a record to read.
        assert_eq!(s(&["close", "7"]).code, 0);

        // mcp_read with the roadmap_id + last filters (both branches).
        let arr = mcp_read(&json!({"roadmap_id": "7", "last": 5})).expect("mcp_read ok");
        let arr = arr.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("roadmap_id").unwrap().as_str(), Some("7"));

        // mcp_resource_last_100 returns the finalised record.
        let res = mcp_resource_last_100().expect("resource ok");
        assert_eq!(res.as_array().unwrap().len(), 1);

        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_append_rejects_bad_params_and_propagates_dispatch_error() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = with_env("jr-mcp-bad");
        // missing each required param.
        assert!(mcp_append(&json!({"tokens": 1, "model": "m"})).is_err());
        assert!(mcp_append(&json!({"roadmap_id": "7", "model": "m"})).is_err());
        assert!(mcp_append(&json!({"roadmap_id": "7", "tokens": 1})).is_err());
        // valid params but dispatch fails (empty id) ⇒ error propagated from the dispatch arm.
        assert!(mcp_append(&json!({"roadmap_id": "", "tokens": 1, "model": "m"})).is_err());
        clear_env();
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- summarizer (envelope + bare-text + empty branches) ----

    /// Write an executable stub `curl` into `dir` and return a PATH with `dir` prepended.
    #[cfg(all(unix, feature = "journal-summarizer"))]
    fn stub_curl(dir: &std::path::Path, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let curl = dir.join("curl");
        std::fs::write(&curl, script).unwrap();
        let mut perms = std::fs::metadata(&curl).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&curl, perms).unwrap();
        let orig = std::env::var("PATH").unwrap_or_default();
        format!("{}:{}", dir.to_str().unwrap(), orig)
    }

    #[cfg(all(unix, feature = "journal-summarizer"))]
    #[test]
    fn summarize_via_curl_parses_envelope_bare_and_empty() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("jr-curl");
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");

        // 1) JSON envelope with a content[0].text field ⇒ that text.
        std::env::set_var(
            "PATH",
            stub_curl(
                &dir,
                "#!/bin/sh\necho '{\"content\":[{\"text\":\"envelope summary\"}]}'\n",
            ),
        );
        assert_eq!(summarize_via_curl("x").as_deref(), Some("envelope summary"));

        // 2) bare (non-JSON) text ⇒ trimmed text.
        let d2 = temp_dir("jr-curl2");
        std::env::set_var("PATH", stub_curl(&d2, "#!/bin/sh\necho 'bare summary'\n"));
        assert_eq!(summarize_via_curl("x").as_deref(), Some("bare summary"));

        // 3) empty output ⇒ None (fail open).
        let d3 = temp_dir("jr-curl3");
        std::env::set_var("PATH", stub_curl(&d3, "#!/bin/sh\necho ''\n"));
        assert_eq!(summarize_via_curl("x"), None);

        // 4) JSON without a text field ⇒ None (fail open).
        let d4 = temp_dir("jr-curl4");
        std::env::set_var("PATH", stub_curl(&d4, "#!/bin/sh\necho '{\"other\":1}'\n"));
        assert_eq!(summarize_via_curl("x"), None);

        // 5) empty key ⇒ None before any subprocess.
        std::env::set_var("ANTHROPIC_API_KEY", "");
        assert_eq!(summarize_via_curl("x"), None);

        // 6) key entirely UNSET ⇒ None (the `.ok()?` early-return branch).
        std::env::remove_var("ANTHROPIC_API_KEY");
        assert_eq!(summarize_via_curl("x"), None);

        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("PATH");
        for d in [&dir, &d2, &d3, &d4] {
            std::fs::remove_dir_all(d).ok();
        }
    }
}
