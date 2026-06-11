//! offpeak_run — the OS-cron entry point. Port of `run-offpeak-job.sh`. Runs a guarded
//! off-peak job HEADLESS so it survives Claude being closed (machine awake).
//!
//! Guards, in order: (1) flock single-instance; (2) off-peak window gate; (3) hand the
//! persisted prompt to a headless `claude -p` with a scoped tool allowlist. All output
//! appends to `~/.claude/state/i2p-cost/offpeak-job.log`.

use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::process::Command;
use tf_core::{offpeak, state, Out};

/// `date '+%Y-%m-%dT%H:%M:%S%z'` — match the bash stamp exactly (shell out, like the oracle).
fn stamp() -> String {
    Command::new("date")
        .arg("+%Y-%m-%dT%H:%M:%S%z")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim_end().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "?".to_string())
}

fn appendln(log: &str, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = writeln!(f, "{}", line);
    }
}

fn sanitize_job(job: &str) -> String {
    job.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn run(argv: &[String]) -> Out {
    let repo = argv.first().map(|s| s.as_str()).unwrap_or("");
    let job = argv.get(1).map(|s| s.as_str()).unwrap_or("");
    if repo.is_empty() || job.is_empty() {
        return Out::err("usage: run-offpeak-job.sh <repo-dir> <job-id>", 2);
    }
    if !std::path::Path::new(repo).is_dir() {
        return Out::err(format!("run-offpeak-job: repo not found: {}", repo), 2);
    }

    let state_dir = state::home_cost_dir();
    let _ = std::fs::create_dir_all(&state_dir);
    let log = format!("{}/offpeak-job.log", state_dir);
    let lock = format!("{}/offpeak-job-{}.lock", state_dir, sanitize_job(job));

    // (1) Single-instance, non-blocking flock — hold the fd for the whole process.
    let lockfile = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false) // a lock file — never clobber its contents, we only need the fd
        .open(&lock)
    {
        Ok(f) => f,
        Err(_) => return Out::default(), // `exec 9>"$lock" || exit 0`
    };
    let held = unsafe { libc::flock(lockfile.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
    if !held {
        appendln(
            &log,
            &format!(
                "{} [{}] another run holds the lock — skipping",
                stamp(),
                job
            ),
        );
        return Out::default();
    }

    // (2) Off-peak gate — do nothing during peak hours.
    let now = state::now_epoch().to_string();
    let ow = offpeak::window(offpeak::WindowArgs {
        now: &now,
        start: "22:00",
        end: "08:00",
        reset: None,
        tz_offset_min: None,
    });
    if state::raw_field(ow.stdout.trim_end(), "in_offpeak") != "true" {
        appendln(
            &log,
            &format!("{} [{}] peak hours — skipping", stamp(), job),
        );
        return Out::default();
    }

    // (3) The persisted prompt is the source of truth for what the job does.
    let repo_trim = repo.strip_suffix('/').unwrap_or(repo);
    let prompt_file = format!("{}/.i2p/scheduled-jobs/{}.prompt.txt", repo_trim, job);
    let prompt = match std::fs::read_to_string(&prompt_file) {
        Ok(p) => p,
        Err(_) => {
            appendln(
                &log,
                &format!("{} [{}] no prompt at {}", stamp(), job, prompt_file),
            );
            return Out::default();
        }
    };
    if !command_on_path("claude") {
        appendln(
            &log,
            &format!("{} [{}] claude CLI not on PATH", stamp(), job),
        );
        return Out::default();
    }

    appendln(
        &log,
        &format!(
            "{} [{}] off-peak fire — launching headless claude",
            stamp(),
            job
        ),
    );
    let logf = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .ok();
    let (out, err) = match &logf {
        Some(f) => (
            std::process::Stdio::from(f.try_clone().unwrap()),
            std::process::Stdio::from(f.try_clone().unwrap()),
        ),
        None => (std::process::Stdio::null(), std::process::Stdio::null()),
    };
    let rc = Command::new("claude")
        .current_dir(repo)
        .args([
            "-p",
            &prompt,
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
            "Read",
            "Edit",
            "Glob",
            "Grep",
            "Bash",
        ])
        .stdout(out)
        .stderr(err)
        .status()
        .ok()
        .and_then(|s| s.code())
        .unwrap_or(0);
    appendln(
        &log,
        &format!("{} [{}] headless run finished (rc={})", stamp(), job, rc),
    );
    Out::default()
}

fn command_on_path(prog: &str) -> bool {
    std::env::var("PATH")
        .map(|p| {
            p.split(':')
                .any(|d| std::path::Path::new(d).join(prog).is_file())
        })
        .unwrap_or(false)
}
