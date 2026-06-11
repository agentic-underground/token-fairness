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

use crate::{ensemble, fmt, state, Out};
use serde_json::{json, Value};

const ALPHA: f64 = 0.4;
/// Cap the persisted ratio history so the file stays small but a new algorithm can still backtest.
const LOG_CAP: usize = 200;

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

/// `calibrate ratio <name>` → the learned ratio the estimator now uses. This is the **champion's**
/// prediction (the lowest-error algorithm), falling back to the legacy EWMA-0.4 (`ratio_ewma`) for
/// entries written before the ensemble, and `1.0` when there is no data. Champion-driven, so the
/// headline estimate gets the accuracy gain.
pub fn ratio_string(name: &str) -> String {
    let path = state::calibration_file();
    let v = match state::read_json(&path) {
        Some(v) => v,
        None => return "1.0".into(),
    };
    let Some(e) = v.get(name) else {
        return "1.0".into();
    };
    let samples = state::int(e, "samples", 0);
    if let Some(champ) = e.get("champion_ratio").and_then(|x| x.as_f64()) {
        return ratio_text(samples, champ);
    }
    match e.get("ratio_ewma").and_then(|x| x.as_f64()) {
        Some(val) => ratio_text(samples, val),
        None => "1.0".into(),
    }
}

pub fn ratio(name: &str) -> Out {
    Out::ok(ratio_string(name) + "\n")
}

/// Smoothing constant for taxonomy backoff — how many samples a node needs to half-trust itself.
const BACKOFF_K0: f64 = 3.0;

/// The ratio the ESTIMATOR uses, with hierarchical-taxonomy backoff (Phase 3). For a key like
/// `experiment/code-gen/opus`, a node with few samples shrinks toward its parent's blended ratio,
/// recursively up to the global prior 1.0 — so a brand-new job type inherits a sane estimate, and
/// fidelity rises automatically as the node accrues its own samples (`w = n/(n+k0)`). A flat key
/// (no `/`) shrinks toward 1.0 directly; with zero samples it IS 1.0 (estimate unchanged).
pub fn resolved_ratio(name: &str) -> f64 {
    let root = state::read_json(&state::calibration_file()).unwrap_or_else(|| json!({}));
    resolve_node(&root, name)
}

fn resolve_node(root: &Value, key: &str) -> f64 {
    if key.is_empty() {
        return 1.0; // the global prior: ratio 1.0 == "no adjustment"
    }
    let entry = root.get(key);
    let n = entry.map(|e| state::int(e, "samples", 0)).unwrap_or(0) as f64;
    let own = entry
        .and_then(|e| e.get("champion_ratio").and_then(|x| x.as_f64()))
        .or_else(|| entry.and_then(|e| e.get("ratio_ewma").and_then(|x| x.as_f64())))
        .unwrap_or(1.0);
    let parent = key.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    let w = n / (n + BACKOFF_K0);
    w * own + (1.0 - w) * resolve_node(root, parent)
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

    // ── Ensemble: score every algorithm's STANDING prediction (on the pre-sample history) against
    // the realised ratio r, fold its online error, then append r and re-select champion + blend.
    let mut log: Vec<f64> = prev
        .get("samples_log")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default();
    let mut errs: serde_json::Map<String, Value> = prev
        .get("algorithms")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if !log.is_empty() {
        for (aname, f) in ensemble::algorithms() {
            let pred = f(&log);
            let prior = errs.get(aname).and_then(|x| x.as_f64()).unwrap_or(-1.0);
            errs.insert(aname.to_string(), json!(ensemble::fold_err(prior, pred, r)));
        }
    }
    log.push(r);
    if log.len() > LOG_CAP {
        log.drain(0..log.len() - LOG_CAP);
    }
    let verdict = ensemble::decide(&log, |n| {
        errs.get(n).and_then(|x| x.as_f64()).unwrap_or(-1.0)
    });

    // Preserve any pre-existing extra fields; overwrite the known ones (legacy + ensemble).
    let mut entry = prev.as_object().cloned().unwrap_or_default();
    entry.insert("samples".into(), json!(samples));
    entry.insert("ratio_ewma".into(), json!(new_ewma)); // legacy EWMA-0.4 (back-compat / band)
    entry.insert("w_n".into(), json!(wn));
    entry.insert("w_mean".into(), json!(wmean));
    entry.insert("w_m2".into(), json!(wm2));
    entry.insert("last_ratio".into(), json!(r));
    entry.insert("prev_band".into(), json!(old_band));
    entry.insert("samples_log".into(), json!(log));
    entry.insert("algorithms".into(), Value::Object(errs));
    entry.insert("champion".into(), json!(verdict.champion));
    entry.insert("champion_ratio".into(), json!(verdict.champion_ratio));
    entry.insert("blend_ratio".into(), json!(verdict.blend_ratio));
    entry.insert("mape".into(), json!(verdict.mape));
    root.as_object_mut()
        .unwrap()
        .insert(name.to_string(), Value::Object(entry));

    let serialized = serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".into());
    let _ = state::write_atomic(&path, &(serialized + "\n"));

    // The estimator's own accuracy ledger — one line per closed job (the KAIZEN over-time signal).
    append_accuracy(name, est_n, act_n, r, &verdict);

    Out::ok(ratio_text(samples, verdict.champion_ratio) + "\n")
}

