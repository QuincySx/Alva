// INPUT:  std::{path, string}, thiserror, wasmtime, wasmtime_wasi::{p1, pipe, WasiCtxBuilder}
// OUTPUT: Access, Grant, RunRequest, RunOutcome, SandboxError, SandboxRunner, run_module
// POS:    Native WASIp1 runner boundary that mounts per-call preopens and captures guest process results.

//! Host-side runner for WASIp1 command modules.
//!
//! The guest receives no ambient filesystem access. Each [`Grant`] is opened
//! by Wasmtime and exposed as one WASI preopen for the lifetime of a single
//! run.
//!
//! Hold a [`SandboxRunner`] to compile many jobs against one shared
//! [`Engine`] (its code cache survives across runs); every run still gets a
//! fresh `Store` + WASI context, so isolation is per-run. [`run_module`] is a
//! one-shot convenience that builds a throwaway runner.

use std::path::PathBuf;
use std::string::FromUtf8Error;

use thiserror::Error;
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{DirPerms, FilePerms, I32Exit, WasiCtxBuilder};

const OUTPUT_CAPACITY: usize = 1024 * 1024;

/// Filesystem access a [`Grant`] confers on its mounted directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Access {
    /// The guest may read the directory and its files, but not create,
    /// delete, or modify anything within it.
    ReadOnly,
    /// Full read + write: the guest may create, delete, rename, and overwrite.
    #[default]
    ReadWrite,
}

impl Access {
    fn dir_perms(self) -> DirPerms {
        match self {
            Access::ReadOnly => DirPerms::READ,
            Access::ReadWrite => DirPerms::all(),
        }
    }

    fn file_perms(self) -> FilePerms {
        match self {
            Access::ReadOnly => FilePerms::READ,
            Access::ReadWrite => FilePerms::all(),
        }
    }
}

/// Maps one host directory into the guest's filesystem namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// Existing host directory to expose.
    pub host: PathBuf,
    /// Guest-visible mount point, for example `/work`.
    pub guest: String,
    /// Whether the guest may mutate the mounted directory.
    pub access: Access,
}

impl Grant {
    /// A read-write mount (`access = ReadWrite`).
    pub fn read_write(host: impl Into<PathBuf>, guest: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            guest: guest.into(),
            access: Access::ReadWrite,
        }
    }

    /// A read-only mount: the guest can read but not mutate the directory.
    pub fn read_only(host: impl Into<PathBuf>, guest: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            guest: guest.into(),
            access: Access::ReadOnly,
        }
    }
}

/// All inputs required for one isolated module invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    /// Bytes of a WASIp1 core WebAssembly module exporting `_start`.
    pub module: Vec<u8>,
    /// Directories explicitly made visible to the guest for this invocation.
    pub grants: Vec<Grant>,
    /// Exact WASI argument vector exposed to the guest.
    pub args: Vec<String>,
}

/// Observable process result from a completed guest invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// Zero for a normally returned `_start`, or the code passed to WASI
    /// `proc_exit`.
    pub exit_code: i32,
    /// UTF-8 stdout captured in memory.
    pub stdout: String,
    /// UTF-8 stderr captured in memory.
    pub stderr: String,
}

