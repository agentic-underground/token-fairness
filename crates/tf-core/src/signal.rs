//! signal — the SELF-DRIVING signal probe + the phase-0 payload recorder.
//! Ports `signal-probe.sh` (conclude/verdict/report) and `verify-payload.sh`.
//!
//! `verify-payload` appends one JSONL line per hook fire describing the payload (does it
//! carry `.rate_limits`?). `signal conclude` groups that log by hook event and writes the
//! standing `signal-findings.json` verdict that the live-ceiling guard adapts to.

use crate::{state, Out};
use serde_json::{json, Value};

fn probe_path() -> String {
    if let Ok(p) = std::env::var("I2P_PAYLOAD_PROBE") {
        return p;
    }
    format!("{}/payload-probe.jsonl", state::home_cost_dir())
}

fn findings_path() -> String {
    if let Ok(p) = std::env::var("I2P_SIGNAL_FINDINGS") {
        return p;
    }
    format!("{}/signal-findings.json", state::home_cost_dir())
}

/// `tf verify-payload [--report]` — append one probe line for the piped payload.
pub fn verify_payload(first_arg: Option<&str>, payload: &str) -> Out {
    if first_arg == Some("--report") {
        return report();
    }
    if payload.trim().is_empty() {
        return Out::default();
    }
    let v: Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return Out::default(),
    };
    let now = state::now_epoch();

    // keys → jq `keys` sorts ascending.
    let mut keys: Vec<String> = v
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();

    let null_if_absent = |ptr: &str| {
        v.pointer(ptr)
            .cloned()
            .filter(|x| !x.is_null())
            .unwrap_or(Value::Null)
    };
    let tool = v
        .get("tool_name")
        .cloned()
        .filter(|x| !x.is_null())
        .or_else(|| v.pointer("/tool/name").cloned().filter(|x| !x.is_null()))
        .unwrap_or(Value::Null);
    let hook_event = v
        .get("hook_event_name")
        .cloned()
        .filter(|x| !x.is_null())
        .or_else(|| v.get("hookEventName").cloned().filter(|x| !x.is_null()))
        .unwrap_or(Value::Null);

    let line = json!({
        "at": now,
        "top_level_keys": keys,
        "has_rate_limits": v.get("rate_limits").is_some(),
        "five_hour_pct": null_if_absent("/rate_limits/five_hour/used_percentage"),
        "has_cost": v.get("cost").is_some(),
        "cost_usd": null_if_absent("/cost/total_cost_usd"),
        "has_transcript": v.get("transcript_path").is_some(),
        "tool": tool,
        "hook_event": hook_event
    });

    let log = format!("{}/payload-probe.jsonl", state::state_dir());
    if let Some(dir) = std::path::Path::new(&log).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        let _ = writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default());
    }
    Out::default()
}

/// `tf signal {conclude|verdict|report}` (default report).
pub fn dispatch(argv: &[String]) -> Out {
    match argv.first().map(|s| s.as_str()).unwrap_or("report") {
        "conclude" => conclude(),
        "verdict" => verdict(),
        "report" => report(),
        _ => Out::err("usage: signal-probe.sh {conclude|verdict|report}", 2),
    }
}

fn conclude() -> Out {
    let probe = probe_path();
    let raw = match std::fs::read_to_string(&probe) {
        Ok(s) => s,
        Err(_) => {
            return Out::ok(format!(
                "signal-probe: no capture log at {} (nothing to conclude)\n",
                probe
            ))
        }
    };
    let entries: Vec<Value> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    // group_by(.hook_event): jq sorts (null first, then strings ascending), groups consecutive.
    let mut keys: Vec<Option<String>> = entries
        .iter()
        .map(|e| {
            e.get("hook_event")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    keys.sort();
    keys.dedup();

    let mut events = serde_json::Map::new();
    for k in &keys {
        let members: Vec<&Value> = entries
            .iter()
            .filter(|e| {
                e.get("hook_event")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    == *k
            })
            .collect();
        let fires = members.len();
        let with = members
            .iter()
            .filter(|e| {
                e.get("has_rate_limits")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
            })
            .count();
        let key = k.clone().unwrap_or_else(|| "null".to_string());
        events.insert(
            key,
            json!({ "fires": fires, "with_rate_limits": with, "present": with > 0 }),
        );
    }

    let total_with = entries
        .iter()
        .filter(|e| {
            e.get("has_rate_limits")
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        })
        .count();

    let (verdict, guard) = if total_with > 0 {
        ("hook-signal-available", "live-ceiling")
    } else {
        ("no-hook-signal", "budget-cap")
    };
    let note = if verdict == "no-hook-signal" {
        "No hook event carries .rate_limits in this harness build. The live ceiling guard returns ASK; the budget cap + off-peak window + per-wave throttle are the real guards. (The interactive statusline does receive the signal, but a headless cron has no statusline.)"
    } else {
        "At least one hook event carries .rate_limits — the snapshot bridge can feed the live ceiling guard."
    };
    let now = state::now_epoch();
    let findings = json!({
        "concluded_at": now,
        "verdict": verdict,
        "guard_mode": guard,
        "total_captures_with_signal": total_with,
        "events": Value::Object(events),
        "note": note
    });
    let fp = findings_path();
    if state::write_json(&fp, &findings).is_err() {
        return Out::err("signal-probe: failed to write findings", 1);
    }
    Out::ok(format!(
        "signal-probe: concluded → {} (guard: {}); written to {}\n",
        verdict, guard, fp
    ))
}

fn verdict() -> Out {
    let v = state::read_json(&findings_path())
        .and_then(|f| {
            f.get("verdict")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());
    Out::ok(v + "\n")
}

fn report() -> Out {
    let fp = findings_path();
    let f = match state::read_json(&fp) {
        Some(f) => f,
        None => {
            return Out::ok("No signal findings yet. Run: signal-probe.sh conclude\n".to_string())
        }
    };
    let v = f.get("verdict").and_then(|x| x.as_str()).unwrap_or("null");
    let g = f
        .get("guard_mode")
        .and_then(|x| x.as_str())
        .unwrap_or("null");
    let mut s = format!(
        "🔎 Live-signal probe — verdict: {}  (guard mode: {})\n",
        v, g
    );
    if let Some(events) = f.get("events").and_then(|e| e.as_object()) {
        for (k, val) in events {
            let fires = val.get("fires").and_then(|x| x.as_i64()).unwrap_or(0);
            let with = val
                .get("with_rate_limits")
                .and_then(|x| x.as_i64())
                .unwrap_or(0);
            s.push_str(&format!(
                "   {}: {} fires · rate_limits in {}\n",
                k, fires, with
            ));
        }
    }
    let note = f.get("note").and_then(|x| x.as_str()).unwrap_or("");
    s.push_str(&format!("   {}\n", note));
    Out::ok(s)
}
