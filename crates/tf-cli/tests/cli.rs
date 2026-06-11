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
