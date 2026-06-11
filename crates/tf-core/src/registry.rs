//! registry — the DURABLE scheduled-job registry. Port of `jobs-registry.sh`.
//!
//! Two scopes kept in sync: the project's `<dir>/.i2p/scheduled-jobs.json` (authoritative,
//! carries `armed`/`armed_via`) and a per-machine index at
//! `$I2P_MACHINE_REGISTRY | ~/.claude/state/i2p-cost/scheduled-jobs.json` (keyed by repo+id).
//! `reset-armed` is the SessionStart semantics: session arming is ephemeral, oscron durable.

use crate::{state, Out};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

fn proj_path(dir: &str) -> String {
    let d = dir.strip_suffix('/').unwrap_or(dir);
    format!("{}/.i2p/scheduled-jobs.json", d)
}

fn machine_path() -> String {
    if let Ok(p) = std::env::var("I2P_MACHINE_REGISTRY") {
        return p;
    }
    format!("{}/scheduled-jobs.json", state::state_dir())
}

/// Lexical absolute path of `dir` — matches bash `cd "$dir" && pwd` (logical, no symlink
/// resolution). If `dir` is not an existing directory, bash echoes it verbatim.
fn abs_dir(dir: &str) -> String {
    let p = Path::new(dir);
    if !p.is_dir() {
        return dir.to_string();
    }
    let base = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in base.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Read `<file>.jobs` as an array (the `(.jobs // [])` of every verb).
fn jobs_of(path: &str) -> Vec<Value> {
    state::read_json(path)
        .and_then(|v| v.get("jobs").cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
}

fn write_jobs(path: &str, jobs: Vec<Value>) {
    let _ = state::write_json(path, &json!({ "jobs": jobs }));
}

pub fn dispatch(argv: &[String]) -> Out {
    let cmd = argv.first().map(|s| s.as_str()).unwrap_or("list");
    let dir = argv.get(1).map(|s| s.as_str()).unwrap_or(".");
    let arg = |i: usize| argv.get(i).map(|s| s.as_str()).unwrap_or("");
    let proj = proj_path(dir);

    match cmd {
        "register" => {
            let id = arg(2);
            if id.is_empty() {
                return Out::err("jobs-registry: <id> required", 2);
            }
            let cron = arg(3);
            let budget = state::digits_or(arg(4), 0);
            let ledger = arg(5);
            let prompt = arg(6);
            let note = arg(7);
            let machine = machine_path();
            let repo = abs_dir(dir);

            let mut pjobs = jobs_of(&proj);
            pjobs.retain(|j| j.get("id").and_then(|v| v.as_str()) != Some(id));
            pjobs.push(json!({
                "id": id, "cron": cron, "recurring": true, "budget_total": budget,
                "ledger": ledger, "prompt_file": prompt, "note": note, "armed": false
            }));
            write_jobs(&proj, pjobs);

            let mut mjobs = jobs_of(&machine);
            mjobs.retain(|j| {
                !(j.get("repo").and_then(|v| v.as_str()) == Some(repo.as_str())
                    && j.get("id").and_then(|v| v.as_str()) == Some(id))
            });
            mjobs.push(json!({
                "repo": repo, "id": id, "cron": cron, "budget_total": budget, "note": note
            }));
            write_jobs(&machine, mjobs);

            Out::ok(format!(
                "jobs-registry: registered {} (project + machine index)\n",
                id
            ))
        }

        "list" => {
            let jobs = jobs_of(&proj);
            Out::ok(
                serde_json::to_string(&Value::Array(jobs)).unwrap_or_else(|_| "[]".into()) + "\n",
            )
        }

        "get" => {
            let id = arg(2);
            let jobs = jobs_of(&proj);
            let found = jobs
                .into_iter()
                .find(|j| j.get("id").and_then(|v| v.as_str()) == Some(id))
                .unwrap_or_else(|| json!({}));
            Out::ok(serde_json::to_string(&found).unwrap_or_else(|_| "{}".into()) + "\n")
        }

        "arm" => {
            let id = arg(2);
            let method = if arg(3).is_empty() { "session" } else { arg(3) };
            let mut jobs = jobs_of(&proj);
            for j in jobs.iter_mut() {
                if j.get("id").and_then(|v| v.as_str()) == Some(id) {
                    j["armed"] = json!(true);
                    j["armed_via"] = json!(method);
                }
            }
            write_jobs(&proj, jobs);
            Out::ok(format!("jobs-registry: armed {} (via {})\n", id, method))
        }

        "reset-armed" => {
            let mut jobs = jobs_of(&proj);
            for j in jobs.iter_mut() {
                let via = j
                    .get("armed_via")
                    .and_then(|v| v.as_str())
                    .unwrap_or("session");
                if via != "oscron" {
                    j["armed"] = json!(false);
                }
            }
            write_jobs(&proj, jobs);
            Out::default()
        }

        "remove" => {
            let id = arg(2);
            let machine = machine_path();
            let repo = abs_dir(dir);
            let mut pjobs = jobs_of(&proj);
            pjobs.retain(|j| j.get("id").and_then(|v| v.as_str()) != Some(id));
            write_jobs(&proj, pjobs);
            let mut mjobs = jobs_of(&machine);
            mjobs.retain(|j| {
                !(j.get("repo").and_then(|v| v.as_str()) == Some(repo.as_str())
                    && j.get("id").and_then(|v| v.as_str()) == Some(id))
            });
            write_jobs(&machine, mjobs);
            Out::ok(format!("jobs-registry: removed {}\n", id))
        }

        _ => Out::err(
            "usage: jobs-registry.sh {register|list|get|arm|remove} <dir> [id] …",
            2,
        ),
    }
}
