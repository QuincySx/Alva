// INPUT:  std::{ffi, fs, io, path}, macOS Seatbelt, Linux Landlock
// OUTPUT: SandboxMode, SandboxEnforcement, SandboxConfig
// POS:    Models per-invocation enforcement for macOS Seatbelt and Linux Landlock OS workers.
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

/// Execution policy retained for callers that configure the security guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    /// Write confinement with unrestricted network access.
    RestrictiveOpen,
    /// Write confinement with network access denied.
    RestrictiveClosed,
    /// Legacy name: currently the same network policy as `RestrictiveOpen`.
    RestrictiveProxied,
    /// No Seatbelt confinement.
    PermissiveOpen,
}

/// Enforcement that is active for this particular invocation.
///
/// Platform support is deliberately not represented here. A macOS process is
/// still `None` unless it was launched through the OS worker path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxEnforcement {
    None,
    /// macOS Seatbelt denies writes outside the configured roots/files. Reads
    /// remain unrestricted because the only cargo-compatible profile found by
    /// Ticket 12 requires `(allow file-read*)`.
    MacOsSeatbeltWriteConfinement,
    /// Linux Landlock denies filesystem reads and writes except for explicit
    /// grants and the runtime support paths required to execute the worker.
    LinuxLandlockConfinement,
}

/// Per-invocation sandbox configuration.
///
/// The default/guard configuration records policy only and is not enforced.
/// [`Self::for_os_worker`] is the production constructor for a process that is
/// about to be, or already was, launched through `sandbox-exec`.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    mode: SandboxMode,
    enforcement: SandboxEnforcement,
    writable_dirs: Vec<PathBuf>,
    writable_files: Vec<PathBuf>,
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    allow_network: bool,
}

impl SandboxConfig {
    pub fn new(mode: SandboxMode) -> Self {
        let allow_network = matches!(
            mode,
            SandboxMode::RestrictiveOpen
                | SandboxMode::RestrictiveProxied
                | SandboxMode::PermissiveOpen
        );
        Self {
            mode,
            enforcement: SandboxEnforcement::None,
            writable_dirs: Vec::new(),
            writable_files: Vec::new(),
            allow_network,
        }
    }

    pub fn mode(&self) -> SandboxMode {
        self.mode
    }

    pub const fn enforcement(&self) -> SandboxEnforcement {
        self.enforcement
    }

    /// Whether confinement is active for this invocation.
    ///
    /// This is an instance property, not a platform constant: ordinary macOS
    /// runs return false, while a child that successfully entered Seatbelt or
    /// Landlock through `--sandbox os-write` carries a concrete enforcement
    /// variant and returns true.
    pub const fn is_enforced(&self) -> bool {
        matches!(
            self.enforcement,
            SandboxEnforcement::MacOsSeatbeltWriteConfinement
                | SandboxEnforcement::LinuxLandlockConfinement
        )
    }

    pub fn promises_isolation(&self) -> bool {
        !matches!(self.mode, SandboxMode::PermissiveOpen)
    }

    /// Build the only production macOS profile supported by `--sandbox os-write`.
    ///
    /// Every directory/file must already exist. Paths are canonicalized,
    /// validated after resolution, sorted, and deduplicated before any value
    /// can become a `sandbox-exec -D` parameter.
    #[cfg(target_os = "macos")]
    pub fn for_os_worker(
        writable_dirs: impl IntoIterator<Item = PathBuf>,
        writable_files: impl IntoIterator<Item = PathBuf>,
    ) -> io::Result<Self> {
        let mut dirs = writable_dirs
            .into_iter()
            .map(|path| canonical_existing_dir(&path))
            .collect::<io::Result<Vec<_>>>()?;
        dirs.sort();
        dirs.dedup();
        // A child root contributes no additional permission when an ancestor
        // is already present. Removing it also makes generated argv stable.
        let mut minimal_dirs: Vec<PathBuf> = Vec::with_capacity(dirs.len());
        for dir in dirs {
            if !minimal_dirs.iter().any(|parent| dir.starts_with(parent)) {
                minimal_dirs.push(dir);
            }
        }

        let mut files = writable_files
            .into_iter()
            .map(|path| canonical_existing_file(&path))
            .collect::<io::Result<Vec<_>>>()?;
        files.sort();
        files.dedup();
        files.retain(|file| !minimal_dirs.iter().any(|dir| file.starts_with(dir)));

        Ok(Self {
            mode: SandboxMode::RestrictiveOpen,
            // Construction describes the intended policy. Only a real kernel
            // denial observed by `confirm_write_confinement` may activate it.
            enforcement: SandboxEnforcement::None,
            writable_dirs: minimal_dirs,
            writable_files: files,
            // The native provider lives in the worker process. Initial OS
            // tier networking is therefore honest unrestricted networking;
            // `--allow-domain` remains wasm-only.
            allow_network: true,
        })
    }