/// Failure to configure or execute a guest module.
#[derive(Debug, Error)]
pub enum SandboxError {
    /// The supplied bytes are not a module accepted by Wasmtime.
    #[error("failed to compile WASIp1 module: {0}")]
    Module(#[source] wasmtime::Error),

    /// A host directory could not be opened as a WASI preopen.
    #[error("failed to mount host directory {host:?} at guest path {guest:?}: {source}")]
    Grant {
        /// Host directory from the rejected grant.
        host: PathBuf,
        /// Guest mount point from the rejected grant.
        guest: String,
        /// Wasmtime/WASI error describing why the preopen failed.
        #[source]
        source: wasmtime::Error,
    },

    /// WASIp1 imports could not be registered with the module linker.
    #[error("failed to register WASIp1 imports: {0}")]
    WasiLinker(#[source] wasmtime::Error),

    /// The module could not be instantiated with the configured WASI imports.
    #[error("failed to instantiate WASIp1 module: {0}")]
    Instantiate(#[source] wasmtime::Error),

    /// The module does not expose the WASI command entry point with the
    /// expected signature.
    #[error("failed to resolve WASIp1 `_start` entry point: {0}")]
    StartFunction(#[source] wasmtime::Error),

    /// The guest trapped for a reason other than WASI `proc_exit`.
    #[error("WASIp1 module execution failed: {0}")]
    Execution(#[source] wasmtime::Error),

    /// Captured stdout was not valid UTF-8.
    #[error("guest stdout is not valid UTF-8: {0}")]
    StdoutUtf8(#[source] FromUtf8Error),

    /// Captured stderr was not valid UTF-8.
    #[error("guest stderr is not valid UTF-8: {0}")]
    StderrUtf8(#[source] FromUtf8Error),
}

/// A reusable host runner that owns one shared [`Engine`].
///
/// Wasmtime is designed for one `Engine` shared across many `Store`s: the
/// engine carries the compiled-code cache, while each `Store` is the
/// isolation boundary. Constructing a runner once and calling [`run`] per job
/// lets repeated jobs reuse that cache; every run still builds a fresh WASI
/// context and `Store`, so no state leaks between runs.
///
/// [`run`]: SandboxRunner::run
#[derive(Clone)]
pub struct SandboxRunner {
    engine: Engine,
}

impl Default for SandboxRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxRunner {
    /// Build a runner with a fresh shared engine.
    pub fn new() -> Self {
        Self {
            engine: Engine::default(),
        }
    }

    /// Runs one WASIp1 command module with only the requested filesystem
    /// grants, on this runner's shared engine.
    ///
    /// A fresh linker, WASI context and store are created on every call. Host
    /// stdio, environment variables and filesystem paths are not inherited.
    pub fn run(&self, req: RunRequest) -> Result<RunOutcome, SandboxError> {
        let module = Module::new(&self.engine, &req.module).map_err(SandboxError::Module)?;

        let stdout = MemoryOutputPipe::new(OUTPUT_CAPACITY);
        let stderr = MemoryOutputPipe::new(OUTPUT_CAPACITY);
        let mut wasi = WasiCtxBuilder::new();
        wasi.allow_blocking_current_thread(true)
            .allow_tcp(false)
            .allow_udp(false)
            .allow_ip_name_lookup(false)
            .stdout(stdout.clone())
            .stderr(stderr.clone())
            .args(&req.args);

        for grant in req.grants {
            wasi.preopened_dir(
                &grant.host,
                &grant.guest,
                grant.access.dir_perms(),
                grant.access.file_perms(),
            )
            .map_err(|source| SandboxError::Grant {
                host: grant.host,
                guest: grant.guest,
                source,
            })?;
        }

        let wasi: WasiP1Ctx = wasi.build_p1();
        let mut linker: Linker<WasiP1Ctx> = Linker::new(&self.engine);
        p1::add_to_linker_sync(&mut linker, |ctx| ctx).map_err(SandboxError::WasiLinker)?;

        let mut store = Store::new(&self.engine, wasi);
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(SandboxError::Instantiate)?;
        let start = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(SandboxError::StartFunction)?;

        let exit_code = match start.call(&mut store, ()) {
            Ok(()) => 0,
            Err(error) => match error.downcast_ref::<I32Exit>() {
                Some(exit) => exit.0,
                None => return Err(SandboxError::Execution(error)),
            },
        };

        let stdout =
            String::from_utf8(stdout.contents().to_vec()).map_err(SandboxError::StdoutUtf8)?;
        let stderr =
            String::from_utf8(stderr.contents().to_vec()).map_err(SandboxError::StderrUtf8)?;

        Ok(RunOutcome {
            exit_code,
            stdout,
            stderr,
        })
    }
}

/// Runs one WASIp1 command module with only the requested filesystem grants.
///
/// One-shot convenience over [`SandboxRunner`]: builds a throwaway runner
/// with its own engine. Callers running many jobs should hold a
/// [`SandboxRunner`] instead so the compiled-code cache is reused.
pub fn run_module(req: RunRequest) -> Result<RunOutcome, SandboxError> {
    SandboxRunner::new().run(req)
}
