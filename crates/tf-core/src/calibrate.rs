//! calibrate — the estimate↔actual CONVERGENCE engine. Port of `calibrate.sh`.
//!
//! Reads/writes the shared `calibration.json`, keyed per profile/class. Folds each
//! `actual/estimate` ratio into a canonical α=0.4 EWMA and tracks Welford online
//! variance so the p95 confidence band tightens as samples accrue.
//!
//! Faithfulness note: jq preserves the 4-decimal literal of the *first* sample's ratio
//! but renders every *computed* EWMA with shortest-round-trip (== Rust `f64` Display).
//! So `ratio`/`close` output is reconstructed from `(samples, ratio_ewma)` — 4-decimal
//! when `samples == 1`, shortest otherwise — rather than by replaying jq's storage.

use crate::{fmt, state, Out};
use serde_json::{json, Value};

const ALPHA: f64 = 0.4;

/// Render `ratio_ewma` the way the bash's `jq -r` would, given how many samples produced it.
fn ratio_text(samples: i64, val: f64) -> String {
    if samples == 1 {
        fmt::fixed(val, 4) // first sample: jq-preserved 4-decimal literal
    } else {
        fmt::shortest(val) // computed EWMA: shortest round-trip
    }
}

/// The p95 relative band (%) for a Welford state, rounded to 1 decimal — the bash
/// `old_band`/`confidence` arithmetic. Used both for `prev_band` capture and `confidence`.
fn band_pct(n: i64, mean: f64, m2: f64) -> f64 {
    if n <= 0 {
        return 60.0;
    }
    let mut b = if n >= 2 {
        let mut var = m2 / ((n - 1) as f64);
        if var < 0.0 {
            var = 0.0;
        }
        let sd = var.sqrt();
        if mean != 0.0 {
            1.645 * sd / mean * 100.0
        } else {
            50.0
        }
    } else {
        50.0
    };
    if n < 5 && b < 40.0 {
        b = 40.0;
    }
    fmt::fixed(b, 1).parse().unwrap_or(b)
}

/// `calibrate ratio <name>` → the learned `ratio_ewma` (default `1.0`).
pub fn ratio_string(name: &str) -> String {
    let path = state::calibration_file();
    let v = match state::read_json(&path) {
        Some(v) => v,
        None => return "1.0".into(),
    };
    match v.get(name) {
        Some(e) => match e.get("ratio_ewma").and_then(|x| x.as_f64()) {
            Some(val) => ratio_text(state::int(e, "samples", 0), val),
            None => "1.0".into(),
        },
        None => "1.0".into(),
    }
}

pub fn ratio(name: &str) -> Out {
    Out::ok(ratio_string(name) + "\n")
}

