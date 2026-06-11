//! estimate — L0 PRE-FLIGHT ESTIMATOR. Port of `scheduler-estimate.sh`.
//!
//!   est_total = fanout_width × per_unit_tokens × calibration_ratio
//!
//! `per_unit_tokens` is chosen by evidence, best-first (measured → history → declared →
//! seed); a `--class` short-circuits the profile path to price any plan, not just a
//! fan-out. The learned ratio and the convergence band come from [`crate::calibrate`].

use crate::{calibrate, fmt, Out};
use serde_json::Value;

const SEED_UNIT: i64 = 20000;

fn class_seed(class: &str) -> Option<i64> {
    match class {
        "small" => Some(25000),
        "medium" => Some(80000),
        "large" => Some(250000),
        "epic" => Some(700000),
        _ => None,
    }
}

fn is_pos_int(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<i64>().map(|n| n > 0).unwrap_or(false)
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// A profile JSON pointer as a string (jq `"$1 // empty"`), numbers stringified.
fn pj(profile: &Option<Value>, pointer: &str) -> Option<String> {
    let p = profile.as_ref()?;
    let v = p.pointer(pointer)?;
    if let Some(s) = v.as_str() {
        Some(s.to_string())
    } else if let Some(n) = v.as_i64() {
        Some(n.to_string())
    } else if v.is_number() {
        Some(fmt::shortest(v.as_f64().unwrap()))
    } else {
        None
    }
}

pub struct Args<'a> {
    pub profile_path: Option<&'a str>,
    pub width: Option<&'a str>,
    pub name: Option<&'a str>,
    pub measured: Option<&'a str>,
    pub history: Option<&'a str>,
    pub class: Option<&'a str>,
}

pub fn estimate(a: Args) -> Out {
    let profile: Option<Value> = a
        .profile_path
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok());

    let (name, width, per_unit, basis, confidence): (String, i64, i64, &str, &str);

    let class_seed_val = a.class.and_then(class_seed);
    if let (Some(class), Some(seed)) = (a.class, class_seed_val) {
        name = format!("plan:{}", class);
        width = 1;
        per_unit = seed;
        basis = "class";
        confidence = "low";
    } else {
        let mut nm = a.name.map(|s| s.to_string()).unwrap_or_default();
        if nm.is_empty() {
            nm = pj(&profile, "/name").unwrap_or_default();
        }
        if nm.is_empty() {
            nm = "unnamed".into();
        }
        name = nm;

        let w_raw = a
            .width
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| pj(&profile, "/fanout/width_default"))
            .unwrap_or_default();
        width = if all_digits(&w_raw) {
            w_raw.parse().unwrap()
        } else {
            1
        };

        let declared = pj(&profile, "/estimated_unit_tokens").unwrap_or_default();
        let declared_ok = all_digits(&declared);

        let measured = a.measured.unwrap_or("");
        let history = a.history.unwrap_or("");
        if is_pos_int(measured) {
            per_unit = measured.parse().unwrap();
            basis = "measured";
            confidence = "high";
        } else if is_pos_int(history) {
            per_unit = history.parse().unwrap();
            basis = "history";
            confidence = "high";
        } else if declared_ok {
            per_unit = declared.parse().unwrap();
            basis = "declared";
            confidence = "low";
        } else {
            per_unit = SEED_UNIT;
            basis = "seed";
            confidence = "low";
        }
    }

    let ratio_str = calibrate::ratio_string(&name);
    let ratio_f: f64 = ratio_str.parse().unwrap_or(1.0);

    let est_total = fmt::round_i64(width as f64 * per_unit as f64 * ratio_f);

    let conv = calibrate::confidence_string(&name);
    let band: f64 = serde_json::from_str::<Value>(&conv)
        .ok()
        .and_then(|v| v.get("p95_band_pct").and_then(|x| x.as_f64()))
        .unwrap_or(60.0);

    let lo = {
        let v = est_total as f64 * (1.0 - band / 100.0);
        fmt::round_i64(if v < 0.0 { 0.0 } else { v })
    };
    let hi = fmt::round_i64(est_total as f64 * (1.0 + band / 100.0));

    let line = format!(
        "{{\"name\":\"{}\",\"per_unit\":{},\"basis\":\"{}\",\"confidence\":\"{}\",\"fanout\":{},\"ratio\":{},\"est_total\":{},\"convergence\":{},\"interval\":[{},{}]}}\n",
        name, per_unit, basis, confidence, width, ratio_str, est_total, conv, lo, hi
    );
    Out::ok(line)
}