/// Append one record to the accuracy ledger (best-effort; never blocks `close`).
fn append_accuracy(name: &str, est: i64, act: i64, ratio: f64, v: &ensemble::Verdict) {
    use std::io::Write;
    let path = state::accuracy_ledger();
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let line = json!({
        "at": state::now_epoch(), "key": name, "est": est, "actual": act,
        "ratio": ratio, "champion": v.champion, "champion_ratio": v.champion_ratio,
        "blend_ratio": v.blend_ratio, "mape": v.mape
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default());
    }
}

/// The KAIZEN view for `<name>` — champion, blend, MAPE, and the full per-algorithm scoreboard.
/// One-line JSON; the surface `tf estimator` and `tf report --kaizen` render.
pub fn kaizen_string(name: &str) -> String {
    let path = state::calibration_file();
    let entry = state::read_json(&path).and_then(|v| v.get(name).cloned());
    let Some(e) = entry else {
        return format!(
            "{{\"key\":\"{}\",\"samples\":0,\"champion\":\"{}\",\"champion_ratio\":1.0,\"blend_ratio\":1.0,\"mape\":null,\"board\":[]}}",
            name,
            ensemble::DEFAULT_CHAMPION
        );
    };
    let log: Vec<f64> = e
        .get("samples_log")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default();
    let errs = e
        .get("algorithms")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let verdict = ensemble::decide(&log, |n| {
        errs.get(n).and_then(|x| x.as_f64()).unwrap_or(-1.0)
    });
    let samples = state::int(&e, "samples", 0);

    let board: Vec<String> = verdict
        .board
        .iter()
        .map(|(n, err, pred)| {
            let err_s = if *err < 0.0 {
                "null".to_string()
            } else {
                fmt::fixed(*err, 4)
            };
            format!(
                "{{\"algo\":\"{}\",\"err\":{},\"pred\":{}}}",
                n,
                err_s,
                fmt::fixed(*pred, 4)
            )
        })
        .collect();
    let mape_s = if samples < 1 {
        "null".to_string()
    } else {
        fmt::fixed(verdict.mape, 4)
    };
    format!(
        "{{\"key\":\"{}\",\"samples\":{},\"champion\":\"{}\",\"champion_ratio\":{},\"blend_ratio\":{},\"mape\":{},\"board\":[{}]}}",
        name,
        samples,
        verdict.champion,
        fmt::fixed(verdict.champion_ratio, 4),
        fmt::fixed(verdict.blend_ratio, 4),
        mape_s,
        board.join(",")
    )
}

pub fn kaizen(name: &str) -> Out {
    Out::ok(kaizen_string(name) + "\n")
}

/// `tf estimator backtest <key>` — replay the recorded `samples_log` from scratch through every
/// algorithm, scoring each one's full-history MAPE, and rank them. This is the deterministic
/// "self-review" / best-formula hunt: it proves which formula WOULD have predicted this class best,
/// independent of the online (recency-weighted) champion. The basis for promoting a new algorithm.
pub fn backtest_string(name: &str) -> String {
    let path = state::calibration_file();
    let log: Vec<f64> = state::read_json(&path)
        .and_then(|v| v.get(name).cloned())
        .and_then(|e| e.get("samples_log").and_then(|v| v.as_array()).cloned())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default();

    if log.len() < 2 {
        return format!(
            "{{\"key\":\"{}\",\"samples\":{},\"note\":\"need ≥2 samples to backtest\",\"ranking\":[]}}",
            name,
            log.len()
        );
    }

    // Mean APE of each algorithm predicting log[i] from log[..i], for i = 1..n.
    let mut scored: Vec<(String, f64)> = ensemble::algorithms()
        .iter()
        .map(|(aname, f)| {
            let mut sum = 0.0;
            let mut count = 0u32;
            for i in 1..log.len() {
                sum += ensemble::ape(f(&log[..i]), log[i]);
                count += 1;
            }
            (aname.to_string(), sum / count.max(1) as f64)
        })
        .collect();
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let best = scored.first().map(|(n, _)| n.clone()).unwrap_or_default();

    let ranking: Vec<String> = scored
        .iter()
        .map(|(n, m)| format!("{{\"algo\":\"{}\",\"mape\":{}}}", n, fmt::fixed(*m, 4)))
        .collect();
    format!(
        "{{\"key\":\"{}\",\"samples\":{},\"best\":\"{}\",\"ranking\":[{}]}}",
        name,
        log.len(),
        best,
        ranking.join(",")
    )
}

pub fn backtest(name: &str) -> Out {
    Out::ok(backtest_string(name) + "\n")
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
