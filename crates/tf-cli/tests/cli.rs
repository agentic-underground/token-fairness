//! Self-contained conformance: drive the built `tf` binary and assert byte-exact output and
//! exit code against vectors frozen from the original bash scheduler. No bash needed at test
//! time, so this is the CI gate in the standalone repo. The live differential proof against
//! the bash original lives in `tests/conformance.sh`.

use std::io::Write;
use std::process::{Command, Stdio};

fn tf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tf"))
}

/// Run tf with args + stdin, return (stdout, exit_code).
fn run(args: &[&str], stdin: &str, envs: &[(&str, &str)]) -> (String, i32) {
    let mut cmd = tf();
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

fn assert_line(args: &[&str], stdin: &str, envs: &[(&str, &str)], want: &str, code: i32) {
    let (got, rc) = run(args, stdin, envs);
    assert_eq!(got.trim_end_matches('\n'), want, "args={:?}", args);
    assert_eq!(rc, code, "exit code for args={:?}", args);
}

const P_CLEAR: &str = r#"{"rate_limits":{"five_hour":{"used_percentage":42.5,"resets_at":1749635640},"seven_day":{"used_percentage":18.2,"resets_at":1750000000}}}"#;
const P_HALT: &str = r#"{"rate_limits":{"five_hour":{"used_percentage":86,"resets_at":1749635640},"seven_day":{"used_percentage":18.2,"resets_at":1750000000}}}"#;

#[test]
fn ceiling_check_vectors() {
    assert_line(
        &["ceiling-check"],
        P_CLEAR,
        &[],
        r#"{"verdict":"CLEAR","window":"five_hour","used_pct":42.5,"ceiling":85,"headroom":15,"resets_at":1749635640}"#,
        0,
    );
    assert_line(
        &["ceiling-check"],
        P_HALT,
        &[],
        r#"{"verdict":"HALT","window":"five_hour","used_pct":86,"ceiling":85,"headroom":15,"resets_at":1749635640}"#,
        10,
    );
    assert_line(
        &["ceiling-check"],
        "{}",
        &[],
        r#"{"verdict":"NO_SIGNAL","window":"seven_day","used_pct":null,"ceiling":85,"headroom":15,"resets_at":null}"#,
        20,
    );
    assert_line(
        &["ceiling-check", "--headroom", "200"],
        P_CLEAR,
        &[],
        r#"{"verdict":"NO_SIGNAL","reason":"bad-headroom","headroom":"200"}"#,
        20,
    );
}

#[test]
fn offpeak_window_vectors() {
    assert_line(
        &[
            "offpeak-window",
            "--now",
            "1700000000",
            "--tz-offset-min",
            "-420",
        ],
        "",
        &[],
        r#"{"in_offpeak":false,"minutes_to_offpeak":406,"minutes_to_reset":null,"reset_in_window":null,"local_hhmm":"15:13"}"#,
        0,
    );
    assert_line(
        &[
            "offpeak-window",
            "--now",
            "1700000000",
            "--reset",
            "1700003600",
            "--tz-offset-min",
            "-420",
        ],
        "",
        &[],
        r#"{"in_offpeak":false,"minutes_to_offpeak":406,"minutes_to_reset":60,"reset_in_window":false,"local_hhmm":"15:13"}"#,
        0,
    );
    assert_line(
        &["offpeak-window", "--tz-offset-min", "0"],
        "",
        &[],
        r#"{"error":"--now EPOCH required"}"#,
        2,
    );
}

#[test]
fn offpeak_budget_vectors() {
    assert_line(
        &[
            "offpeak-budget",
            "--now",
            "1700000000",
            "--login",
            "1700054000",
            "--reset",
            "1700000000",
        ],
        "",
        &[],
        r#"{"now":1700000000,"login":1700054000,"reset":1700000000,"window_hours":5,"login_window_index":3,"unattended_windows":3,"current_headroom":15,"truncated":false,"windows":[{"index":0,"ends_at":1700000000,"role":"unattended","ceiling_pct":85,"headroom":15},{"index":1,"ends_at":1700018000,"role":"unattended","ceiling_pct":85,"headroom":15},{"index":2,"ends_at":1700036000,"role":"unattended","ceiling_pct":85,"headroom":15},{"index":3,"ends_at":1700054000,"role":"login","ceiling_pct":40,"headroom":60}]}"#,
        0,
    );
}

#[test]
fn estimate_vectors() {
    assert_line(
        &["estimate", "--class", "large"],
        "",
        &[],
        r#"{"name":"plan:large","per_unit":250000,"basis":"class","confidence":"low","fanout":1,"ratio":1.0,"est_total":250000,"convergence":{"samples":0,"mean_ratio":1.0000,"sd":0.0000,"p95_band_pct":60.0,"tier":"SEEDING","prev_band":-1.0,"trend":"flat"},"interval":[100000,400000]}"#,
        0,
    );
    assert_line(
        &["estimate", "--name", "brandnewthing", "--width", "4"],
        "",
        &[],
        r#"{"name":"brandnewthing","per_unit":20000,"basis":"seed","confidence":"low","fanout":4,"ratio":1.0,"est_total":80000,"convergence":{"samples":0,"mean_ratio":1.0000,"sd":0.0000,"p95_band_pct":60.0,"tier":"SEEDING","prev_band":-1.0,"trend":"flat"},"interval":[32000,128000]}"#,
        0,
    );
}

#[test]
fn calibrate_sequence() {
    // Isolated calibration file per test run.
    let f = std::env::temp_dir().join(format!("tf-cal-test-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&f);
    let fp = f.to_str().unwrap();
    let env = [("I2P_CALIBRATION_FILE", fp)];

    assert_line(&["calibrate", "ratio", "foo"], "", &env, "1.0", 0);
    assert_line(
        &["calibrate", "close", "foo", "100000", "120000"],
        "",
        &env,
        "1.2000",
        0,
    );
    assert_line(&["calibrate", "ratio", "foo"], "", &env, "1.2000", 0);
    assert_line(
        &["calibrate", "confidence", "foo"],
        "",
        &env,
        r#"{"samples":1,"mean_ratio":1.2000,"sd":0.0000,"p95_band_pct":50.0,"tier":"CALIBRATING","prev_band":60.0,"trend":"improving"}"#,
        0,
    );
    let _ = std::fs::remove_file(&f);
}

/// The SOLID convergence covenant, "continuous improvement made visible": as estimate↔actual
/// samples accrue, the p95 band tightens DRAMATICALLY and the tier walks
/// SEEDING→CALIBRATING→CONVERGING→CONVERGED. (The band can wiggle in the noisy tail — variance,
/// not regression — so we assert the dramatic tightening + the tier progression, not strict
/// monotonicity.)
#[test]
fn convergence_band_tightens_over_samples() {
    let f = std::env::temp_dir().join(format!("tf-conv-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&f);
    let fp = f.to_str().unwrap();
    let env = [("I2P_CALIBRATION_FILE", fp)];

    let close_one = |i: i64| {
        let actual = (100000 + i * 400).to_string();
        run(
            &["calibrate", "close", "grind", "100000", &actual],
            "",
            &env,
        );
    };
    let band_tier = || -> (f64, String) {
        let (c, _) = run(&["calibrate", "confidence", "grind"], "", &env);
        let v: serde_json::Value = serde_json::from_str(c.trim()).unwrap();
        (
            v["p95_band_pct"].as_f64().unwrap(),
            v["tier"].as_str().unwrap().to_string(),
        )
    };

    close_one(1);
    let (band1, tier1) = band_tier();
    for i in 2..=6 {
        close_one(i);
    }
    let (band6, tier6) = band_tier();
    for i in 7..=12 {
        close_one(i);
    }
    let (band12, tier12) = band_tier();

    assert!(band1 >= 40.0, "early band is wide (SEEDING-ish): {band1}");
    assert_eq!(tier1, "CALIBRATING");
    assert!(band6 < band1, "band tightened: {band6} < {band1}");
    assert_eq!(tier6, "CONVERGING");
    assert_eq!(tier12, "CONVERGED");
    assert!(band12 <= 15.0, "converged band is tight: {band12}");
    let _ = std::fs::remove_file(&f);
}

/// Fail-closed (the non-negotiable): a partial payload — a window object present but with no
/// `used_percentage` — is NO_SIGNAL / exit 20, never a silent CLEAR.
#[test]
fn ceiling_partial_payload_fails_closed() {
    assert_line(
        &["ceiling-check", "--window", "five_hour"],
        r#"{"rate_limits":{"five_hour":{"resets_at":1749635640}}}"#,
        &[],
        r#"{"verdict":"NO_SIGNAL","window":"five_hour","used_pct":null,"ceiling":85,"headroom":15,"resets_at":1749635640}"#,
        20,
    );
}

/// The morning reserve (L3): windows that fully reset before login may run to `100 − headroom`
/// (85%), but the window the user INHERITS at login is held to `100 − morning_reserve` (here 40%),
/// so they wake to a usable allowance.
#[test]
fn offpeak_budget_morning_reserve() {
    let (got, rc) = run(
        &[
            "offpeak-budget",
            "--now",
            "1700000000",
            "--login",
            "1700054000",
            "--reset",
            "1700000000",
            "--morning-reserve",
            "60",
            "--headroom",
            "15",
        ],
        "",
        &[],
    );
    assert_eq!(rc, 0);
    let v: serde_json::Value = serde_json::from_str(got.trim()).unwrap();
    let windows = v["windows"].as_array().unwrap();
    for w in windows {
        let role = w["role"].as_str().unwrap();
        let ceil = w["ceiling_pct"].as_i64().unwrap();
        if role == "login" {
            assert_eq!(ceil, 40, "login window held to 100 - reserve(60)");
        } else {
            assert_eq!(ceil, 85, "unattended window runs to 100 - headroom(15)");
        }
    }
    // and the login window is the one the user inherits (index == login_window_index)
    assert_eq!(v["login_window_index"], 3);
}
