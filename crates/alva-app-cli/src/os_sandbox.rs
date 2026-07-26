// INPUT:  process argv/internal env, alva-agent-security OS sandbox entry, bundled skill cache, scoped job support paths, uuid
// OUTPUT: enter_or_continue, pre-runtime Linux worker entry, OS_SANDBOX_WRITE_FILES_ENV
// POS:    OS-tier host/worker boundary: private temp capability, macOS Seatbelt wrapping, and pre-runtime Linux Landlock entry.
use std::path::PathBuf;

use alva_app_core::SandboxConfig;

const OS_SANDBOX_ACTIVE_ENV: &str = "ALVA_INTERNAL_OS_SANDBOX_ACTIVE";
const OS_SANDBOX_PROBE_ENV: &str = "ALVA_INTERNAL_OS_SANDBOX_PROBE";
const OS_SANDBOX_PROBE_NONCE_ENV: &str = "ALVA_INTERNAL_OS_SANDBOX_PROBE_NONCE";
const OS_SANDBOX_TMPDIR_ENV: &str = "ALVA_INTERNAL_OS_SANDBOX_TMPDIR";
pub(crate) const OS_SANDBOX_WRITE_FILES_ENV: &str = "ALVA_OS_SANDBOX_WRITE_FILES";

pub(crate) enum EnterOutcome {
    Continue(SandboxConfig),
    ChildExited(i32),
}

pub(crate) fn enter_or_continue(
    argv: &[String],
    grants: &[PathBuf],
) -> Result<EnterOutcome, String> {
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (argv, grants);
        Err("--sandbox os-write is currently supported only on macOS and Linux".to_string())
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var_os(OS_SANDBOX_ACTIVE_ENV).is_some() {
            return Err(
                "internal Linux OS sandbox worker reached the async runtime before Landlock was enforced"
                    .to_string(),
            );
        }

        // Extraction may create cache files. The unsandboxed host prepares it;
        // the worker receives only read/execute access to the finished tree.
        crate::bundled_skills::ensure_extracted()
            .map_err(|error| format!("prepare bundled skills before OS sandbox: {error}"))?;
        let executable = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map_err(|error| format!("resolve current alva executable: {error}"))?;
        let child_args = argv
            .iter()
            .skip(1)
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        let private_tmp = PrivateSandboxTemp::create()?;
        let status = std::process::Command::new(&executable)
            .args(child_args)
            .env(OS_SANDBOX_ACTIVE_ENV, "1")
            .env(OS_SANDBOX_TMPDIR_ENV, private_tmp.path())
            .env("TMPDIR", private_tmp.path())
            .status()
            .map_err(|error| format!("launch Linux Landlock worker: {error}"))?;
        Ok(EnterOutcome::ChildExited(status.code().unwrap_or(1)))
    }

    #[cfg(target_os = "macos")]
    {
        let active = std::env::var_os(OS_SANDBOX_ACTIVE_ENV).is_some();
        let owned_tmpdir = if active {
            None
        } else {
            Some(PrivateSandboxTemp::create()?)
        };
        let tmpdir = match &owned_tmpdir {
            Some(temp) => temp.path().to_path_buf(),
            None => sandbox_tmpdir_from_env()?,
        };
        let support_files = support_files_from_env()?;
        if active {
            std::env::remove_var(OS_SANDBOX_WRITE_FILES_ENV);
        }
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
            .env(OS_SANDBOX_TMPDIR_ENV, &tmpdir)
            .env("TMPDIR", &tmpdir)
            .status()
            .map_err(|error| format!("launch macOS sandbox worker: {error}"))?;
        Ok(EnterOutcome::ChildExited(status.code().unwrap_or(1)))
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_worker_is_active() -> bool {
    std::env::var_os(OS_SANDBOX_ACTIVE_ENV).is_some()
}

/// Enter Landlock before Tokio creates any worker threads. Landlock applies
/// to the calling thread and is inherited by threads/processes created later;
/// applying it from `run()` would leave the runtime's existing siblings bare.
#[cfg(target_os = "linux")]
pub(crate) fn enter_linux_worker_before_threads(
    grants: &[PathBuf],
) -> Result<SandboxConfig, String> {
    let tmpdir = sandbox_tmpdir_from_env()?;
    let support_files = support_files_from_env()?;
    std::env::remove_var(OS_SANDBOX_WRITE_FILES_ENV);
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|error| format!("resolve current alva executable: {error}"))?;

    let mut read_write_dirs = grants.to_vec();
    read_write_dirs.push(tmpdir);

    let mut read_only_dirs = linux_system_read_roots();
    read_only_dirs.extend(linux_user_read_roots());

    let mut read_write_files = support_files;
    for path in ["/dev/null", "/dev/tty"] {
        let path = PathBuf::from(path);
        if path.exists() {
            read_write_files.push(path);
        }
    }

    let mut read_only_files = vec![executable];
    for path in [
        "/dev/random",
        "/dev/urandom",
        "/dev/zero",
        "/etc/hosts",
        "/etc/resolv.conf",
        "/etc/nsswitch.conf",
        "/etc/host.conf",
        "/etc/gai.conf",
        "/etc/ld.so.cache",
        "/etc/localtime",
        "/etc/passwd",
        "/etc/group",
    ] {
        let path = PathBuf::from(path);
        if path.exists() {
            read_only_files.push(path);
        }
    }

    let config = SandboxConfig::enter_linux_landlock_worker(
        read_write_dirs,
        read_only_dirs,
        read_write_files,
        read_only_files,
    )
    .map_err(|error| format!("enforce Linux Landlock sandbox: {error}"))?;
    std::env::remove_var(OS_SANDBOX_ACTIVE_ENV);
    Ok(config)
}

