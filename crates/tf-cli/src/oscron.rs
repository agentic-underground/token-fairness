//! oscron — install/remove the OS-level crontab entry for a guarded off-peak job.
//! Port of `install-oscron.sh`. Idempotent replace-by-marker; never touches other entries.
//!
//! The crontab line is byte-identical to the bash, marker and all:
//!   `<cron> bash <wrapper> <repo> <job> >> <log> 2>&1  # i2p-scheduler:<job>`
//! The wrapper is the plugin's run-offpeak shim (overridable via `I2P_OFFPEAK_WRAPPER`).
//! Cron has a minimal env — the wrapper resolves absolute paths to claude/tf/flock (review S3).

use std::io::Write;
use std::process::{Command, Stdio};
use tf_core::{state, Out};

/// The crontab program (overridable for tests via `I2P_CRONTAB`, e.g. a fake honouring -l/-).
fn crontab_cmd() -> String {
    std::env::var("I2P_CRONTAB").unwrap_or_else(|_| "crontab".to_string())
}

/// Split a possibly-multi-word command ("crontab" or "/path/fake -x") into program + args.
fn split_cmd(s: &str) -> (String, Vec<String>) {
    let mut it = s.split_whitespace().map(|t| t.to_string());
    let prog = it.next().unwrap_or_default();
    (prog, it.collect())
}

/// `command -v <prog>` — resolve on PATH or as an executable path.
fn command_exists(prog: &str) -> bool {
    if prog.contains('/') {
        return std::path::Path::new(prog).is_file();
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            if std::path::Path::new(dir).join(prog).is_file() {
                return true;
            }
        }
    }
    false
}

/// `$CRONTAB -l` — the current crontab text (empty on no-crontab / error).
fn current_crontab() -> String {
    let (prog, mut args) = split_cmd(&crontab_cmd());
    args.push("-l".to_string());
    Command::new(prog)
        .args(&args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Pipe `content` into `$CRONTAB -`.
fn write_crontab(content: &str) {
    let (prog, mut args) = split_cmd(&crontab_cmd());
    args.push("-".to_string());
    if let Ok(mut child) = Command::new(prog).args(&args).stdin(Stdio::piped()).spawn() {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(content.as_bytes());
        }
        let _ = child.wait();
    }
}

fn wrapper_path(repo_abs: &str) -> String {
    std::env::var("I2P_OFFPEAK_WRAPPER")
        .unwrap_or_else(|_| format!("{}/plugins/scheduler/hooks/run-offpeak.sh", repo_abs))
}

/// Lexical absolute dir (bash `cd "$repo" && pwd || echo "$repo"`).
fn abs_dir(dir: &str) -> String {
    use std::path::{Component, Path, PathBuf};
    let p = Path::new(dir);
    if !p.is_dir() {
        return dir.to_string();
    }
    let base = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut out = PathBuf::new();
    for c in base.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

pub fn dispatch(argv: &[String]) -> Out {
    let (prog, _) = split_cmd(&crontab_cmd());
    if !command_exists(&prog) {
        return Out::err("install-oscron: no crontab on this system", 2);
    }

    // Accept both `oscron uninstall <job>` (the tf subcommand form) and `--uninstall <job>`.
    let first = argv.first().map(|s| s.as_str()).unwrap_or("");
    if first == "uninstall" || first == "--uninstall" {
        let job = argv.get(1).map(|s| s.as_str()).unwrap_or("");
        if job.is_empty() {
            return Out::err("usage: install-oscron.sh --uninstall <job-id>", 2);
        }
        let marker = format!("# i2p-scheduler:{}", job);
        let kept: String = current_crontab()
            .lines()
            .filter(|l| !l.contains(&marker))
            .map(|l| format!("{}\n", l))
            .collect();
        write_crontab(&kept);
        return Out::ok(format!(
            "install-oscron: removed crontab entry for {}\n",
            job
        ));
    }

    // install: `oscron install <repo> <job> [cron]` or `oscron <repo> <job> [cron]`.
    let base = if first == "install" { 1 } else { 0 };
    let repo = argv.get(base).map(|s| s.as_str()).unwrap_or("");
    let job = argv.get(base + 1).map(|s| s.as_str()).unwrap_or("");
    let cron = argv
        .get(base + 2)
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("17 22,23,0-7 * * *");
    if repo.is_empty() || job.is_empty() {
        return Out::err(
            "usage: install-oscron.sh <repo-dir> <job-id> [cron-expr]",
            2,
        );
    }
    let repo_abs = abs_dir(repo);
    let wrapper = wrapper_path(&repo_abs);
    if !std::path::Path::new(&wrapper).is_file() {
        return Out::err(format!("install-oscron: wrapper not found: {}", wrapper), 2);
    }
    let log = format!("{}/offpeak-job.log", state::home_cost_dir());
    let marker = format!("# i2p-scheduler:{}", job);
    let line = format!(
        "{} bash {} {} {} >> {} 2>&1  {}",
        cron, wrapper, repo_abs, job, log, marker
    );

    let mut content: String = current_crontab()
        .lines()
        .filter(|l| !l.contains(&marker))
        .map(|l| format!("{}\n", l))
        .collect();
    content.push_str(&line);
    content.push('\n');
    write_crontab(&content);

    Out::ok(format!(
        "install-oscron: installed for {}\n  {}\n",
        job, line
    ))
}
