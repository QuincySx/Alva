// INPUT:  alva_app_core::config::alva_home_dir, job_log host sink constants, std::process (detached spawn)
// OUTPUT: pub async fn run — `alva jobs <submit|wait|status|result|list>`
// POS:    Daemonless async jobs: detached print-mode workers plus filesystem/PID state and host-owned tool JSONL audit logs.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::job_log::{JOB_TOOLS_LOG_ENV, JOB_TOOLS_LOG_FILE};

fn jobs_root() -> Result<PathBuf, String> {
    alva_app_core::config::alva_home_dir()
        .map(|h| h.join("jobs"))
        .ok_or_else(|| "cannot resolve the alva home directory".to_string())
}

fn job_dir(id: &str) -> Result<PathBuf, String> {
    // Ids are uuids we generated; refuse path-ish input outright.
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return Err(format!("job not found: {id}"));
    }
    let dir = jobs_root()?.join(id);
    if !dir.is_dir() {
        return Err(format!("job not found: {id}"));
    }
    Ok(dir)
}

fn pid_alive(pid: u32) -> bool {
    // Signal 0: existence probe. ESRCH → gone. (Unix-only, like the rest
    // of the CLI's process handling.)
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct JobMeta {
    job_id: String,
    pid: u32,
    prompt: String,
    workspace: String,
    created_at_ms: i64,
}

enum JobState {
    Running,
    /// Terminal; carries the parsed result object.
    Done(serde_json::Value),
    Crashed,
}

fn read_state(dir: &Path, meta: &JobMeta) -> JobState {
    let result_path = dir.join("result.json");
    let parsed = std::fs::read_to_string(&result_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim()).ok());
    match parsed {
        // A fully-written result means done regardless of pid (the child
        // exits right after flushing stdout).
        Some(v) => JobState::Done(v),
        None if pid_alive(meta.pid) => JobState::Running,
        // Dead child, no parseable result: the worker died mid-write or
        // before producing output.
        None => JobState::Crashed,
    }
}

