// INPUT:  process argv/env, alva-agent-security Seatbelt command builder, bundled skill cache, runtime write-denial probe
// OUTPUT: enter_or_continue, OS_SANDBOX_WRITE_FILES_ENV
// POS:    macOS `--sandbox os-write` host/worker boundary that re-execs the complete native CLI under write confinement.
use std::path::PathBuf;

use alva_app_core::SandboxConfig;

const OS_SANDBOX_ACTIVE_ENV: &str = "ALVA_INTERNAL_OS_SANDBOX_ACTIVE";
const OS_SANDBOX_PROBE_ENV: &str = "ALVA_INTERNAL_OS_SANDBOX_PROBE";
const OS_SANDBOX_PROBE_NONCE_ENV: &str = "ALVA_INTERNAL_OS_SANDBOX_PROBE_NONCE";
pub(crate) const OS_SANDBOX_WRITE_FILES_ENV: &str = "ALVA_OS_SANDBOX_WRITE_FILES";

pub(crate) enum EnterOutcome {
    Continue(SandboxConfig),
    ChildExited(i32),
}

pub(crate) fn enter_or_continue(
    argv: &[String],
    grants: &[PathBuf],
) -> Result<EnterOutcome, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (argv, grants);
        Err("--sandbox os-write is currently supported only on macOS".to_string())
    }

    #[cfg(target_os = "macos")]
    {
        let tmpdir = real_tmpdir()?;
        let support_files = support_files_from_env()?;
        let active = std::env::var_os(OS_SANDBOX_ACTIVE_ENV).is_some();
        let writable_dirs = grants
            .iter()
            .cloned()
            .chain([tmpdir.clone()])
            .collect::<Vec<_>>();
        let mut config = SandboxConfig::for_os_worker(writable_dirs, support_files)
            .map_err(|error| format!("prepare macOS sandbox paths: {error}"))?;

        if active {
            let probe_path = std::env::var_os(OS_SANDBOX_PROBE_ENV)
                .map(PathBuf::from)
                .ok_or_else(|| "OS sandbox child is missing its enforcement probe".to_string())?;
            let probe_nonce = std::env::var(OS_SANDBOX_PROBE_NONCE_ENV)
                .map_err(|_| "OS sandbox child is missing its enforcement nonce".to_string())?;
            config
                .confirm_write_confinement(&probe_path, &probe_nonce)
                .map_err(|error| format!("confirm macOS sandbox enforcement: {error}"))?;
            for name in [
                OS_SANDBOX_ACTIVE_ENV,
                OS_SANDBOX_PROBE_ENV,
                OS_SANDBOX_PROBE_NONCE_ENV,
            ] {
                std::env::remove_var(name);
            }
            return Ok(EnterOutcome::Continue(config));
        }

        // Extraction may create cache files. The host prepares it before
        // confinement; the child then takes ensure_extracted's read-only path.
        crate::bundled_skills::ensure_extracted()
            .map_err(|error| format!("prepare bundled skills before OS sandbox: {error}"))?;

        let executable = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map_err(|error| format!("resolve current alva executable: {error}"))?;
        let probe = WriteDenialProbe::create(grants, &tmpdir, &executable)?;
        let child_args = argv
            .iter()
            .skip(1)
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        let sandbox_argv = config.sandbox_exec_argv(executable.as_os_str(), &child_args);
        let program = sandbox_argv
            .first()
            .ok_or_else(|| "macOS sandbox command is empty".to_string())?;
        let status = std::process::Command::new(program)
            .args(&sandbox_argv[1..])
            .env(OS_SANDBOX_ACTIVE_ENV, "1")
            .env(OS_SANDBOX_PROBE_ENV, &probe.path)
            .env(OS_SANDBOX_PROBE_NONCE_ENV, &probe.nonce)
            .env("TMPDIR", &tmpdir)
            .status()
            .map_err(|error| format!("launch macOS sandbox worker: {error}"))?;
        Ok(EnterOutcome::ChildExited(status.code().unwrap_or(1)))
    }
}

#[cfg(target_os = "macos")]
struct WriteDenialProbe {
    path: PathBuf,
    nonce: String,
}

#[cfg(target_os = "macos")]
impl WriteDenialProbe {
    fn create(
        grants: &[PathBuf],
        tmpdir: &std::path::Path,
        executable: &std::path::Path,
    ) -> Result<Self, String> {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;

        let mut forbidden = grants.to_vec();
        forbidden.push(tmpdir.to_path_buf());
        let mut candidates = vec![PathBuf::from("/private/tmp")];
        if let Some(parent) = executable.parent() {
            candidates.push(parent.to_path_buf());
        }
        if let Some(home) = dirs::home_dir() {
            candidates.push(home);
        }
        if let Ok(cwd) = std::env::current_dir() {
            if let Some(parent) = cwd.parent() {
                candidates.push(parent.to_path_buf());
            }
        }

        let nonce = uuid::Uuid::new_v4().to_string();
        for candidate in candidates {
            let Ok(candidate) = std::fs::canonicalize(candidate) else {
                continue;
            };
            if forbidden.iter().any(|root| candidate.starts_with(root)) {
                continue;
            }
            let path = candidate.join(format!(".alva-os-sandbox-probe-{nonce}"));
            let opened = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&path);
            let Ok(mut file) = opened else { continue };
            if file.write_all(nonce.as_bytes()).is_err() || file.flush().is_err() {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            drop(file);
            // Prove the unsandboxed host can append. The child's EPERM then
            // demonstrates a policy transition, not ordinary file permissions.
            if std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .is_err()
            {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            return Ok(Self { path, nonce });
        }
        Err("cannot create an OS sandbox enforcement probe outside all writable grants/TMPDIR; refuse to claim enforcement for these overly broad roots".to_string())
    }
}

#[cfg(target_os = "macos")]
impl Drop for WriteDenialProbe {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(target_os = "macos")]
fn real_tmpdir() -> Result<PathBuf, String> {
    let candidate = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|error| format!("cannot resolve TMPDIR {}: {error}", candidate.display()))?;
    if !canonical.is_dir() {
        return Err(format!(
            "TMPDIR is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

#[cfg(target_os = "macos")]
fn support_files_from_env() -> Result<Vec<PathBuf>, String> {
    let Some(raw) = std::env::var_os(OS_SANDBOX_WRITE_FILES_ENV) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<PathBuf>>(&raw.to_string_lossy())
        .map_err(|error| format!("invalid internal OS sandbox support-file list: {error}"))
}
