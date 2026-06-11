//! scheduler — the THIN DISPATCHER. Port of `scheduler.sh` (+ `preflight-fanout.sh`).
//!
//! Composes the pure helpers (estimate / ceiling / offpeak / calibrate) into one verdict and
//! names it; owns no arithmetic. Verdicts: CONTINUE · PROBE · DEFER · HALT · ASK. The gate is
//! the load-bearing seam — payload on stdin, fresh-snapshot fallback, fail-closed to ASK.

use crate::{calibrate, ceiling, estimate, fmt, offpeak, state, Out};
use serde_json::Value;
use std::collections::HashMap;

const DEFER_THRESHOLD: i64 = 150000;

/// Lenient flag/positional parse identical to the CLI front door (`--flag v` / `--flag=v`).
fn parse(argv: &[String]) -> (HashMap<String, String>, Vec<String>) {
    let mut flags = HashMap::new();
    let mut pos = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if let Some(rest) = a.strip_prefix("--") {
            if let Some((k, v)) = rest.split_once('=') {
                flags.insert(k.to_string(), v.to_string());
                i += 1;
            } else if i + 1 < argv.len() && !argv[i + 1].starts_with("--") {
                flags.insert(rest.to_string(), argv[i + 1].clone());
                i += 2;
            } else {
                flags.insert(rest.to_string(), String::new());
                i += 1;
            }
        } else {
            pos.push(a.clone());
            i += 1;
        }
    }
    (flags, pos)
}

fn est_args(f: &HashMap<String, String>) -> estimate::Args<'_> {
    estimate::Args {
        profile_path: f.get("profile").map(|s| s.as_str()),
        width: f.get("width").map(|s| s.as_str()),
        name: f.get("name").map(|s| s.as_str()),
        measured: f.get("measured-unit-tokens").map(|s| s.as_str()),
        history: f.get("history-tokens").map(|s| s.as_str()),
        class: f.get("class").map(|s| s.as_str()),
    }
}

/// fmt_tok — awk: ≥1M "%.1fM"; ≥1000 "%dk" (round half up); else "%d".
fn fmt_tok(t: i64) -> String {
    let tf = t as f64;
    if tf >= 1_000_000.0 {
        format!("{}M", fmt::fixed(tf / 1_000_000.0, 1))
    } else if tf >= 1000.0 {
        format!("{}k", fmt::round_i64(tf / 1000.0))
    } else {
        format!("{}", t)
    }
}

fn has_live_signal(payload: &str) -> bool {
    let v: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
    v.pointer("/rate_limits/five_hour/used_percentage")
        .filter(|x| !x.is_null())
        .is_some()
        || v.pointer("/rate_limits/seven_day/used_percentage")
            .filter(|x| !x.is_null())
            .is_some()
}

// ── preflight ──────────────────────────────────────────────────────────────────────────
pub fn preflight(argv: &[String]) -> Out {
    let (flags, _) = parse(argv);
    let est = estimate::estimate(est_args(&flags));
    let est_out = est.stdout.trim_end();
    let conf = state::raw_field(est_out, "confidence");
    let verdict = if conf == "high" { "CONTINUE" } else { "PROBE" };
    let body = if est_out.is_empty() { "null" } else { est_out };
    let code = if verdict == "CONTINUE" { 0 } else { 3 };
    Out::line(
        format!("{{\"verdict\":\"{}\",\"estimate\":{}}}\n", verdict, body),
        code,
    )
}

