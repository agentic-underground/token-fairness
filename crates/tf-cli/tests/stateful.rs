//! Self-contained frozen-vector conformance for the STATEFUL + orchestration tier
//! (ledger / registry / snapshot / signal / report / gate / plan / preflight / oscron).
//! Vectors were captured from the bash oracle via `tests/conformance.sh` and frozen here so
//! `cargo test` is a standalone CI gate — no bash, no real crontab, no network.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn tf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tf"))
}

/// Unique isolated dir per (test, pid) — auto-namespaced so tests don't collide.
fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tf-stateful-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

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

fn line(args: &[&str], stdin: &str, envs: &[(&str, &str)], want: &str, code: i32) {
    let (got, rc) = run(args, stdin, envs);
    assert_eq!(got.trim_end_matches('\n'), want, "args={:?}", args);
    assert_eq!(rc, code, "exit for args={:?}", args);
}

#[test]
fn ledger_lifecycle() {
    let d = tmp("ledger");
    let ds = d.to_str().unwrap();
    line(
        &[
            "ledger", "init", ds, "j", "reviewer", "a,b,c", "500000", "15",
        ],
        "",
        &[],
        &format!("job-ledger: initialised {}/.i2p/jobs/j.json (3 units)", ds),
        0,
    );
    line(&["ledger", "mark-done", ds, "j", "b"], "", &[], "", 0);
    line(&["ledger", "remaining", ds, "j"], "", &[], "a\nc", 0);
    // status is the full ledger doc (pretty, jq-insertion-order); compare canonicalised.
    let (got, rc) = run(&["ledger", "status", ds, "j"], "", &[]);
    assert_eq!(rc, 0);
    let v: serde_json::Value = serde_json::from_str(&got).unwrap();
    assert_eq!(v["state"], "running");
    assert_eq!(v["units"]["done"], serde_json::json!(["b"]));
    assert_eq!(v["units"]["remaining"], serde_json::json!(["a", "c"]));
    assert_eq!(v["units"]["total"], 3);
    // missing ledger → exit 2
    let (_o, rc) = run(&["ledger", "status", ds, "ghost"], "", &[]);
    assert_eq!(rc, 2);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ledger_status_is_pretty_on_disk() {
    let d = tmp("ledgerdisk");
    let ds = d.to_str().unwrap();
    run(&["ledger", "init", ds, "j", "p", "x,y", "0", "15"], "", &[]);
    let f = d.join(".i2p/jobs/j.json");
    let body = std::fs::read_to_string(&f).unwrap();
    assert!(
        body.ends_with('\n'),
        "state file ends with newline (jq parity)"
    );
    assert!(
        body.contains("  \"job_id\": \"j\""),
        "2-space pretty-print like jq"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn registry_dual_scope() {
    let d = tmp("registry");
    let ds = d.to_str().unwrap();
    let machine = d.join("machine.json");
    let env = [("I2P_MACHINE_REGISTRY", machine.to_str().unwrap())];
    line(
        &[
            "registry",
            "register",
            ds,
            "j1",
            "17 22 * * *",
            "300000",
            "./l.json",
            "./p.txt",
            "note",
        ],
        "",
        &env,
        "jobs-registry: registered j1 (project + machine index)",
        0,
    );
    line(
        &["registry", "list", ds],
        "",
        &env,
        r#"[{"id":"j1","cron":"17 22 * * *","recurring":true,"budget_total":300000,"ledger":"./l.json","prompt_file":"./p.txt","note":"note","armed":false}]"#,
        0,
    );
    line(
        &["registry", "get", ds, "j1"],
        "",
        &env,
        r#"{"id":"j1","cron":"17 22 * * *","recurring":true,"budget_total":300000,"ledger":"./l.json","prompt_file":"./p.txt","note":"note","armed":false}"#,
        0,
    );
    line(&["registry", "get", ds, "nope"], "", &env, "{}", 0);
    // arm oscron then reset-armed keeps it (durable); session arming would be cleared.
    run(&["registry", "arm", ds, "j1", "oscron"], "", &env);
    run(&["registry", "reset-armed", ds], "", &env);
    let (got, _) = run(&["registry", "get", ds, "j1"], "", &env);
    assert!(got.contains(r#""armed":true"#) && got.contains(r#""armed_via":"oscron""#));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn snapshot_only_on_signal() {
    let d = tmp("snapshot");
    let env = [
        ("I2P_COST_STATE_DIR", d.to_str().unwrap()),
        ("I2P_CLOCK", "1700000000"),
    ];
    // no rate_limits → no-op, no file
    run(&["snapshot"], r#"{"hello":1}"#, &env);
    assert!(!d.join("ratelimit-snapshot.json").exists());
    // with signal → writes the pinned-clock snapshot
    run(
        &["snapshot"],
        r#"{"hook_event_name":"PostToolUse","rate_limits":{"five_hour":{"used_percentage":42,"resets_at":1749635640}}}"#,
        &env,
    );
    let snap = std::fs::read_to_string(d.join("ratelimit-snapshot.json")).unwrap();
    assert_eq!(
        snap.trim_end(),
        r#"{"captured_at":1700000000,"rate_limits":{"five_hour":{"used_percentage":42,"resets_at":1749635640}},"cost":{}}"#
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn signal_probe_conclude_report() {
    let d = tmp("signal");
    let probe = d.join("payload-probe.jsonl");
    let findings = d.join("sf.json");
    let env = [
        ("I2P_COST_STATE_DIR", d.to_str().unwrap()),
        ("I2P_PAYLOAD_PROBE", probe.to_str().unwrap()),
        ("I2P_SIGNAL_FINDINGS", findings.to_str().unwrap()),
        ("I2P_CLOCK", "1700000000"),
    ];
    run(
        &["verify-payload"],
        r#"{"hook_event_name":"PreToolUse","rate_limits":{"five_hour":{"used_percentage":50}}}"#,
        &env,
    );
    line(
        &["signal", "conclude"],
        "",
        &env,
        &format!(
            "signal-probe: concluded → hook-signal-available (guard: live-ceiling); written to {}",
            findings.to_str().unwrap()
        ),
        0,
    );
    line(&["signal", "verdict"], "", &env, "hook-signal-available", 0);
    line(&["signal", "report"], "", &env,
        "🔎 Live-signal probe — verdict: hook-signal-available  (guard mode: live-ceiling)\n   PreToolUse: 1 fires · rate_limits in 1\n   At least one hook event carries .rate_limits — the snapshot bridge can feed the live ceiling guard.", 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn gate_verdicts() {
    let d = tmp("gate");
    let env = [(
        "I2P_COST_STATE_DIR",
        d.join("empty").to_str().unwrap().to_string(),
    )];
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    // CLEAR → CONTINUE
    line(
        &["gate"],
        r#"{"rate_limits":{"five_hour":{"used_percentage":42.5,"resets_at":1749635640},"seven_day":{"used_percentage":18.2,"resets_at":1750000000}}}"#,
        &env,
        r#"{"verdict":"CONTINUE","ceiling":{"verdict":"CLEAR","window":"five_hour","used_pct":42.5,"ceiling":85,"headroom":15,"resets_at":1749635640},"offpeak":null}"#,
        0,
    );
    // HALT (five breaches, seven clear)
    line(
        &["gate"],
        r#"{"rate_limits":{"five_hour":{"used_percentage":92,"resets_at":1750000000},"seven_day":{"used_percentage":10,"resets_at":1760000000}}}"#,
        &env,
        r#"{"verdict":"HALT","ceiling":{"verdict":"HALT","window":"five_hour","used_pct":92,"ceiling":85,"headroom":15,"resets_at":1750000000}}"#,
        10,
    );
    // no signal, no fresh snapshot → ASK (fail closed)
    line(
        &["gate"],
        "{}",
        &env,
        r#"{"verdict":"ASK","reason":"no-live-signal","ceiling":{"verdict":"NO_SIGNAL","window":"seven_day","used_pct":null,"ceiling":85,"headroom":15,"resets_at":null}}"#,
        20,
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn convergence_loop_advances() {
    let d = tmp("conv");
    let sess = d.join("s.json");
    let pop = d.join("po.json");
    let cal = d.join("c.json");
    let env = [
        ("I2P_SESSION_FILE", sess.to_str().unwrap()),
        ("I2P_PLANOPEN_FILE", pop.to_str().unwrap()),
        ("I2P_CALIBRATION_FILE", cal.to_str().unwrap()),
    ];
    std::fs::write(&sess, r#"{"tokens":1000}"#).unwrap();
    line(
        &["plan-open", "medium", "80000"],
        "",
        &env,
        r#"{"opened":"plan:medium","est":80000,"baseline_tokens":1000}"#,
        0,
    );
    std::fs::write(&sess, r#"{"tokens":85000}"#).unwrap();
    line(
        &["plan-close"],
        "",
        &env,
        r#"{"class":"plan:medium","est":80000,"actual":84000,"convergence":{"samples":1,"mean_ratio":1.0500,"sd":0.0000,"p95_band_pct":50.0,"tier":"CALIBRATING","prev_band":60.0,"trend":"improving"}}"#,
        0,
    );
    // the EWMA actually folded a sample
    let calbody = std::fs::read_to_string(&cal).unwrap();
    assert!(calbody.contains(r#""samples": 1"#));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn preflight_and_fanout() {
    let d = tmp("preflight");
    let env = [(
        "I2P_CALIBRATION_FILE",
        d.join("none.json").to_str().unwrap().to_string(),
    )];
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    line(
        &["preflight", "--class", "large"],
        "",
        &env,
        r#"{"verdict":"PROBE","estimate":{"name":"plan:large","per_unit":250000,"basis":"class","confidence":"low","fanout":1,"ratio":1.0,"est_total":250000,"convergence":{"samples":0,"mean_ratio":1.0000,"sd":0.0000,"p95_band_pct":60.0,"tier":"SEEDING","prev_band":-1.0,"trend":"flat"},"interval":[100000,400000]}}"#,
        3,
    );
    // PreToolUse deny JSON on a HALT payload
    let dempty = d.join("empty");
    let env2 = [("I2P_COST_STATE_DIR", dempty.to_str().unwrap())];
    line(
        &["preflight-fanout"],
        r#"{"rate_limits":{"five_hour":{"used_percentage":92,"resets_at":1750000000},"seven_day":{"used_percentage":10,"resets_at":1760000000}}}"#,
        &env2,
        r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Token ceiling reached (live window at 92%). Spawning more agents now risks a lockout. Pause this job (job-ledger.sh pause) and resume when the window resets — /concierge:schedule."}}"#,
        0,
    );
    // clean payload → no deny, no output
    line(
        &["preflight-fanout"],
        r#"{"rate_limits":{"five_hour":{"used_percentage":40,"resets_at":1750000000},"seven_day":{"used_percentage":10,"resets_at":1760000000}}}"#,
        &env2,
        "",
        0,
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cognition_routing() {
    let d = tmp("route");
    let env = [(
        "I2P_CALIBRATION_FILE",
        d.join("none.json").to_str().unwrap().to_string(),
    )];
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    // discernment → sonnet; large = 250k tokens; 0.7 in-frac → 175k·$3 + 75k·$15 = $1.65
    line(
        &["route", "--cognition", "discernment", "--class", "large"],
        "",
        &env,
        r#"{"name":"plan:large","cognition_class":"discernment","best_fit_tier":"sonnet","model":"claude-sonnet-4","est_total":250000,"interval":[100000,400000],"cost_usd":1.65,"cost_band":[0.66,2.64],"per_tier_usd":{"haiku":0.55,"sonnet":1.65,"opus":2.75},"in_frac":0.7}"#,
        0,
    );
    // escalation bumps discernment → opus (false-PASS propagates)
    let (got, _) = run(
        &[
            "route",
            "--cognition",
            "discernment",
            "--escalate",
            "--class",
            "large",
        ],
        "",
        &env,
    );
    assert!(got.contains(r#""best_fit_tier":"opus""#) && got.contains(r#""cost_usd":2.75"#));
    // mechanical → haiku
    let (got, _) = run(
        &["route", "--cognition", "mechanical", "--class", "medium"],
        "",
        &env,
    );
    assert!(got.contains(r#""best_fit_tier":"haiku""#));
    // determinative leaves the token economy: 0 tokens, 0 $
    line(
        &["route", "--cognition", "determinative", "--class", "large"],
        "",
        &env,
        r#"{"name":"plan:large","cognition_class":"determinative","best_fit_tier":"none","model":null,"est_total":0,"cost_usd":0,"note":"determinative_handler — 0 model tokens; runs as a tested tf/client handler"}"#,
        0,
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn oscron_install_idempotent_via_fake_crontab() {
    let d = tmp("oscron");
    // a fake crontab honouring -l / - (buffer-then-replace, like the real one)
    let fake = d.join("fakecron");
    std::fs::write(&fake, "#!/usr/bin/env bash\nS=\"${FAKECRON_STORE:?}\"\ncase \"$1\" in (-l) [ -f \"$S\" ] && cat \"$S\" || exit 1 ;; (-) t=\"$(mktemp)\"; cat > \"$t\"; mv \"$t\" \"$S\" ;; esac\n").unwrap();
    let mut perm = std::fs::metadata(&fake).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perm.set_mode(0o755);
    std::fs::set_permissions(&fake, perm).unwrap();
    // a wrapper file that must exist for the readability check
    let wrapper = d.join("run-offpeak.sh");
    std::fs::write(&wrapper, "#!/usr/bin/env bash\n").unwrap();
    let store = d.join("store.cron");
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let env = [
        ("I2P_CRONTAB", fake.to_str().unwrap()),
        ("FAKECRON_STORE", store.to_str().unwrap()),
        ("I2P_OFFPEAK_WRAPPER", wrapper.to_str().unwrap()),
    ];
    // install twice → idempotent (one line)
    run(
        &["oscron", "install", repo.to_str().unwrap(), "nightly"],
        "",
        &env,
    );
    run(
        &["oscron", "install", repo.to_str().unwrap(), "nightly"],
        "",
        &env,
    );
    let body = std::fs::read_to_string(&store).unwrap();
    assert_eq!(
        body.matches("# i2p-scheduler:nightly").count(),
        1,
        "idempotent"
    );
    assert!(
        body.contains("17 22,23,0-7 * * * bash"),
        "default cron + marker line"
    );
    // uninstall removes it
    run(&["oscron", "uninstall", "nightly"], "", &env);
    let body2 = std::fs::read_to_string(&store).unwrap();
    assert!(!body2.contains("# i2p-scheduler:nightly"));
    let _ = std::fs::remove_dir_all(&d);
}
