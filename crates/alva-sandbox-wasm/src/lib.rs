// INPUT:  std::{path, string, sync, thread, time}, alva_llm_wire, alva_sandbox_abi, reqwest, thiserror, wasmtime, wasmtime_wasi::{p1, pipe, WasiCtxBuilder}
// OUTPUT: Access, Grant, RunLimits, RunRequest, RunOutcome, SandboxError, SandboxRunner, SandboxStoreData, MAX_WASM_MODULE_BYTES, AuditEvent, proxy registrars, translate_guest_cwd, validate_allowed_domain_pattern, run_module
// POS:    Native WASIp1 runner boundary enforcing per-call filesystem/network grants, context delivery, escalation mapping, resource limits, and captured results.

//! Host-side runner for WASIp1 command modules.
//!
//! The guest receives no ambient filesystem access. Each [`Grant`] is opened
//! by Wasmtime and exposed as one WASI preopen for the lifetime of a single
//! run.
//!
//! Hold a [`SandboxRunner`] to execute many jobs against one shared [`Engine`]
//! configuration; every run compiles its supplied module and gets a fresh
//! `Store` + WASI context, so isolation is per-run. [`run_module`] is a
//! one-shot convenience that builds a throwaway runner.

mod context_proxy;
mod escalation_proxy;
mod http_proxy;
mod llm_proxy;
mod log_proxy;

pub use alva_sandbox_abi::{
    AuditEvent, EscalationProxyRequest, EscalationProxyResult, EscalationResponse,
    WasmEnvironmentContext,
};
pub use context_proxy::register_wasm_environment_context_proxy;
pub use escalation_proxy::{register_escalation_proxy, translate_guest_cwd};
pub use http_proxy::validate_allowed_domain_pattern;
pub use llm_proxy::register_llm_proxy;
pub use log_proxy::register_job_log_proxy;

use std::path::PathBuf;
use std::string::FromUtf8Error;
use std::time::Duration;

use thiserror::Error;
use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Trap};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{DirPerms, FilePerms, I32Exit, WasiCtxBuilder};

const OUTPUT_CAPACITY: usize = 1024 * 1024;
/// Maximum encoded module size accepted before Wasmtime validation/compilation.
///
/// Store memory, fuel, and epoch limits begin only after compilation, so this
/// host-side byte ceiling bounds the otherwise-unmetered compiler input.
pub const MAX_WASM_MODULE_BYTES: usize = 64 * 1024 * 1024;
const EPOCH_TICK: Duration = Duration::from_millis(10);
// A coarse elapsed-time backstop for guest execution. Epoch interruption is
// checked while Wasm instructions run; it cannot preempt a synchronous host
// import, so every caller-supplied blocking callback must enforce its own
// timeout/cancellation. Fuel below is the tighter CPU-spin ceiling. CLI callers
// can override this value via `--wasm-wall-clock-secs` /
// `ALVA_WASM_WALL_CLOCK_SECS` (see alva-app-cli).
const DEFAULT_WALL_CLOCK: Duration = Duration::from_secs(3600);
const DEFAULT_MAX_MEMORY_BYTES: usize = 256 * 1024 * 1024;
// Real fuel ceiling, not pure metering: this is the enforced budget a guest
// gets before `store.set_fuel` is exhausted and a `Trap::OutOfFuel` cuts it
// off.
//
// Derivation: `spin_fuel_rate_is_within_expected_order_of_magnitude` in
// tests/runner.rs times a fuel-bounded (not wall-clock-bounded) run of the
// non-yielding "spin" fixture and consistently measures ~1.7e10-3.2e10 fuel
// per second of real time on development hardware, across fuel budgets
// spanning 2e9-5e10. (An earlier attempt derived the rate from a
// *wall-clock*-triggered trap instead; that measured a fuel figure that
// stayed constant regardless of the configured deadline, which turned out to
// be fixed guest-startup cost leaking into the epoch-trap path rather than
// loop-iteration throughput -- see that test's doc comment. Timing a real
// `Trap::OutOfFuel` avoids the confound.)
//
// This budget targets a ~20s time-to-cutoff for a genuine non-yielding spin
// at the measured rate: 2e10 fuel/sec * 20s = 4e11. That is dramatically
// tighter than the 60-minute wall-clock backstop above (a ~180x tighter
// bound in the worst case, and the two mechanisms are independent: fuel
// tracks CPU burned executing wasm instructions, wall clock tracks elapsed
// real time including blocked-on-host-call waiting, so this catches the
// "the guest itself is spinning" case specifically that wall clock alone
// leaves to a 60-minute leash).
//
// Because both the ceiling and every workload it is compared against run on
// the *same* engine, expressing the ceiling as "N seconds' worth of this
// engine's own measured native rate" keeps the headroom ratios
// self-consistent regardless of whether the deployment host is faster or
// slower than the measurement host:
//   - a real knowledge-base -p run (7 model turns): ~17.0M fuel measured
//     -> ~23,500x headroom
//   - one QuickJS agent-loop turn incl. a tool call: ~5M fuel measured
//     -> ~80,000x headroom
//   - the worst legitimate case, a `run_script` guest burning its own 10s
//     timeout at full wasm-native speed: ~2e11 fuel (10s * 2e10/s, i.e. half
//     of this budget) -> ~2x headroom, so a legal 10s script is not starved
//     by a fuel ceiling tighter than its own timeout, while a spin that
//     doesn't stop at 10s keeps burning into the other half and traps.
// CLI callers can override per run via `--wasm-fuel` / `ALVA_WASM_FUEL` (see
// alva-app-cli) for workloads that legitimately need more.
const DEFAULT_WASM_FUEL: u64 = 400_000_000_000;

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

