//! offpeak — L3 OFF-PEAK CLOCK. Port of `offpeak-window.sh` (and, below, the budget calc).
//!
//! Pure deterministic time arithmetic in epoch+offset seconds-of-day, so it is DST-correct
//! for the given instant and fully testable (tests pass an explicit `--tz-offset-min`).
//! Off-peak defaults to 22:00–08:00 and wraps midnight, handled explicitly.

use crate::Out;

/// `date +%z` (±HHMM) → minutes east of UTC. Only used when the caller omits the offset.
fn machine_tzoff() -> i64 {
    if let Ok(out) = std::process::Command::new("date").arg("+%z").output() {
        let z = String::from_utf8_lossy(&out.stdout);
        let z = z.trim();
        let b = z.as_bytes();
        if b.len() == 5 && (b[0] == b'+' || b[0] == b'-') {
            if let (Ok(h), Ok(m)) = (z[1..3].parse::<i64>(), z[3..5].parse::<i64>()) {
                let v = h * 60 + m;
                return if b[0] == b'-' { -v } else { v };
            }
        }
    }
    0
}

/// "HH:MM" → seconds-of-day, or `None` if malformed (caller substitutes a default).
fn hm_to_sec(s: &str) -> Option<i64> {
    let (h, m) = s.split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    if !h.bytes().all(|b| b.is_ascii_digit()) || !m.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60)
}

fn local_sod(epoch: i64, tzoff: i64) -> i64 {
    let e = epoch + tzoff * 60;
    let mut s = e % 86400;
    if s < 0 {
        s += 86400;
    }
    s
}

fn in_window(s: i64, start: i64, end: i64) -> bool {
    if start <= end {
        s >= start && s < end
    } else {
        s >= start || s < end
    }
}

pub struct WindowArgs<'a> {
    pub now: &'a str,
    pub start: &'a str,
    pub end: &'a str,
    pub reset: Option<&'a str>,
    pub tz_offset_min: Option<&'a str>,
}

pub fn window(a: WindowArgs) -> Out {
    if a.now.is_empty() || !a.now.bytes().all(|b| b.is_ascii_digit()) {
        return Out::line("{\"error\":\"--now EPOCH required\"}\n", 2);
    }
    let now: i64 = a.now.parse().unwrap();

    let tzoff: i64 = match a.tz_offset_min {
        Some(t) if !t.is_empty() && t != "-" => {
            t.parse().unwrap_or_else(|_| machine_tzoff_or_zero(t))
        }
        _ => machine_tzoff(),
    };

    let start_sec = hm_to_sec(a.start).unwrap_or(79200); // 22:00
    let end_sec = hm_to_sec(a.end).unwrap_or(28800); // 08:00

    let now_sod = local_sod(now, tzoff);
    let in_offpeak = in_window(now_sod, start_sec, end_sec);

    let mins_to_offpeak = if in_offpeak {
        0
    } else {
        ((start_sec - now_sod + 86400) % 86400) / 60
    };

    let (mins_to_reset, reset_in_window): (String, String) = match a.reset {
        Some(r) if !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()) => {
            let reset: i64 = r.parse().unwrap();
            let mtr = (reset - now) / 60;
            let rsod = local_sod(reset, tzoff);
            let riw = in_window(rsod, start_sec, end_sec);
            (
                mtr.to_string(),
                if riw { "true".into() } else { "false".into() },
            )
        }
        _ => ("null".into(), "null".into()),
    };

    let hh = now_sod / 3600;
    let mm = (now_sod % 3600) / 60;

    let line = format!(
        "{{\"in_offpeak\":{},\"minutes_to_offpeak\":{},\"minutes_to_reset\":{},\"reset_in_window\":{},\"local_hhmm\":\"{:02}:{:02}\"}}\n",
        in_offpeak, mins_to_offpeak, mins_to_reset, reset_in_window, hh, mm
    );
    Out::ok(line)
}

// If an offset was supplied but unparseable, bash falls back to 0 (not the machine).
fn machine_tzoff_or_zero(_raw: &str) -> i64 {
    0
}

// ── offpeak-budget: the overnight budget calculator ──────────────────────────────────

fn int_or(s: &str, default: i64) -> i64 {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().unwrap()
    } else {
        default
    }
}

pub struct BudgetArgs<'a> {
    pub now: &'a str,
    pub login: &'a str,
    pub reset: &'a str,
    pub headroom: &'a str,
    pub reserve: &'a str,
    pub window_hours: &'a str,
}

/// Port of `offpeak-budget.sh`. From "what time will you log in tomorrow?" compute a
/// per-window spend ceiling: windows that fully reset before login may run to
/// `100 - headroom`; the window inherited AT login is held to `100 - reserve`.
pub fn budget(a: BudgetArgs) -> Out {
    let is_int = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    for (label, val) in [("now", a.now), ("login", a.login), ("reset", a.reset)] {
        if !is_int(val) {
            return Out::line(format!("{{\"error\":\"--{} EPOCH required\"}}\n", label), 2);
        }
    }
    let now: i64 = a.now.parse().unwrap();
    let login: i64 = a.login.parse().unwrap();
    let reset: i64 = a.reset.parse().unwrap();

    let mut headroom = int_or(a.headroom, 15);
    let mut reserve = int_or(a.reserve, 60);
    let mut wh = int_or(a.window_hours, 5);
    if wh <= 0 {
        wh = 5;
    }
    if headroom > 100 {
        headroom = 15;
    }
    if reserve > 100 {
        reserve = 60;
    }

    let w = wh * 3600;
    let unatt_ceiling = 100 - headroom;
    let login_ceiling = 100 - reserve;

    let lwi = if login <= reset {
        0
    } else {
        let diff = login - reset;
        (diff + w - 1) / w // ceil(diff / W)
    };

    const MAXW: i64 = 50;
    let mut truncated = "false";
    let mut last_idx = lwi;
    if last_idx > MAXW {
        last_idx = MAXW;
        truncated = "true";
    }

    let mut windows = String::from("[");
    let mut i = 0;
    while i <= last_idx {
        let ends_at = reset + i * w;
        let (role, ceil, hr) = if i == lwi {
            ("login", login_ceiling, reserve)
        } else {
            ("unattended", unatt_ceiling, headroom)
        };
        if i > 0 {
            windows.push(',');
        }
        windows.push_str(&format!(
            "{{\"index\":{},\"ends_at\":{},\"role\":\"{}\",\"ceiling_pct\":{},\"headroom\":{}}}",
            i, ends_at, role, ceil, hr
        ));
        i += 1;
    }
    windows.push(']');

    let current_headroom = if lwi == 0 { reserve } else { headroom };

    let line = format!(
        "{{\"now\":{},\"login\":{},\"reset\":{},\"window_hours\":{},\"login_window_index\":{},\"unattended_windows\":{},\"current_headroom\":{},\"truncated\":{},\"windows\":{}}}\n",
        now, login, reset, wh, lwi, lwi, current_headroom, truncated, windows
    );
    Out::ok(line)
}
