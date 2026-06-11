//! routing — Phase 2 COGNITION ROUTING. Makes the estimator predict a best-fit MODEL and a
//! $-cost band, not just a token band (plan §4).
//!
//! The routing rule: `best_fit = cheapest tier whose ceiling ≥ the unit's cognition floor`;
//! never downgrade below the floor to save tokens. `determinative` units leave the token
//! economy entirely — a tested handler produces the one correct output for 0 model tokens.
//!
//!   cognition_class → tier
//!   ─────────────────────────────────────────────────────────────────────
//!   determinative   → none   (a determinative_handler; 0 tokens, 0 $)
//!   mechanical      → haiku   (high-volume, low-judgement)
//!   discernment     → sonnet  (→ opus when a false PASS propagates: gates, security)
//!   thought-intensive → opus  (one error cascades)
//!
//!   cost($) = (in_tok·price.in + out_tok·price.out) / 1e6, the token band already scaled by
//!   ratio_ewma(profile) (it comes straight out of `estimate`). Pricing canon = model-prices.tsv
//!   (`prefix<TAB>in<TAB>out<TAB>cache_write<TAB>cache_read`, per 1M tokens); a built-in default
//!   keeps `tf route` working with no file, overridable by `--prices` / `$I2P_MODEL_PRICES`.

use crate::{estimate, fmt, Out};
use serde_json::Value;
use std::collections::HashMap;

/// (input, output) USD per 1M tokens — the built-in default, in sync with model-prices.tsv.
fn default_prices() -> HashMap<&'static str, (f64, f64)> {
    HashMap::from([
        ("claude-opus-4", (5.00, 25.00)),
        ("claude-sonnet-4", (3.00, 15.00)),
        ("claude-haiku-4", (1.00, 5.00)),
    ])
}

/// Load the price map from a TSV (`prefix\tin\tout\t…`), else the built-in default.
fn load_prices(path: Option<&str>) -> HashMap<String, (f64, f64)> {
    let p = path
        .map(|s| s.to_string())
        .or_else(|| std::env::var("I2P_MODEL_PRICES").ok());
    if let Some(p) = p {
        if let Ok(body) = std::fs::read_to_string(&p) {
            let mut m = HashMap::new();
            for line in body.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let cols: Vec<&str> = line.split('\t').collect();
                if cols.len() >= 3 {
                    if let (Ok(i), Ok(o)) = (cols[1].parse::<f64>(), cols[2].parse::<f64>()) {
                        m.insert(cols[0].to_string(), (i, o));
                    }
                }
            }
            if !m.is_empty() {
                return m;
            }
        }
    }
    default_prices()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

/// The tier a model name belongs to → its price-table prefix.
fn tier_prefix(tier: &str) -> &'static str {
    match tier {
        "haiku" => "claude-haiku-4",
        "sonnet" => "claude-sonnet-4",
        "opus" => "claude-opus-4",
        _ => "",
    }
}

/// Look a tier's (in, out) price up by prefix-match (a model id matches the first row whose
/// prefix it starts with — the bash capture-cost convention).
fn price_of(prices: &HashMap<String, (f64, f64)>, tier: &str) -> Option<(f64, f64)> {
    let pref = tier_prefix(tier);
    if pref.is_empty() {
        return None;
    }
    prices.get(pref).copied().or_else(|| {
        prices
            .iter()
            .find(|(k, _)| pref.starts_with(k.as_str()) || k.starts_with(pref))
            .map(|(_, v)| *v)
    })
}

/// cognition_class → (tier, model). `escalate` bumps discernment to opus (false-PASS propagates).
fn route_class(class: &str, escalate: bool) -> (&'static str, Option<&'static str>) {
    match class {
        "determinative" => ("none", None),
        "mechanical" => ("haiku", Some("claude-haiku-4")),
        "discernment" => {
            if escalate {
                ("opus", Some("claude-opus-4"))
            } else {
                ("sonnet", Some("claude-sonnet-4"))
            }
        }
        "thought-intensive" => ("opus", Some("claude-opus-4")),
        _ => ("sonnet", Some("claude-sonnet-4")), // unknown floor → safe middle
    }
}

