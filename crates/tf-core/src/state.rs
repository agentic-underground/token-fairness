//! Shared state-file plumbing: path resolution (honouring the same env overrides as the
//! bash), JSON reads, and atomic write-then-rename (the bash `>$tmp && mv -f` pattern).

use serde_json::Value;
use std::path::Path;

pub fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// `~/.claude/state/i2p-cost` — overridable by `I2P_COST_STATE_DIR` (as in the bash).
pub fn state_dir() -> String {
    if let Ok(d) = std::env::var("I2P_COST_STATE_DIR") {
        return d;
    }
    format!("{}/.claude/state/i2p-cost", home())
}

/// The calibration ledger path — `I2P_CALIBRATION_FILE` overrides (tests rely on this).
pub fn calibration_file() -> String {
    if let Ok(p) = std::env::var("I2P_CALIBRATION_FILE") {
        return p;
    }
    format!("{}/calibration.json", state_dir())
}

pub fn read_json(path: &str) -> Option<Value> {
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

/// Atomic write: create parent dir, write a pid-tagged temp, rename over the target.
pub fn write_atomic(path: &str, content: &str) -> std::io::Result<()> {
    if let Some(dir) = Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let tmp = format!("{}.tmp.{}", path, std::process::id());
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

/// `obj.key` as f64 with a default (mirrors jq `(.key // default)` for numbers).
pub fn num(v: &Value, key: &str, default: f64) -> f64 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(default)
}

/// `obj.key` as i64 with a default.
pub fn int(v: &Value, key: &str, default: i64) -> i64 {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(default)
}