/// Resource ceilings for one WASIp1 module invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunLimits {
    /// Maximum elapsed wall-clock time before the engine epoch traps executing
    /// guest instructions. Synchronous host imports must enforce their own
    /// deadline because epoch interruption cannot preempt callback code.
    pub wall_clock: Duration,
    /// Maximum bytes for each WebAssembly linear memory in the store.
    pub max_memory_bytes: usize,
    /// Maximum Wasmtime fuel units the guest may consume before a
    /// `Trap::OutOfFuel` cuts it off. Fuel tracks wasm-instruction execution
    /// specifically: a guest blocked on a host call (for example waiting on
    /// a model completion) does not spend fuel while it waits, so this is a
    /// precise CPU-spin gate, unlike the coarse wall-clock backstop above.
    pub fuel: u64,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            wall_clock: DEFAULT_WALL_CLOCK,
            max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
            fuel: DEFAULT_WASM_FUEL,
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
    /// Hostnames the untrusted guest may reach through the fetch import.
    /// Empty is fail-closed. Patterns use exact host matching unless prefixed
    /// by `*.`; ports are intentionally not part of matching.
    pub allowed_domains: Vec<String>,
    /// Per-run wall-clock and linear-memory ceilings. Callers such as the CLI
    /// may override these without rebuilding the runner.
    pub limits: RunLimits,
}

/// Store data shared by WASI and caller-registered host imports.
///
/// Fields stay private so host imports cannot bypass the runner's limiter.
pub struct SandboxStoreData {
    wasi: WasiP1Ctx,
    limits: StoreLimits,
}

impl SandboxStoreData {
    /// Read the WASIp1 context from a caller-provided host import.
    pub fn wasi(&self) -> &WasiP1Ctx {
        &self.wasi
    }

    /// Mutably access the WASIp1 context from a caller-provided host import.
    /// The resource limiter remains private and cannot be replaced.
    pub fn wasi_mut(&mut self) -> &mut WasiP1Ctx {
        &mut self.wasi
    }
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
    /// Wasmtime fuel units consumed executing guest instructions this run.
    ///
    /// The store is seeded with `RunLimits::fuel` before the guest starts and
    /// this is `RunLimits::fuel - store.get_fuel()` at completion, so it
    /// reflects real guest CPU work. Host calls the guest blocks on (for
    /// example waiting on a model completion) do not consume fuel, so this
    /// tracks wasm-instruction work specifically. A run that exhausts its
    /// budget traps (see [`SandboxError::Execution`]) rather than reaching
    /// this field, so on success this is always strictly less than the
    /// budget that was set.
    pub fuel_consumed: u64,
}