// ── plan ───────────────────────────────────────────────────────────────────────────────
pub fn plan(argv: &[String]) -> Out {
    let (flags, _) = parse(argv);
    let now = flags.get("now").cloned().unwrap_or_default();
    let start = flags
        .get("start")
        .cloned()
        .unwrap_or_else(|| "22:00".into());
    let end = flags.get("end").cloned().unwrap_or_else(|| "08:00".into());
    let tz = flags.get("tz-offset-min").cloned().unwrap_or_default();

    let est = estimate::estimate(est_args(&flags));
    let est_out = est.stdout.trim_end();

    let et = state::raw_field(est_out, "est_total");
    let est_total: i64 = if !et.is_empty() && et.bytes().all(|b| b.is_ascii_digit()) {
        et.parse().unwrap_or(0)
    } else {
        0
    };
    let band = {
        let b = state::raw_field(est_out, "p95_band_pct");
        if b.is_empty() {
            "60".to_string()
        } else {
            b
        }
    };
    let tier = {
        let t = state::raw_field(est_out, "tier");
        if t.is_empty() {
            "SEEDING".to_string()
        } else {
            t
        }
    };
    let samples = {
        let s = state::raw_field(est_out, "samples");
        if s.is_empty() {
            "0".to_string()
        } else {
            s
        }
    };
    let pname = {
        let n = state::raw_field(est_out, "name");
        if n.is_empty() {
            "plan".to_string()
        } else {
            n
        }
    };

    let mut decision = "RUN NOW";
    let mut in_offpeak = "n/a".to_string();
    if !now.is_empty() {
        let ow = offpeak::window(offpeak::WindowArgs {
            now: &now,
            start: &start,
            end: &end,
            reset: None,
            tz_offset_min: if tz.is_empty() {
                None
            } else {
                Some(tz.as_str())
            },
        });
        in_offpeak = state::raw_field(ow.stdout.trim_end(), "in_offpeak");
        if in_offpeak.is_empty() {
            in_offpeak = "false".into();
        }
        if in_offpeak != "true" && est_total >= DEFER_THRESHOLD {
            decision = "DEFER";
        }
    }

    let band_disp = fmt::fixed(band.parse::<f64>().unwrap_or(0.0), 0);
    let mut s = String::new();
    s.push_str(&format!(
        "💰 ~{} tokens · p95 ±{}% · {} ({} samples)\n",
        fmt_tok(est_total),
        band_disp,
        tier,
        samples
    ));
    if decision == "DEFER" {
        s.push_str(&format!(
            "🕒 Schedule: DEFER → off-peak {}–{} (now is peak; est is large)\n",
            start, end
        ));
    } else {
        let suffix = if in_offpeak == "true" {
            " (off-peak)"
        } else {
            ""
        };
        s.push_str(&format!("🕒 Schedule: RUN NOW{}\n", suffix));
    }
    s.push_str(&format!(
        "{{\"name\":\"{}\",\"est_total\":{},\"p95_band_pct\":{},\"tier\":\"{}\",\"samples\":{},\"decision\":\"{}\",\"in_offpeak\":\"{}\"}}\n",
        pname, est_total, band, tier, samples, decision, in_offpeak
    ));
    let code = if decision == "DEFER" { 4 } else { 0 };
    Out::line(s, code)
}

// ── plan-open ──────────────────────────────────────────────────────────────────────────
fn session_file() -> String {
    std::env::var("I2P_SESSION_FILE")
        .unwrap_or_else(|_| format!("{}/session.json", state::home_cost_dir()))
}
fn planopen_file() -> String {
    std::env::var("I2P_PLANOPEN_FILE")
        .unwrap_or_else(|_| format!("{}/plan-open.json", state::home_cost_dir()))
}
fn session_tokens() -> i64 {
    state::read_json(&session_file())
        .and_then(|v| v.get("tokens").and_then(|x| x.as_i64()))
        .unwrap_or(0)
}

pub fn plan_open(argv: &[String]) -> Out {
    let (_, pos) = parse(argv);
    let class = pos.first().map(|s| s.as_str()).unwrap_or("");
    if class.is_empty() {
        return Out::err("scheduler: plan-open <class> <est>", 2);
    }
    let est = state::digits_or(pos.get(1).map(|s| s.as_str()).unwrap_or(""), 0);
    let base = session_tokens();
    let popen = planopen_file();
    let _ = state::write_json(
        &popen,
        &serde_json::json!({ "class": class, "est": est, "baseline_tokens": base }),
    );
    Out::ok(format!(
        "{{\"opened\":\"plan:{}\",\"est\":{},\"baseline_tokens\":{}}}\n",
        class, est, base
    ))
}

pub fn plan_close(_argv: &[String]) -> Out {
    let popen = planopen_file();
    let pv = match state::read_json(&popen) {
        Some(v) => v,
        None => return Out::line("{\"error\":\"no-open-plan\"}\n", 2),
    };
    let pclass = pv
        .get("class")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let pest = pv.get("est").and_then(|x| x.as_i64()).unwrap_or(0).max(0);
    let base = pv
        .get("baseline_tokens")
        .and_then(|x| x.as_i64())
        .unwrap_or(0);
    let cur = session_tokens();
    let mut actual = cur - base;
    if actual < 0 {
        actual = 0;
    }
    // §3.4: convergence dies silently if the session-token writer never ran. Warn, don't hide it.
    if base == 0 && cur == 0 {
        eprintln!("scheduler: plan-close sees baseline==current==0 — the session.json .tokens writer may not be installed; convergence cannot advance.");
    }
    let conv = if pest > 0 && actual > 0 {
        let name = format!("plan:{}", pclass);
        let _ = calibrate::close(&name, &pest.to_string(), &actual.to_string());
        calibrate::confidence_string(&name)
    } else {
        "null".to_string()
    };
    let _ = std::fs::remove_file(&popen);
    Out::ok(format!(
        "{{\"class\":\"plan:{}\",\"est\":{},\"actual\":{},\"convergence\":{}}}\n",
        pclass, pest, actual, conv
    ))
}