    /// Mark this invocation enforced only after observing Seatbelt deny an
    /// append that ordinary Unix permissions permit.
    #[cfg(target_os = "macos")]
    pub fn confirm_write_confinement(
        &mut self,
        probe_path: &Path,
        expected_nonce: &str,
    ) -> io::Result<()> {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let invalid = |message: &str| io::Error::new(io::ErrorKind::PermissionDenied, message);
        let canonical = std::fs::canonicalize(probe_path)?;
        if canonical != probe_path {
            return Err(invalid(
                "OS sandbox enforcement probe path changed before worker startup",
            ));
        }
        let metadata = std::fs::metadata(&canonical)?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o777 != 0o600
        {
            return Err(invalid(
                "OS sandbox enforcement probe is not an owner-writable 0600 file",
            ));
        }
        if std::fs::read_to_string(&canonical)? != expected_nonce {
            return Err(invalid("OS sandbox enforcement probe nonce does not match"));
        }
        match std::fs::OpenOptions::new().append(true).open(&canonical) {
            Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
                self.enforcement = SandboxEnforcement::MacOsSeatbeltWriteConfinement;
                Ok(())
            }
            Err(error) => Err(invalid(&format!(
                "OS sandbox enforcement probe failed for a non-Seatbelt reason: {error}"
            ))),
            Ok(_) => Err(invalid(
                "OS sandbox enforcement probe remained writable; refuse to mark this invocation enforced",
            )),
        }
    }

    /// Fixed-policy profile. Dynamic paths are parameters, never interpolated
    /// into Scheme source.
    #[cfg(target_os = "macos")]
    pub fn generate_sb_profile(&self) -> String {
        if matches!(self.mode, SandboxMode::PermissiveOpen) {
            return "(version 1)\n(allow default)\n".to_string();
        }

        let mut profile = String::from(
            "(version 1)\n\
             (deny default)\n\
             (allow file-read*)\n\
             (allow process-exec* process-fork)\n\
             (allow sysctl-read mach-lookup mach-priv-task-port)\n\
             (allow signal (target self))\n\
             (allow file-write*\n",
        );
        for index in 0..self.writable_dirs.len() {
            profile.push_str(&format!("  (subpath (param \"WRITE_DIR_{index}\"))\n"));
        }
        for index in 0..self.writable_files.len() {
            profile.push_str(&format!("  (literal (param \"WRITE_FILE_{index}\"))\n"));
        }
        profile.push_str("  (literal \"/dev/null\")\n  (literal \"/dev/tty\"))\n");
        if self.allow_network {
            profile.push_str("(allow network*)\n");
        }
        profile
    }

    /// Build argv for launching the complete worker under Seatbelt.
    #[cfg(target_os = "macos")]
    pub fn sandbox_exec_argv(&self, command: &OsStr, args: &[OsString]) -> Vec<OsString> {
        let mut argv = vec![
            OsString::from("/usr/bin/sandbox-exec"),
            OsString::from("-p"),
            OsString::from(self.generate_sb_profile()),
        ];
        for (index, path) in self.writable_dirs.iter().enumerate() {
            argv.push(OsString::from("-D"));
            let mut parameter = OsString::from(format!("WRITE_DIR_{index}="));
            parameter.push(path);
            argv.push(parameter);
        }
        for (index, path) in self.writable_files.iter().enumerate() {
            argv.push(OsString::from("-D"));
            let mut parameter = OsString::from(format!("WRITE_FILE_{index}="));
            parameter.push(path);
            argv.push(parameter);
        }
        argv.push(command.to_os_string());
        argv.extend_from_slice(args);
        argv
    }

    /// Apply a Linux Landlock ABI v1 filesystem ruleset to the current thread.
    ///
    /// This must run before any worker threads are created: Landlock is
    /// inherited by future threads and child processes, but it does not
    /// retroactively restrict sibling threads. The returned configuration is
    /// marked enforced only after the kernel reports `FullyEnforced` and
    /// `PR_SET_NO_NEW_PRIVS` succeeded.
    #[cfg(target_os = "linux")]
    pub fn enter_linux_landlock_worker(
        read_write_dirs: impl IntoIterator<Item = PathBuf>,
        read_only_dirs: impl IntoIterator<Item = PathBuf>,
        read_write_files: impl IntoIterator<Item = PathBuf>,
        read_only_files: impl IntoIterator<Item = PathBuf>,
    ) -> io::Result<Self> {
        use landlock::{
            Access as _, AccessFs, CompatLevel, Compatible as _, PathBeneath, PathFd, Ruleset,
            RulesetAttr as _, RulesetCreatedAttr as _, ABI,
        };

        let read_write_dirs = canonical_paths(read_write_dirs, PathKind::Directory)?;
        let read_only_dirs = canonical_paths(read_only_dirs, PathKind::Directory)?;
        let read_write_files = canonical_paths(read_write_files, PathKind::File)?;
        let read_only_files = canonical_paths(read_only_files, PathKind::File)?;

        let abi = ABI::V1;
        let all_access = AccessFs::from_all(abi);
        let read_access = AccessFs::from_read(abi);
        let file_access = AccessFs::from_file(abi);
        let read_file_access = read_access & file_access;

        let mut ruleset = Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(all_access)
            .map_err(landlock_error(
                "negotiate Landlock ABI v1 filesystem access",
            ))?
            .create()
            .map_err(landlock_error("create Landlock ruleset"))?;

        for path in read_write_dirs {
            ruleset = ruleset
                .add_rule(PathBeneath::new(
                    PathFd::new(&path).map_err(landlock_error("open Landlock read-write root"))?,
                    all_access,
                ))
                .map_err(landlock_error("add Landlock read-write root"))?;
        }
        for path in read_only_dirs {
            ruleset = ruleset
                .add_rule(PathBeneath::new(
                    PathFd::new(&path).map_err(landlock_error("open Landlock read-only root"))?,
                    read_access,
                ))
                .map_err(landlock_error("add Landlock read-only root"))?;
        }
        for path in read_write_files {
            ruleset = ruleset
                .add_rule(PathBeneath::new(
                    PathFd::new(&path).map_err(landlock_error("open Landlock writable file"))?,
                    file_access,
                ))
                .map_err(landlock_error("add Landlock writable file"))?;
        }
        for path in read_only_files {
            ruleset = ruleset
                .add_rule(PathBeneath::new(
                    PathFd::new(&path).map_err(landlock_error("open Landlock readable file"))?,
                    read_file_access,
                ))
                .map_err(landlock_error("add Landlock readable file"))?;
        }

        let status = ruleset
            .restrict_self()
            .map_err(landlock_error("restrict Linux worker with Landlock"))?;
        ensure_linux_landlock_enforced(status.ruleset, status.no_new_privs)?;

        Ok(Self {
            mode: SandboxMode::RestrictiveOpen,
            enforcement: SandboxEnforcement::LinuxLandlockConfinement,
            writable_dirs: Vec::new(),
            writable_files: Vec::new(),
            // Ticket 14 only negotiates filesystem ABI v1. Network access is
            // intentionally unrestricted, matching the shared OS-tier CLI.
            allow_network: true,
        })
    }

    /// Legacy guard constructor. It does not launch a process and therefore
    /// must remain unenforced on every platform.
    pub fn for_workspace(_workspace: &Path, mode: SandboxMode) -> Self {
        Self::new(mode)
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
enum PathKind {
    Directory,
    File,
}

#[cfg(target_os = "linux")]
fn canonical_paths(
    paths: impl IntoIterator<Item = PathBuf>,
    kind: PathKind,
) -> io::Result<Vec<PathBuf>> {
    let mut canonical = paths
        .into_iter()
        .map(|path| {
            let resolved = std::fs::canonicalize(&path)?;
            let valid = match kind {
                PathKind::Directory => resolved.is_dir(),
                PathKind::File => !resolved.is_dir(),
            };
            if valid {
                Ok(resolved)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid Landlock support path: {}", path.display()),
                ))
            }
        })
        .collect::<io::Result<Vec<_>>>()?;
    canonical.sort();
    canonical.dedup();
    Ok(canonical)
}

