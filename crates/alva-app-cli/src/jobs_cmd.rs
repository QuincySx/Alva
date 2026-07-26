// INPUT:  alva_app_core::config::alva_home_dir, job_log/os_sandbox support-path constants, std::process (detached spawn)
// OUTPUT: pub async fn run — `alva jobs <submit|wait|status|result|list>`
// POS:    Daemonless async jobs: detached print-mode workers plus filesystem/PID state and host-owned tool JSONL audit logs.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::job_log::{JOB_TOOLS_LOG_ENV, JOB_TOOLS_LOG_FILE};
use crate::os_sandbox::OS_SANDBOX_WRITE_FILES_ENV;

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
        Some("list") => list(&args[1..]),
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
    let prompt = validate_submit_args(rest)?;
    let job_id = uuid::Uuid::new_v4().to_string();
    let dir = jobs_root()?.join(&job_id);
    create_private_dir(&dir).map_err(|e| format!("create job dir: {e}"))?;

    let result_path = dir.join("result.json");
    let stderr_path = dir.join("stderr.log");
    let result_file =
        create_private_file(&result_path).map_err(|e| format!("create result file: {e}"))?;
    let stderr_file =
        create_private_file(&stderr_path).map_err(|e| format!("create stderr log: {e}"))?;
    let tools_log_path = dir.join(JOB_TOOLS_LOG_FILE);
    create_private_file(&tools_log_path).map_err(|e| format!("create tool log: {e}"))?;

    let exe = std::env::current_exe().map_err(|e| format!("resolve alva binary: {e}"))?;
    let workspace =
        std::env::current_dir().map_err(|e| format!("resolve current workspace: {e}"))?;
    let support_paths = serde_json::to_string(&[&result_path, &stderr_path, &tools_log_path])
        .map_err(|e| format!("serialize job support paths: {e}"))?;

    let mut cmd = std::process::Command::new(exe);
    cmd.arg("-p")
        .args(["--output-format", "json"])
        .args(rest) // caller's flags + prompt, verbatim — same surface as -p
        .current_dir(&workspace)
        // The child is the host in both native and wasm tiers. WASI args,
        // env, and preopens never receive this path.
        .env(JOB_TOOLS_LOG_ENV, &tools_log_path)
        .env(OS_SANDBOX_WRITE_FILES_ENV, support_paths)
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
    let mut child = cmd.spawn().map_err(|e| format!("spawn worker: {e}"))?;

    let meta = JobMeta {
        job_id: job_id.clone(),
        pid: child.id(),
        prompt,
        workspace: workspace.display().to_string(),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
    };
    let meta_json =
        serde_json::to_vec_pretty(&meta).map_err(|e| format!("serialize job meta: {e}"))?;
    if let Err(error) = write_private_file(&dir.join("meta.json"), &meta_json) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("write job meta: {error}"));
    }
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

fn validate_submit_args(rest: &[String]) -> Result<String, String> {
    if rest.is_empty() {
        return Err("jobs submit needs a prompt (and optional -p flags)".into());
    }
    let mut prompt = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "-p" | "--print" | "--output-format" => {
                return Err(format!(
                    "jobs submit owns {}; remove it (job results are always JSON)",
                    rest[index]
                ))
            }
            "--sandbox"
            | "--grant"
            | "--allow-domain"
            | "--provider"
            | "--permission-mode"
            | "--max-turns"
            | "--wasm-wall-clock-secs"
            | "--wasm-fuel"
            | "--allowed-tools"
            | "--system-prompt"
            | "--resume"
            | "--ui-mode" => {
                let value = rest
                    .get(index + 1)
                    .ok_or_else(|| format!("{} requires a value", rest[index]))?;
                if value.starts_with('-') {
                    return Err(format!(
                        "{} requires a value, got flag {value:?}",
                        rest[index]
                    ));
                }
                index += 2;
            }
            "--dangerously-allow-unsandboxed" => index += 1,
            "--" => {
                prompt.extend(rest[index + 1..].iter().cloned());
                break;
            }
            value if value.starts_with('-') => {
                return Err(format!("jobs submit: unknown flag {value:?}"));
            }
            value => {
                prompt.push(value.to_string());
                index += 1;
            }
        }
    }
    if prompt.is_empty() {
        return Err("jobs submit needs a positional prompt; stdin is not available to jobs".into());
    }
    Ok(prompt.join(" "))
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

fn write_private_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut file = create_private_file(path)?;
    file.write_all(contents)?;
    file.sync_all()
}

/// Block until the job completes, then print the result object (same shape
/// as `-p --output-format json`) and exit by its is_error. The polling is
/// an internal detail — callers get blocking semantics, not a poll loop.
async fn wait(rest: &[String]) -> Result<i32, String> {
    let id = rest.first().ok_or("jobs wait needs a job id")?;
    let timeout: Option<u64> = match &rest[1..] {
        [] => None,
        [flag, value] if flag == "--timeout-secs" => Some(
            value
                .parse()
                .map_err(|_| "--timeout-secs expects a number")?,
        ),
        _ => return Err("usage: alva jobs wait <job-id> [--timeout-secs <seconds>]".to_string()),
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
    let [id] = rest else {
        return Err("usage: alva jobs status <job-id>".to_string());
    };
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
    let [id] = rest else {
        return Err("usage: alva jobs result <job-id>".to_string());
    };
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

fn list(rest: &[String]) -> Result<i32, String> {
    if !rest.is_empty() {
        return Err("usage: alva jobs list".to_string());
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn submit_rejects_flags_owned_by_job_runner() {
        for values in [
            vec!["--output-format", "text", "prompt"],
            vec!["-p", "prompt"],
            vec!["--print", "prompt"],
        ] {
            let error = validate_submit_args(&args(&values)).unwrap_err();
            assert!(error.contains("jobs submit owns"), "{error}");
        }
    }

    #[test]
    fn submit_requires_a_positional_prompt_and_valid_flag_values() {
        assert!(validate_submit_args(&args(&["--max-turns", "3"])).is_err());
        assert!(validate_submit_args(&args(&["--max-turns"])).is_err());
        assert!(
            validate_submit_args(&args(&["--max-turns", "--provider", "fast", "prompt"])).is_err()
        );
        assert!(validate_submit_args(&args(&["--typo", "prompt"])).is_err());
    }

    #[test]
    fn submit_extracts_prompt_without_recording_flag_values() {
        let prompt = validate_submit_args(&args(&[
            "--provider",
            "fast",
            "--max-turns",
            "3",
            "fix",
            "the",
            "tests",
        ]))
        .unwrap();
        assert_eq!(prompt, "fix the tests");
    }
}