fn load_meta(dir: &Path) -> Result<JobMeta, String> {
    let raw = std::fs::read_to_string(dir.join("meta.json"))
        .map_err(|e| format!("job meta unreadable: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("job meta corrupt: {e}"))
}

pub async fn run(args: &[String]) -> i32 {
    let outcome = match args.first().map(String::as_str) {
        Some("submit") => submit(&args[1..]),
        Some("wait") => wait(&args[1..]).await,
        Some("status") => status(&args[1..]),
        Some("result") => result(&args[1..]),
        Some("list") => list(),
        other => Err(format!(
            "alva jobs: unknown subcommand {:?}\n\
             Usage: alva jobs <submit|wait|status|result|list>",
            other.unwrap_or("<none>")
        )),
    };
    match outcome {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

/// `alva jobs submit [same flags as -p] "PROMPT"` — detach and return.
/// Env (ALVA_* model selection) is inherited by the child, so model choice
/// works exactly like a synchronous dispatch.
fn submit(rest: &[String]) -> Result<i32, String> {
    if rest.is_empty() {
        return Err("jobs submit needs a prompt (and optional -p flags)".into());
    }
    let job_id = uuid::Uuid::new_v4().to_string();
    let dir = jobs_root()?.join(&job_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create job dir: {e}"))?;

    let result_file = std::fs::File::create(dir.join("result.json"))
        .map_err(|e| format!("create result file: {e}"))?;
    let stderr_file = std::fs::File::create(dir.join("stderr.log"))
        .map_err(|e| format!("create stderr log: {e}"))?;
    let tools_log_path = dir.join(JOB_TOOLS_LOG_FILE);
    std::fs::File::create(&tools_log_path).map_err(|e| format!("create tool log: {e}"))?;

    let exe = std::env::current_exe().map_err(|e| format!("resolve alva binary: {e}"))?;
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("-p")
        .args(["--output-format", "json"])
        .args(rest) // caller's flags + prompt, verbatim — same surface as -p
        .current_dir(&workspace)
        // The child is the host in both native and wasm tiers. WASI args,
        // env, and preopens never receive this path.
        .env(JOB_TOOLS_LOG_ENV, &tools_log_path)
        .stdin(std::process::Stdio::null())
        .stdout(result_file)
        .stderr(stderr_file);
    // Detach: own process group, so the child survives this process (and
    // its terminal) exiting. Orphans get reparented to init and finish.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let child = cmd.spawn().map_err(|e| format!("spawn worker: {e}"))?;

    let meta = JobMeta {
        job_id: job_id.clone(),
        pid: child.id(),
        prompt: rest.join(" "),
        workspace: workspace.display().to_string(),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
    };
    std::fs::write(
        dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .map_err(|e| format!("write job meta: {e}"))?;
    // Deliberately NOT waiting on the child: the whole point is to return
    // now. (The pid probe tolerates the zombie window on the rare path
    // where we outlive the child — kill(0) still succeeds on zombies, and
    // `Done` is decided by result.json anyway.)
    std::mem::forget(child);

    println!(
        "{}",
        serde_json::json!({ "job_id": job_id, "pid": meta.pid })
    );
    Ok(0)
}

/// Block until the job completes, then print the result object (same shape
/// as `-p --output-format json`) and exit by its is_error. The polling is
/// an internal detail — callers get blocking semantics, not a poll loop.
async fn wait(rest: &[String]) -> Result<i32, String> {
    let id = rest.first().ok_or("jobs wait needs a job id")?;
    let timeout: Option<u64> = match rest.iter().position(|a| a == "--timeout-secs") {
        Some(i) => Some(
            rest.get(i + 1)
                .and_then(|v| v.parse().ok())
                .ok_or("--timeout-secs expects a number")?,
        ),
        None => None,
    };
    let dir = job_dir(id)?;
    let meta = load_meta(&dir)?;
    let started = std::time::Instant::now();

    loop {
        match read_state(&dir, &meta) {
            JobState::Done(v) => {
                println!("{v}");
                let failed = v["is_error"].as_bool().unwrap_or(false);
                return Ok(i32::from(failed));
            }
            JobState::Crashed => {
                let diag = std::fs::read_to_string(dir.join("stderr.log")).unwrap_or_default();
                return Err(format!(
                    "job {id} crashed without producing a result. stderr tail:\n{}",
                    diag.lines().rev().take(8).collect::<Vec<_>>().join("\n")
                ));
            }
            JobState::Running => {
                if let Some(t) = timeout {
                    if started.elapsed() > Duration::from_secs(t) {
                        eprintln!("timed out after {t}s waiting for job {id} (still running)");
                        return Ok(124); // timeout(1) convention
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

fn status(rest: &[String]) -> Result<i32, String> {
    let id = rest.first().ok_or("jobs status needs a job id")?;
    let dir = job_dir(id)?;
    let meta = load_meta(&dir)?;
    let state = match read_state(&dir, &meta) {
        JobState::Running => "running",
        JobState::Done(v) if v["is_error"].as_bool().unwrap_or(false) => "failed",
        JobState::Done(_) => "completed",
        JobState::Crashed => "crashed",
    };
    println!(
        "{}",
        serde_json::json!({
            "job_id": meta.job_id,
            "state": state,
            "pid": meta.pid,
            "workspace": meta.workspace,
            "created_at_ms": meta.created_at_ms,
        })
    );
    Ok(0)
}

fn result(rest: &[String]) -> Result<i32, String> {
    let id = rest.first().ok_or("jobs result needs a job id")?;
    let dir = job_dir(id)?;
    let meta = load_meta(&dir)?;
    match read_state(&dir, &meta) {
        JobState::Done(v) => {
            println!("{v}");
            Ok(i32::from(v["is_error"].as_bool().unwrap_or(false)))
        }
        JobState::Running => Err(format!("job {id} is still running (use `jobs wait`)")),
        JobState::Crashed => Err(format!("job {id} crashed without producing a result")),
    }
}

fn list() -> Result<i32, String> {
    let root = jobs_root()?;
    let mut rows = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let dir = entry.path();
            let Ok(meta) = load_meta(&dir) else { continue };
            let state = match read_state(&dir, &meta) {
                JobState::Running => "running",
                JobState::Done(v) if v["is_error"].as_bool().unwrap_or(false) => "failed",
                JobState::Done(_) => "completed",
                JobState::Crashed => "crashed",
            };
            rows.push(serde_json::json!({
                "job_id": meta.job_id,
                "state": state,
                "created_at_ms": meta.created_at_ms,
                "prompt": meta.prompt.chars().take(80).collect::<String>(),
            }));
        }
    }
    rows.sort_by_key(|r| -r["created_at_ms"].as_i64().unwrap_or(0));
    println!("{}", serde_json::json!(rows));
    Ok(0)
}