#[cfg(target_os = "linux")]
fn landlock_error<E>(context: &'static str) -> impl FnOnce(E) -> io::Error
where
    E: std::fmt::Display,
{
    move |error| {
        io::Error::other(format!(
        "{context}: {error}. --sandbox os-write requires Linux 5.13+ with Landlock ABI v1 enabled"
    ))
    }
}

#[cfg(target_os = "linux")]
fn ensure_linux_landlock_enforced(
    ruleset: landlock::RulesetStatus,
    no_new_privs: bool,
) -> io::Result<()> {
    if ruleset != landlock::RulesetStatus::FullyEnforced || !no_new_privs {
        return Err(io::Error::other(format!(
            "Landlock did not fully enforce the worker (ruleset={ruleset:?}, no_new_privs={no_new_privs}); \
             --sandbox os-write requires Linux 5.13+ with Landlock ABI v1 enabled"
        )));
    }
    Ok(())
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self::new(SandboxMode::RestrictiveOpen)
    }
}

#[cfg(target_os = "macos")]
fn canonical_existing_dir(path: &Path) -> io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "sandbox writable directory is not a directory: {}",
                path.display()
            ),
        ))
    }
}

#[cfg(target_os = "macos")]
fn canonical_existing_file(path: &Path) -> io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    if canonical.is_file() {
        Ok(canonical)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("sandbox writable file is not a file: {}", path.display()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::PermissionsExt as _;

    #[cfg(target_os = "macos")]
    fn run_sandboxed(
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        env: &[(&str, &Path)],
    ) -> std::process::Output {
        let args = args.iter().map(OsString::from).collect::<Vec<_>>();
        let argv = config.sandbox_exec_argv(OsStr::new(command), &args);
        let mut child = std::process::Command::new(&argv[0]);
        child.args(&argv[1..]);
        for (name, value) in env {
            child.env(name, value);
        }
        child.output().expect("launch sandbox-exec")
    }

    /// Pins the original Ticket 12 regression: platform availability is not
    /// enforcement. Ordinary macOS runs must stay false; only the separately
    /// tested, probe-verified OS worker instance may become true.
    #[test]
    fn is_enforced_stays_false_until_a_sandbox_actually_wraps_commands() {
        let config = SandboxConfig::default();
        assert!(!config.is_enforced());
        assert_eq!(config.enforcement(), SandboxEnforcement::None);
    }

    #[test]
    fn default_mode_is_restrictive_open() {
        assert_eq!(
            SandboxConfig::default().mode(),
            SandboxMode::RestrictiveOpen
        );
    }

    #[test]
    fn restrictive_closed_denies_network() {
        assert!(!SandboxConfig::new(SandboxMode::RestrictiveClosed).allow_network);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_landlock_status_must_be_fully_enforced_with_no_new_privs() {
        assert!(
            ensure_linux_landlock_enforced(landlock::RulesetStatus::FullyEnforced, true).is_ok()
        );
        for (status, no_new_privs) in [
            (landlock::RulesetStatus::PartiallyEnforced, true),
            (landlock::RulesetStatus::NotEnforced, true),
            (landlock::RulesetStatus::FullyEnforced, false),
        ] {
            let error = ensure_linux_landlock_enforced(status, no_new_privs).unwrap_err();
            assert!(error.to_string().contains("requires Linux 5.13+"));
        }
    }

    /// The child process is necessary because Landlock restriction is
    /// irreversible. The parent test below invokes only this exact test in a
    /// fresh process and supplies paths through environment variables.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_landlock_child_probe() {
        let Some(grant) = std::env::var_os("ALVA_TEST_LANDLOCK_GRANT") else {
            return;
        };
        let outside = PathBuf::from(std::env::var_os("ALVA_TEST_LANDLOCK_OUTSIDE").unwrap());
        let grant = PathBuf::from(grant);
        let executable = std::env::current_exe().unwrap();
        let read_roots = ["/bin", "/usr", "/lib", "/lib64"]
            .into_iter()
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();

        let config = SandboxConfig::enter_linux_landlock_worker(
            [grant.clone()],
            read_roots,
            [],
            [executable],
        )
        .expect("Linux CI must provide Landlock ABI v1");
        assert!(config.is_enforced());
        assert_eq!(
            config.enforcement(),
            SandboxEnforcement::LinuxLandlockConfinement
        );

        let inside = grant.join("inside.txt");
        std::fs::write(&inside, "allowed").expect("write inside grant");
        assert_eq!(std::fs::read_to_string(&inside).unwrap(), "allowed");

        let read_error = std::fs::read_to_string(&outside).unwrap_err();
        assert_eq!(read_error.kind(), io::ErrorKind::PermissionDenied);
        let outside_write = outside.with_file_name("outside-write.txt");
        let write_error = std::fs::write(&outside_write, "blocked").unwrap_err();
        assert_eq!(write_error.kind(), io::ErrorKind::PermissionDenied);

        let child_target = outside.with_file_name("child-write.txt");
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", "printf blocked > \"$ALVA_TEST_CHILD_TARGET\""])
            .env("ALVA_TEST_CHILD_TARGET", &child_target)
            .status()
            .expect("spawn shell inside Landlock");
        assert!(!status.success(), "child shell escaped Landlock");
        assert!(!child_target.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_landlock_confines_reads_writes_and_child_processes() {
        let root = tempfile::Builder::new()
            .prefix("alva-landlock-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap();
        let grant = root.path().join("grant");
        let outside = root.path().join("outside.txt");
        std::fs::create_dir(&grant).unwrap();
        std::fs::write(&outside, "secret").unwrap();

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sandbox::tests::linux_landlock_child_probe",
                "--nocapture",
            ])
            .env("ALVA_TEST_LANDLOCK_GRANT", &grant)
            .env("ALVA_TEST_LANDLOCK_OUTSIDE", &outside)
            .output()
            .expect("spawn isolated Landlock test child");
        assert!(
            output.status.success(),
            "Landlock child failed:\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn os_worker_cannot_claim_enforcement_without_a_kernel_denial() {
        let grant = tempfile::tempdir().unwrap();
        let probe = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(probe.path(), "nonce").unwrap();
        std::fs::set_permissions(probe.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
        let probe_path = std::fs::canonicalize(probe.path()).unwrap();
        let mut requested = SandboxConfig::for_os_worker([grant.path().to_path_buf()], []).unwrap();

        let error = requested
            .confirm_write_confinement(&probe_path, "nonce")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("remained writable"));
        assert!(!requested.is_enforced());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn profile_uses_fixed_parameters_and_ticket_12_write_only_shape() {
        let grant = tempfile::tempdir().unwrap();
        let support = tempfile::NamedTempFile::new().unwrap();
        let config = SandboxConfig::for_os_worker(
            [grant.path().to_path_buf()],
            [support.path().to_path_buf()],
        )
        .unwrap();
        let profile = config.generate_sb_profile();
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(allow file-read*)"));
        assert!(profile.contains("(allow process-exec* process-fork)"));
        assert!(profile.contains("(allow signal (target self))"));
        assert!(profile.contains("(subpath (param \"WRITE_DIR_0\"))"));
        assert!(profile.contains("(literal (param \"WRITE_FILE_0\"))"));
        assert!(!profile.contains(&grant.path().display().to_string()));
        assert!(!profile.contains(&support.path().display().to_string()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn symlinked_grant_is_canonicalized_before_parameter_injection() {
        let root = tempfile::Builder::new()
            .prefix("alva-os-canonical-")
            .tempdir_in("/tmp")
            .unwrap();
        let config = SandboxConfig::for_os_worker([root.path().to_path_buf()], []).unwrap();
        let argv = config.sandbox_exec_argv(OsStr::new("/usr/bin/true"), &[]);
        let parameter = argv
            .iter()
            .find_map(|arg| arg.to_str().filter(|s| s.starts_with("WRITE_DIR_0=")))
            .unwrap();
        assert!(
            parameter.starts_with("WRITE_DIR_0=/private/tmp/"),
            "{parameter}"
        );
        assert!(!parameter.starts_with("WRITE_DIR_0=/tmp/"), "{parameter}");
    }

    /// Requires an unrestricted macOS test host. Codex's managed sandbox
    /// rejects the nested `sandbox_apply` before this profile can run.
    #[cfg(target_os = "macos")]
    #[test]
    fn canonicalized_tmp_grant_allows_inside_write_and_denies_outside_write() {
        let root = tempfile::Builder::new()
            .prefix("alva-os-enforce-")
            .tempdir_in("/tmp")
            .unwrap();
        let grant = root.path().join("grant");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&grant).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let inside_file = grant.join("inside.txt");
        let outside_file = outside.join("outside.txt");
        let config = SandboxConfig::for_os_worker([grant.clone()], []).unwrap();

        let inside = run_sandboxed(
            &config,
            "/bin/sh",
            &["-c", "printf allowed > \"$ALVA_TARGET\""],
            &[("ALVA_TARGET", &inside_file)],
        );
        assert!(
            inside.status.success(),
            "inside write failed: {}",
            String::from_utf8_lossy(&inside.stderr)
        );

        let denied = run_sandboxed(
            &config,
            "/bin/sh",
            &["-c", "printf blocked > \"$ALVA_TARGET\""],
            &[("ALVA_TARGET", &outside_file)],
        );
        assert!(
            !denied.status.success(),
            "outside write unexpectedly succeeded"
        );
        let denied_stderr = String::from_utf8_lossy(&denied.stderr);
        assert!(
            !denied_stderr.contains("sandbox_apply"),
            "profile was never applied; this is an environment failure, not a path denial: {denied_stderr}"
        );
        assert!(
            denied_stderr.contains("Operation not permitted"),
            "expected kernel denial, got: {}",
            denied_stderr
        );
        assert!(!outside_file.exists());
    }

    /// Requires an unrestricted macOS test host; pins Seatbelt inheritance
    /// through a shell-spawned child command.
    #[cfg(target_os = "macos")]
    #[test]
    fn child_shell_inherits_outside_write_denial() {
        let root = tempfile::Builder::new()
            .prefix("alva-os-child-")
            .tempdir_in("/private/tmp")
            .unwrap();
        let grant = root.path().join("grant");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&grant).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let outside_file = outside.join("child.txt");
        let config = SandboxConfig::for_os_worker([grant], []).unwrap();

        let denied = run_sandboxed(
            &config,
            "/bin/sh",
            &["-c", "/bin/sh -c 'printf blocked > \"$ALVA_OUTSIDE\"'"],
            &[("ALVA_OUTSIDE", &outside_file)],
        );
        assert!(
            !denied.status.success(),
            "child write unexpectedly succeeded"
        );
        let denied_stderr = String::from_utf8_lossy(&denied.stderr);
        assert!(
            !denied_stderr.contains("sandbox_apply"),
            "profile was never applied; this is an environment failure, not an inherited denial: {denied_stderr}"
        );
        assert!(
            denied_stderr.contains("Operation not permitted"),
            "expected child kernel denial, got: {}",
            denied_stderr
        );
        assert!(!outside_file.exists());
    }
}