#[cfg(target_os = "linux")]
fn linux_system_read_roots() -> Vec<PathBuf> {
    ["/bin", "/sbin", "/usr", "/lib", "/lib64", "/nix/store"]
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .chain(
            ["/etc/ssl", "/etc/pki", "/etc/ca-certificates"]
                .into_iter()
                .map(PathBuf::from)
                .filter(|path| path.is_dir()),
        )
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_user_read_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(path) = std::env::var("PATH") {
        roots.extend(std::env::split_paths(&path).filter(|path| path.is_dir()));
    }
    if let Some(path) = alva_app_core::config::alva_home_dir() {
        if path.is_dir() {
            roots.push(path);
        }
    }
    if let Ok(path) = std::env::var("XDG_CONFIG_HOME") {
        let path = PathBuf::from(path).join("alva");
        if path.is_dir() {
            roots.push(path);
        }
    }
    let bundled = crate::bundled_skills::bundled_skills_cache_dir();
    if bundled.is_dir() {
        roots.push(bundled);
    }
    if let Some(home) = dirs::home_dir() {
        for path in [
            std::env::var_os("CARGO_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cargo")),
            std::env::var_os("RUSTUP_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".rustup")),
        ] {
            if path.is_dir() {
                roots.push(path);
            }
        }
    }
    roots
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct PrivateSandboxTemp {
    path: PathBuf,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl PrivateSandboxTemp {
    fn create() -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt as _;

        #[cfg(target_os = "macos")]
        let base = PathBuf::from("/private/tmp");
        #[cfg(target_os = "linux")]
        let base = PathBuf::from("/tmp");
        let base = std::fs::canonicalize(&base)
            .map_err(|error| format!("resolve trusted sandbox temp base: {error}"))?;
        let path = base.join(format!(
            "alva-sandbox-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        builder
            .create(&path)
            .map_err(|error| format!("create private sandbox temp directory: {error}"))?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Drop for PrivateSandboxTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn sandbox_tmpdir_from_env() -> Result<PathBuf, String> {
    let candidate = std::env::var_os(OS_SANDBOX_TMPDIR_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| "OS sandbox child is missing its private temp capability".to_string())?;
    let canonical = std::fs::canonicalize(&candidate).map_err(|error| {
        format!(
            "cannot resolve private sandbox temp {}: {error}",
            candidate.display()
        )
    })?;
    if !canonical.is_dir() {
        return Err(format!(
            "private sandbox temp is not a directory: {}",
            canonical.display()
        ));
    }
    std::env::remove_var(OS_SANDBOX_TMPDIR_ENV);
    Ok(canonical)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn support_files_from_env() -> Result<Vec<PathBuf>, String> {
    let Some(raw) = std::env::var_os(OS_SANDBOX_WRITE_FILES_ENV) else {
        return Ok(Vec::new());
    };
    let requested = serde_json::from_str::<Vec<PathBuf>>(&raw.to_string_lossy())
        .map_err(|error| format!("invalid internal OS sandbox support-file list: {error}"))?;
    let jobs_root = alva_app_core::config::alva_home_dir()
        .map(|root| root.join("jobs"))
        .ok_or_else(|| "cannot resolve jobs root for OS sandbox support files".to_string())?;
    let jobs_root = std::fs::canonicalize(&jobs_root)
        .map_err(|error| format!("resolve jobs root for OS sandbox support files: {error}"))?;
    validate_support_files(requested, &jobs_root)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn validate_support_files(
    requested: Vec<PathBuf>,
    jobs_root: &std::path::Path,
) -> Result<Vec<PathBuf>, String> {
    requested
        .into_iter()
        .map(|path| {
            let canonical = std::fs::canonicalize(&path)
                .map_err(|error| format!("resolve OS sandbox support file: {error}"))?;
            let allowed_name = canonical
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, "result.json" | "stderr.log" | "tools.jsonl"));
            if !canonical.is_file() || !canonical.starts_with(&jobs_root) || !allowed_name {
                return Err(format!(
                    "OS sandbox support file is outside the scoped jobs directory: {}",
                    canonical.display()
                ));
            }
            Ok(canonical)
        })
        .collect()
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn private_sandbox_temp_is_narrow_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = PrivateSandboxTemp::create().unwrap();
        let path = temp.path().to_path_buf();
        assert_ne!(path, PathBuf::from("/"));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        drop(temp);
        assert!(!path.exists());
    }

    #[test]
    fn support_files_are_scoped_to_known_job_outputs() {
        let root = tempfile::tempdir().unwrap();
        let jobs = root.path().join("jobs");
        let job = jobs.join("job-id");
        std::fs::create_dir_all(&job).unwrap();
        let allowed = job.join("tools.jsonl");
        let forbidden_name = job.join("config.json");
        let outside = root.path().join("result.json");
        for path in [&allowed, &forbidden_name, &outside] {
            std::fs::write(path, "").unwrap();
        }
        let jobs = jobs.canonicalize().unwrap();

        assert_eq!(
            validate_support_files(vec![allowed.clone()], &jobs).unwrap(),
            vec![allowed.canonicalize().unwrap()]
        );
        assert!(validate_support_files(vec![forbidden_name], &jobs).is_err());
        assert!(validate_support_files(vec![outside], &jobs).is_err());
    }
}