// ── gate ───────────────────────────────────────────────────────────────────────────────
fn snapshot_path() -> String {
    std::env::var("I2P_RATELIMIT_SNAPSHOT")
        .unwrap_or_else(|_| format!("{}/ratelimit-snapshot.json", state::state_dir()))
}

pub fn gate(argv: &[String], payload_in: &str) -> Out {
    let (flags, _) = parse(argv);
    let headroom = flags
        .get("headroom")
        .cloned()
        .unwrap_or_else(|| "15".into());
    let window = flags
        .get("window")
        .cloned()
        .unwrap_or_else(|| "both".into());
    let require_offpeak = flags.contains_key("require-offpeak");
    let now = flags.get("now").cloned().unwrap_or_default();
    let start = flags
        .get("start")
        .cloned()
        .unwrap_or_else(|| "22:00".into());
    let end = flags.get("end").cloned().unwrap_or_else(|| "08:00".into());
    let tz = flags.get("tz-offset-min").cloned().unwrap_or_default();
    let max_age = state::digits_or(
        flags
            .get("snapshot-max-age")
            .map(|s| s.as_str())
            .unwrap_or(""),
        900,
    );
    let clock = flags.get("clock").cloned().unwrap_or_default();

    let mut payload = payload_in.to_string();
    if !has_live_signal(&payload) {
        let snap = snapshot_path();
        if let Some(sv) = state::read_json(&snap) {
            let cap = sv.get("captured_at").and_then(|x| x.as_i64()).unwrap_or(0);
            let nowclk = state::digits_or(&clock, state::now_epoch());
            let age = nowclk - cap;
            if cap > 0 && age >= 0 && age <= max_age {
                if let Ok(s) = std::fs::read_to_string(&snap) {
                    payload = s;
                }
            }
        }
    }

    let ceil = ceiling::check(&headroom, &window, &payload);
    let ceil_json = ceil.stdout.trim_end();
    match ceil.code {
        10 => {
            return Out::line(
                format!("{{\"verdict\":\"HALT\",\"ceiling\":{}}}\n", ceil_json),
                10,
            )
        }
        20 => {
            return Out::line(
                format!(
                    "{{\"verdict\":\"ASK\",\"reason\":\"no-live-signal\",\"ceiling\":{}}}\n",
                    ceil_json
                ),
                20,
            )
        }
        _ => {}
    }

    let mut ow_json = "null".to_string();
    if require_offpeak && !now.is_empty() {
        let ow = offpeak::window(offpeak::WindowArgs {
            now: &now,
            start: &start,
            end: &end,
            reset: None,
            tz_offset_min: if tz.is_empty() {
                None
            } else {
                Some(tz.as_str())
            },
        });
        ow_json = ow.stdout.trim_end().to_string();
        if state::raw_field(&ow_json, "in_offpeak") != "true" {
            return Out::line(
                format!(
                    "{{\"verdict\":\"DEFER\",\"ceiling\":{},\"offpeak\":{}}}\n",
                    ceil_json, ow_json
                ),
                4,
            );
        }
    }

    Out::line(
        format!(
            "{{\"verdict\":\"CONTINUE\",\"ceiling\":{},\"offpeak\":{}}}\n",
            ceil_json, ow_json
        ),
        0,
    )
}

// ── preflight-fanout (the PreToolUse hook) ───────────────────────────────────────────────
pub fn preflight_fanout(payload: &str) -> Out {
    if payload.trim().is_empty() {
        return Out::default();
    }
    let g = gate(&[], payload);
    if g.code == 10 {
        let mut pct = state::raw_field(g.stdout.trim_end(), "used_pct");
        if pct.is_empty() {
            pct = "?".to_string();
        }
        let reason = format!(
            "Token ceiling reached (live window at {}%). Spawning more agents now risks a lockout. Pause this job (job-ledger.sh pause) and resume when the window resets — /concierge:schedule.",
            pct
        );
        let deny = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason
            }
        });
        return Out::ok(serde_json::to_string(&deny).unwrap_or_default() + "\n");
    }
    Out::default()
}