/// Failure to configure or execute a guest module.
#[derive(Debug, Error)]
pub enum SandboxError {
    /// The encoded module exceeds the pre-compilation host input ceiling.
    #[error("WASIp1 module is {actual} bytes; limit is {limit} bytes")]
    ModuleTooLarge { actual: usize, limit: usize },

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

    /// Caller-provided host imports could not be registered with the linker.
    #[error("failed to register host imports: {0}")]
    HostImports(#[source] wasmtime::Error),

    /// The module could not be instantiated with the configured WASI imports.
    #[error("failed to instantiate WASIp1 module: {0}")]
    Instantiate(#[source] wasmtime::Error),

    /// The module does not expose the WASI command entry point with the
    /// expected signature.
    #[error("failed to resolve WASIp1 `_start` entry point: {0}")]
    StartFunction(#[source] wasmtime::Error),

    /// The guest trapped for a reason other than WASI `proc_exit`. This
    /// covers the wall-clock deadline, the linear-memory limiter, the fuel
    /// budget, and any other Wasmtime execution failure.
    #[error("WASIp1 module execution failed: {source}")]
    Execution {
        /// The underlying Wasmtime error or trap.
        #[source]
        source: wasmtime::Error,
        /// Fuel consumed by the guest before this trap, computed the same
        /// way as [`RunOutcome::fuel_consumed`]. Present for every
        /// `Execution` error, not only fuel-related ones, so callers can
        /// tell a genuine CPU-bound spin (high fuel burned quickly) apart
        /// from a wall-clock trap on a guest that was mostly blocked on a
        /// host call (low fuel despite the long wall-clock spend).
        fuel_consumed: u64,
    },

    /// Captured stdout was not valid UTF-8.
    #[error("guest stdout is not valid UTF-8: {0}")]
    StdoutUtf8(#[source] FromUtf8Error),

    /// Captured stderr was not valid UTF-8.
    #[error("guest stderr is not valid UTF-8: {0}")]
    StderrUtf8(#[source] FromUtf8Error),
}

impl SandboxError {
    /// Fuel consumed by the guest before an [`SandboxError::Execution`]
    /// trap. `None` for every other variant, since those fail before any
    /// guest instruction runs.
    pub fn fuel_consumed(&self) -> Option<u64> {
        match self {
            SandboxError::Execution { fuel_consumed, .. } => Some(*fuel_consumed),
            _ => None,
        }
    }
}

/// A reusable host runner that owns one shared [`Engine`].
///
/// Wasmtime is designed for one `Engine` shared across many `Store`s: the
/// engine carries shared compilation configuration/resources, while each
/// `Store` is the isolation boundary. Every run compiles its supplied bytes and
/// builds a fresh WASI context and `Store`, so no state leaks between runs.
///
/// [`run`]: SandboxRunner::run
pub struct SandboxRunner {
    inner: std::sync::Arc<RunnerInner>,
}

struct RunnerInner {
    engine: Engine,
    epoch_cancel: std::sync::mpsc::Sender<()>,
    epoch_thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Drop for RunnerInner {
    fn drop(&mut self) {
        let _ = self.epoch_cancel.send(());
        if let Some(thread) = self.epoch_thread.lock().expect("epoch thread lock").take() {
            let _ = thread.join();
        }
    }
}

impl Clone for SandboxRunner {
    fn clone(&self) -> Self {
        Self {
            inner: std::sync::Arc::clone(&self.inner),
        }
    }
}

impl Default for SandboxRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxRunner {
    /// Build a runner with a fresh shared engine.
    pub fn new() -> Self {
        let mut config = Config::new();
        config.epoch_interruption(true);
        config.consume_fuel(true);
        let engine = Engine::new(&config).expect(
            "epoch interruption and fuel consumption are valid Wasmtime engine configurations",
        );
        let deadline_engine = engine.clone();
        let (epoch_cancel, epoch_wait) = std::sync::mpsc::channel();
        let epoch_thread = std::thread::spawn(move || {
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                epoch_wait.recv_timeout(EPOCH_TICK)
            {
                deadline_engine.increment_epoch();
            }
        });
        Self {
            inner: std::sync::Arc::new(RunnerInner {
                engine,
                epoch_cancel,
                epoch_thread: std::sync::Mutex::new(Some(epoch_thread)),
            }),
        }
    }

    /// Runs one WASIp1 command module with only the requested filesystem
    /// grants, on this runner's shared engine.
    ///
    /// A fresh linker, WASI context and store are created on every call. Host
    /// stdio, environment variables and filesystem paths are not inherited.
    pub fn run(&self, req: RunRequest) -> Result<RunOutcome, SandboxError> {
        self.run_with_imports(req, |_| Ok(()))
    }

    /// Runs one WASIp1 command module after registering per-run host imports.
    ///
    /// The callback receives the fresh linker after WASI imports have been
    /// installed and before the module is instantiated. It is intentionally
    /// synchronous: imported functions execute on the guest's calling thread,
    /// matching the blocking LLM-proxy ABI selected for the spike.
    pub fn run_with_imports<F>(
        &self,
        req: RunRequest,
        register: F,
    ) -> Result<RunOutcome, SandboxError>
    where
        F: FnOnce(&mut Linker<SandboxStoreData>) -> Result<(), wasmtime::Error>,
    {
        let limits = req.limits;
        let allowed_domains = req.allowed_domains;
        enforce_module_size(req.module.len())?;
        let module = Module::new(&self.inner.engine, &req.module).map_err(SandboxError::Module)?;

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
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(limits.max_memory_bytes)
            .trap_on_grow_failure(true)
            .build();
        let mut linker: Linker<SandboxStoreData> = Linker::new(&self.inner.engine);
        // The guest declares its host imports unconditionally, so every one of
        // them must resolve or instantiation fails with "unknown import" at
        // runtime. Defaults are installed here and `register` may shadow them,
        // which means a caller can only *change* behaviour, never forget an
        // import into a runtime error. Audit logging is off unless asked for;
        // the LLM proxy has no sensible default and stays the caller's job.
        linker.allow_shadowing(true);
        p1::add_to_linker_sync(&mut linker, |data| &mut data.wasi)
            .map_err(SandboxError::WasiLinker)?;
        http_proxy::register_http_proxy(&mut linker, allowed_domains)
            .map_err(SandboxError::HostImports)?;
        log_proxy::register_job_log_proxy(&mut linker, |_event| Ok(()))
            .map_err(SandboxError::HostImports)?;
        context_proxy::register_wasm_environment_context_proxy(&mut linker, || {
            Ok(alva_sandbox_abi::WasmEnvironmentContext::new(""))
        })
        .map_err(SandboxError::HostImports)?;
        escalation_proxy::register_escalation_proxy(&mut linker, |_request| {
            Ok(alva_sandbox_abi::EscalationProxyResult::failure(
                "host escalation is not configured for this run",
            ))
        })
        .map_err(SandboxError::HostImports)?;
        register(&mut linker).map_err(SandboxError::HostImports)?;

        let mut store = Store::new(
            &self.inner.engine,
            SandboxStoreData {
                wasi,
                limits: store_limits,
            },
        );
        store.limiter(|data| &mut data.limits);
        store.set_epoch_deadline(epoch_ticks(limits.wall_clock));
        store.epoch_deadline_trap();
        // Real enforced ceiling: set after the store is created because fuel
        // is exhausted at zero by default. See `DEFAULT_WASM_FUEL` for the
        // default budget and its derivation; `RunLimits::fuel` lets callers
        // override it per run.
        store
            .set_fuel(limits.fuel)
            .expect("fuel consumption is enabled by this runner's engine config");
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(SandboxError::Instantiate)?;
        let start = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(SandboxError::StartFunction)?;

        let run_started = std::time::Instant::now();
        let call_result = start.call(&mut store, ());
        // `u64`, not the `u128` `Duration::as_millis()` returns: a run
        // lasting anywhere near `u64::MAX` ms (585 million years) is not a
        // real scenario, and `tracing`'s field-recording API dispatches
        // `u128` values through a distinct `record_u128` visitor method,
        // which is easy for a log consumer to forget to implement (as a
        // regression here already demonstrated for a test-side subscriber).
        let elapsed_ms = u64::try_from(run_started.elapsed().as_millis()).unwrap_or(u64::MAX);

        // Computed unconditionally so it covers every exit path below: a
        // normal return, a WASI proc_exit, an epoch/memory/fuel trap, and
        // any other execution error all fall through this same measurement
        // before the match decides how to report the outcome.
        let fuel_consumed = limits.fuel.saturating_sub(
            store
                .get_fuel()
                .expect("fuel consumption is enabled by this runner's engine config"),
        );
        tracing::info!(fuel_consumed, elapsed_ms, "wasm run consumed fuel");

        let exit_code = match call_result {
            Ok(()) => 0,
            Err(error) => match error.downcast_ref::<I32Exit>() {
                Some(exit) => exit.0,
                None if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt) => {
                    return Err(SandboxError::Execution {
                        source: wasmtime::Error::msg(format!(
                            "wall-clock limit of {} ms exceeded",
                            limits.wall_clock.as_millis()
                        )),
                        fuel_consumed,
                    });
                }
                None if error.downcast_ref::<Trap>() == Some(&Trap::OutOfFuel) => {
                    return Err(SandboxError::Execution {
                        source: wasmtime::Error::msg(format!(
                            "fuel budget of {} exceeded (possible runaway guest CPU spin)",
                            limits.fuel
                        )),
                        fuel_consumed,
                    });
                }
                None if format!("{error:#}").contains("growing memory") => {
                    return Err(SandboxError::Execution {
                        source: wasmtime::Error::msg(format!(
                            "linear-memory limit of {} bytes exceeded: {error}",
                            limits.max_memory_bytes
                        )),
                        fuel_consumed,
                    });
                }
                None => {
                    return Err(SandboxError::Execution {
                        source: error,
                        fuel_consumed,
                    })
                }
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
            fuel_consumed,
        })
    }
}

fn epoch_ticks(wall_clock: Duration) -> u64 {
    let ticks = wall_clock.as_nanos().div_ceil(EPOCH_TICK.as_nanos());
    u64::try_from(ticks.max(1)).unwrap_or(u64::MAX)
}

fn enforce_module_size(actual: usize) -> Result<(), SandboxError> {
    if actual > MAX_WASM_MODULE_BYTES {
        Err(SandboxError::ModuleTooLarge {
            actual,
            limit: MAX_WASM_MODULE_BYTES,
        })
    } else {
        Ok(())
    }
}

/// Runs one WASIp1 command module with only the requested filesystem grants.
///
/// One-shot convenience over [`SandboxRunner`]: builds a throwaway runner
/// with its own engine. Callers running many jobs should hold a
/// [`SandboxRunner`] to reuse engine configuration/resources.
pub fn run_module(req: RunRequest) -> Result<RunOutcome, SandboxError> {
    SandboxRunner::new().run(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_size_is_rejected_before_compilation() {
        assert!(enforce_module_size(MAX_WASM_MODULE_BYTES).is_ok());
        let error = enforce_module_size(MAX_WASM_MODULE_BYTES + 1).unwrap_err();
        assert!(matches!(
            error,
            SandboxError::ModuleTooLarge {
                actual,
                limit: MAX_WASM_MODULE_BYTES
            } if actual == MAX_WASM_MODULE_BYTES + 1
        ));
    }
}
