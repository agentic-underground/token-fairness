//! ceiling — L1 LIVE CEILING GUARD. Port of `ceiling-check.sh`.
//!
//! Reads the live rate-limit payload (the same `.rate_limits.*.used_percentage` the
//! status line renders), compares each requested rolling window against `100 - headroom`,
//! and FAILS CLOSED: any missing/unparseable number is NO_SIGNAL (treated as HALT).
//!
//!   exit 0  CLEAR · exit 10 HALT · exit 20 NO_SIGNAL/bad-headroom
//!
//! Number tokens are lifted raw from the payload text (not reparsed), matching jq's
//! literal-preserving passthrough so `used_pct`/`resets_at` echo exactly what arrived.

use crate::Out;

/// Pull a raw numeric token for `rate_limits.<window>.<field>` from the payload text.
/// `allow_dot` includes `.` (percentages) vs integers only (epochs). Mirrors the bash
/// extractor: isolate the window object `{...}`, then the field's number.
fn extract(payload: &str, window: &str, field: &str, allow_dot: bool) -> Option<String> {
    let wkey = format!("\"{}\"", window);
    let wpos = payload.find(&wkey)?;
    let after = &payload[wpos + wkey.len()..];
    let open = after.find('{')?;
    let close = after[open..].find('}')? + open;
    let obj = &after[open + 1..close];

    let fkey = format!("\"{}\"", field);
    let fpos = obj.find(&fkey)?;
    let rest = &obj[fpos + fkey.len()..];
    let colon = rest.find(':')?;
    let val = rest[colon + 1..].trim_start();
    let tok: String = val
        .chars()
        .take_while(|c| c.is_ascii_digit() || (allow_dot && *c == '.'))
        .collect();
    if tok.is_empty() {
        None
    } else {
        Some(tok)
    }
}

/// jq-style field formatting in the verdict line: null when absent, quoted when
/// non-numeric, bare otherwise.
fn fmt_field(s: &Option<String>) -> String {
    match s {
        None => "null".into(),
        Some(t) if t.is_empty() => "null".into(),
        Some(t) if t.chars().all(|c| c.is_ascii_digit() || c == '.') => t.clone(),
        Some(t) => format!("\"{}\"", t),
    }
}

#[derive(PartialEq, Clone, Copy)]
enum WinState {
    Clear,
    Halt,
    NoSignal,
}

/// `headroom`/`window` are the parsed CLI options; `payload` is stdin.
pub fn check(headroom: &str, window: &str, payload: &str) -> Out {
    // A malformed guard is a dangerous guard — fail closed.
    let hr: i64 = match headroom.parse() {
        Ok(n) if (0..=100).contains(&n) => n,
        _ => {
            return Out::line(
                format!(
                    "{{\"verdict\":\"NO_SIGNAL\",\"reason\":\"bad-headroom\",\"headroom\":\"{}\"}}\n",
                    headroom
                ),
                20,
            )
        }
    };
    let window = match window {
        "five_hour" | "seven_day" | "both" => window,
        _ => "both",
    };
    let ceiling = 100 - hr;

    let breaches = |pct: &str| -> bool {
        pct.parse::<f64>().map(|p| p >= ceiling as f64).unwrap_or(false)
    };

    let eval = |key: &str| -> (WinState, Option<String>, Option<String>) {
        let pct = extract(payload, key, "used_percentage", true);
        let reset = extract(payload, key, "resets_at", false);
        match &pct {
            None => (WinState::NoSignal, pct, reset),
            Some(p) => {
                // Must look like a number (digits and dots only).
                if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    (WinState::NoSignal, pct.clone(), reset)
                } else if breaches(p) {
                    (WinState::Halt, pct.clone(), reset)
                } else {
                    (WinState::Clear, pct.clone(), reset)
                }
            }
        }
    };

    let wants: &[&str] = match window {
        "five_hour" => &["five_hour"],
        "seven_day" => &["seven_day"],
        _ => &["five_hour", "seven_day"],
    };

    // Escalate to the worst verdict seen; NO_SIGNAL > HALT > CLEAR.
    let mut worst = WinState::Clear;
    let mut hit_window: Option<String> = None;
    let mut hit_pct: Option<String> = None;
    let mut hit_reset: Option<String> = None;

    for key in wants {
        let (st, pct, reset) = eval(key);
        match st {
            WinState::NoSignal => {
                worst = WinState::NoSignal;
                hit_window = Some((*key).into());
                hit_pct = pct;
                hit_reset = reset;
            }
            WinState::Halt => {
                if worst != WinState::NoSignal {
                    worst = WinState::Halt;
                    hit_window = Some((*key).into());
                    hit_pct = pct;
                    hit_reset = reset;
                }
            }
            WinState::Clear => {
                if worst == WinState::Clear && hit_window.is_none() {
                    hit_window = Some((*key).into());
                    hit_pct = pct;
                    hit_reset = reset;
                }
            }
        }
    }

    let (verdict, code) = match worst {
        WinState::Clear => ("CLEAR", 0),
        WinState::Halt => ("HALT", 10),
        WinState::NoSignal => ("NO_SIGNAL", 20),
    };

    let line = format!(
        "{{\"verdict\":\"{}\",\"window\":\"{}\",\"used_pct\":{},\"ceiling\":{},\"headroom\":{},\"resets_at\":{}}}\n",
        verdict,
        hit_window.unwrap_or_else(|| "none".into()),
        fmt_field(&hit_pct),
        ceiling,
        hr,
        fmt_field(&hit_reset)
    );
    Out::line(line, code)
}