/// USD for `tokens` at `tier`, split input/output by `in_frac`. jq-style shortest render.
fn cost_at(
    prices: &HashMap<String, (f64, f64)>,
    tier: &str,
    tokens: i64,
    in_frac: f64,
) -> Option<f64> {
    let (pin, pout) = price_of(prices, tier)?;
    let intok = tokens as f64 * in_frac;
    let outtok = tokens as f64 * (1.0 - in_frac);
    Some((intok * pin + outtok * pout) / 1_000_000.0)
}

fn usd(x: f64) -> String {
    // 4 significant cents — round to 4 decimals, render shortest (jq-compatible).
    fmt::shortest(fmt::fixed(x, 4).parse::<f64>().unwrap_or(x))
}

pub fn route(argv: &[String]) -> Out {
    // Parse flags (reuse the same lenient scheme).
    let mut flags: HashMap<String, String> = HashMap::new();
    let mut i = 0;
    while i < argv.len() {
        if let Some(rest) = argv[i].strip_prefix("--") {
            if let Some((k, v)) = rest.split_once('=') {
                flags.insert(k.into(), v.into());
                i += 1;
            } else if i + 1 < argv.len() && !argv[i + 1].starts_with("--") {
                flags.insert(rest.into(), argv[i + 1].clone());
                i += 2;
            } else {
                flags.insert(rest.into(), String::new());
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    let cognition = flags
        .get("cognition")
        .cloned()
        .unwrap_or_else(|| "discernment".into());
    let escalate = flags.contains_key("escalate");
    let in_frac: f64 = flags
        .get("in-frac")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.7);
    let prices = load_prices(flags.get("prices").map(|s| s.as_str()));

    // Token band straight from the estimator (already scaled by ratio_ewma(profile)).
    let est = estimate::estimate(estimate::Args {
        profile_path: flags.get("profile").map(|s| s.as_str()),
        width: flags.get("width").map(|s| s.as_str()),
        name: flags.get("name").map(|s| s.as_str()),
        measured: flags.get("measured-unit-tokens").map(|s| s.as_str()),
        history: flags.get("history-tokens").map(|s| s.as_str()),
        class: flags.get("class").map(|s| s.as_str()),
    });
    let est_out = est.stdout.trim_end();
    let ev: Value = serde_json::from_str(est_out).unwrap_or(Value::Null);
    let est_total = ev.get("est_total").and_then(|x| x.as_i64()).unwrap_or(0);
    let (lo, hi) = ev
        .get("interval")
        .and_then(|x| x.as_array())
        .map(|a| {
            (
                a.first().and_then(|x| x.as_i64()).unwrap_or(0),
                a.get(1).and_then(|x| x.as_i64()).unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));
    let name = ev.get("name").and_then(|x| x.as_str()).unwrap_or("unnamed");

    let (tier, model) = route_class(&cognition, escalate);

    // Determinative units leave the token economy entirely.
    if tier == "none" {
        let line = format!(
            "{{\"name\":\"{}\",\"cognition_class\":\"{}\",\"best_fit_tier\":\"none\",\"model\":null,\"est_total\":0,\"cost_usd\":0,\"note\":\"determinative_handler — 0 model tokens; runs as a tested tf/client handler\"}}\n",
            name, cognition
        );
        return Out::ok(line);
    }

    let best_cost = cost_at(&prices, tier, est_total, in_frac).unwrap_or(0.0);
    let lo_cost = cost_at(&prices, tier, lo, in_frac).unwrap_or(0.0);
    let hi_cost = cost_at(&prices, tier, hi, in_frac).unwrap_or(0.0);

    // Per-tier comparison band, so the banner can show "≈ $X sonnet vs $Y haiku".
    let per = |t: &str| {
        cost_at(&prices, t, est_total, in_frac)
            .map(usd)
            .unwrap_or_else(|| "null".into())
    };

    let model_s = model.unwrap_or("null");
    let line = format!(
        "{{\"name\":\"{}\",\"cognition_class\":\"{}\",\"best_fit_tier\":\"{}\",\"model\":\"{}\",\"est_total\":{},\"interval\":[{},{}],\"cost_usd\":{},\"cost_band\":[{},{}],\"per_tier_usd\":{{\"haiku\":{},\"sonnet\":{},\"opus\":{}}},\"in_frac\":{}}}\n",
        name, cognition, tier, model_s, est_total, lo, hi,
        usd(best_cost), usd(lo_cost), usd(hi_cost),
        per("haiku"), per("sonnet"), per("opus"), fmt::shortest(in_frac)
    );
    Out::ok(line)
}