/// `calibrate close <name> <estimate> <actual>` — fold one sample, echo the new ratio.
/// Invalid input is advisory: a stderr note and exit 0 (never blocks the caller).
pub fn close(name: &str, est: &str, act: &str) -> Out {
    let est_n: i64 = match est.parse() {
        Ok(n) if n > 0 => n,
        Ok(_) => return Out::err("calibrate: estimate must be > 0", 0),
        Err(_) => return Out::err("calibrate: estimate must be a positive integer", 0),
    };
    let act_n: i64 = match act.parse() {
        Ok(n) => n,
        Err(_) => return Out::err("calibrate: actual must be a positive integer", 0),
    };

    // r = the 4-decimal-rounded actual/estimate (bash passes the "%.4f" string to jq).
    let r: f64 = fmt::fixed(act_n as f64 / est_n as f64, 4).parse().unwrap();

    let path = state::calibration_file();
    let mut root = state::read_json(&path).unwrap_or_else(|| json!({}));
    if !root.is_object() {
        root = json!({});
    }

    let prev = root.get(name).cloned().unwrap_or_else(|| json!({}));
    let old_samples = state::int(&prev, "samples", 0);
    let old_ewma = state::num(&prev, "ratio_ewma", 1.0);
    let old_wn = state::int(&prev, "w_n", 0);
    let old_wmean = state::num(&prev, "w_mean", 0.0);
    let old_wm2 = state::num(&prev, "w_m2", 0.0);

    // Band BEFORE this sample, so `confidence` can show a tightening trend.
    let old_band = band_pct(old_wn, old_wmean, old_wm2);

    let new_ewma = if old_samples == 0 {
        r
    } else {
        ALPHA * r + (1.0 - ALPHA) * old_ewma
    };
    let samples = old_samples + 1;
    let wn = old_wn + 1;
    let delta = r - old_wmean;
    let wmean = old_wmean + delta / wn as f64;
    let wm2 = old_wm2 + delta * (r - wmean);

    // Preserve any pre-existing extra fields on the entry; overwrite the known ones.
    let mut entry = prev.as_object().cloned().unwrap_or_default();
    entry.insert("samples".into(), json!(samples));
    entry.insert("ratio_ewma".into(), json!(new_ewma));
    entry.insert("w_n".into(), json!(wn));
    entry.insert("w_mean".into(), json!(wmean));
    entry.insert("w_m2".into(), json!(wm2));
    entry.insert("last_ratio".into(), json!(r));
    entry.insert("prev_band".into(), json!(old_band));
    root.as_object_mut()
        .unwrap()
        .insert(name.to_string(), Value::Object(entry));

    let serialized = serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".into());
    let _ = state::write_atomic(&path, &(serialized + "\n"));

    Out::ok(ratio_text(samples, new_ewma) + "\n")
}

/// The convergence picture for `<name>` — `confidence`'s one-line JSON.
pub fn confidence_string(name: &str) -> String {
    let path = state::calibration_file();
    let (n, mean, m2, prev) = match state::read_json(&path) {
        Some(v) => match v.get(name) {
            Some(e) => (
                state::int(e, "w_n", 0),
                state::num(e, "w_mean", 1.0),
                state::num(e, "w_m2", 0.0),
                state::num(e, "prev_band", -1.0),
            ),
            None => (0, 1.0, 0.0, -1.0),
        },
        None => (0, 1.0, 0.0, -1.0),
    };

    if n <= 0 {
        return format!(
            "{{\"samples\":0,\"mean_ratio\":1.0000,\"sd\":0.0000,\"p95_band_pct\":60.0,\"tier\":\"SEEDING\",\"prev_band\":{},\"trend\":\"flat\"}}",
            fmt::fixed(prev, 1)
        );
    }

    let (sd, mut band) = if n >= 2 {
        let mut var = m2 / ((n - 1) as f64);
        if var < 0.0 {
            var = 0.0;
        }
        let sd = var.sqrt();
        let band = if mean != 0.0 {
            1.645 * sd / mean * 100.0
        } else {
            50.0
        };
        (sd, band)
    } else {
        (0.0, 50.0)
    };

    let tier = if n < 5 {
        if band < 40.0 {
            band = 40.0;
        }
        "CALIBRATING"
    } else if n >= 10 && band <= 15.0 {
        "CONVERGED"
    } else {
        "CONVERGING"
    };

    let mut trend = "flat";
    if prev >= 0.0 {
        if band < prev - 0.05 {
            trend = "improving";
        } else if band > prev + 0.05 {
            trend = "worsening";
        }
    }

    format!(
        "{{\"samples\":{},\"mean_ratio\":{},\"sd\":{},\"p95_band_pct\":{},\"tier\":\"{}\",\"prev_band\":{},\"trend\":\"{}\"}}",
        n,
        fmt::fixed(mean, 4),
        fmt::fixed(sd, 4),
        fmt::fixed(band, 1),
        tier,
        fmt::fixed(prev, 1),
        trend
    )
}

pub fn confidence(name: &str) -> Out {
    Out::ok(confidence_string(name) + "\n")
}
