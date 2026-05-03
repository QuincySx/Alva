// INPUT:  std::sync, async_trait, alva_kernel_abi::{tool::Tool, bus_cap}, super::{Extension, ExtensionContext, HostAPI}
// OUTPUT: LspExtension, LspManager, LspDiagnostic, LspSeverity, StubLspManager, lsp_diagnostics tool
// POS:    LSP integration scaffold. Defines the bus capability + tool surface so
//         agent code can ask "what's wrong with this file" via LspExtension.
//         Real per-language server I/O (process spawn, JSON-RPC, didOpen/didChange,
//         publishDiagnostics subscription) is out of scope — see `TODO(real-lsp)`.

//! Language server scaffold — **DEPRECATED for end-user use**.
//!
//! This module remains as an experimental architectural placeholder.
//! Practical project diagnostics are now delivered by
//! [`alva_app_extension_tooling`] (cargo / tsc / eslint / biome / ruff /
//! pyright / go vet runners); see `wrappers/tooling.rs` in
//! `alva-agent-extension-builtin`. That route is faster to ship per
//! language, has zero runtime RAM cost, and matches the design choice of
//! both `amp` (Sourcegraph) and `pi-mono` — neither of which ships LSP.
//!
//! The trait shape here is preserved as a future-proof seam: if real LSP
//! integration is ever needed (e.g. for hover / find-references that
//! `cargo check` can't provide), `LspManager` can be re-implemented
//! against a real LSP backend without touching the extension/tool
//! contract. No active development planned.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use alva_kernel_abi::{bus_cap, Tool};

use super::{Extension, ExtensionContext};

mod tool_diagnostics;

pub use tool_diagnostics::LspDiagnosticsTool;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LspDiagnostic {
    pub severity: LspSeverity,
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based column number on the start position.
    pub col: u32,
    pub message: String,
    /// Origin tag (e.g. `"rust-analyzer"`, `"tsserver"`, `"pyright"`).
    pub source: String,
}

/// Configuration for one language server entry. Real impls (TODO) will
/// use these to spawn and route file-extension globs to the right server.
#[derive(Debug, Clone)]
pub struct LspServerConfig {
    pub language: String,
    pub command: String,
    pub args: Vec<String>,
    pub root_patterns: Vec<String>,
}

impl LspServerConfig {
    pub fn rust_analyzer() -> Self {
        Self {
            language: "rust".into(),
            command: "rust-analyzer".into(),
            args: vec![],
            root_patterns: vec!["Cargo.toml".into()],
        }
    }

    pub fn typescript() -> Self {
        Self {
            language: "typescript".into(),
            command: "typescript-language-server".into(),
            args: vec!["--stdio".into()],
            root_patterns: vec!["package.json".into(), "tsconfig.json".into()],
        }
    }

    pub fn pyright() -> Self {
        Self {
            language: "python".into(),
            command: "pyright-langserver".into(),
            args: vec!["--stdio".into()],
            root_patterns: vec!["pyproject.toml".into(), "setup.py".into()],
        }
    }
}

// ---------------------------------------------------------------------------
// LspManager bus capability
// ---------------------------------------------------------------------------

/// Bus Capability: query LSP diagnostics for a file. Decoupled from the
/// concrete server transport so a stub impl can satisfy tests and a real
/// LSP-backed impl can land later without API churn.
///
/// **Provider**: `LspExtension::configure` registers `StubLspManager` by
/// default; a real LSP-backed impl will register on the same trait.
/// **Consumers**: `lsp_diagnostics` tool, plus any future
/// `LspDiagnosticsMiddleware` (D5, deferred).
/// **Why bus**: keeps the manager replaceable (default-replacement
/// contract) and avoids LSP I/O code creeping into kernel-core.
#[bus_cap]
pub trait LspManager: Send + Sync {
    /// Snapshot diagnostics for `path`. Empty vec = no diagnostics or
    /// no server attached for this language.
    fn diagnostics(&self, path: &std::path::Path) -> Vec<LspDiagnostic>;

    /// Push diagnostics for `path`. Used by tests / stubs and by future
    /// LSP-backed impls processing `publishDiagnostics` notifications.
    fn set_diagnostics(&self, path: &std::path::Path, diags: Vec<LspDiagnostic>);
}

// ---------------------------------------------------------------------------
// StubLspManager — in-memory backend (tests + no-op default)
// ---------------------------------------------------------------------------

/// In-memory `LspManager` — diagnostics are whatever was last `set_*`'d.
/// Useful for tests and as a no-op default when a real LSP isn't wired.
pub struct StubLspManager {
    state: Mutex<HashMap<PathBuf, Vec<LspDiagnostic>>>,
}

impl StubLspManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for StubLspManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LspManager for StubLspManager {
    fn diagnostics(&self, path: &std::path::Path) -> Vec<LspDiagnostic> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
            .cloned()
            .unwrap_or_default()
    }

    fn set_diagnostics(&self, path: &std::path::Path, diags: Vec<LspDiagnostic>) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_path_buf(), diags);
    }
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

pub struct LspExtension {
    servers: Vec<LspServerConfig>,
    manager: OnceManager,
}

#[derive(Default)]
struct OnceManager(std::sync::OnceLock<Arc<dyn LspManager>>);

impl LspExtension {
    /// Default config: rust-analyzer + tsserver + pyright. Servers that
    /// aren't installed locally are tolerated — `StubLspManager` ignores
    /// the config entirely, and a real impl (TODO) should warn-and-skip.
    pub fn new() -> Self {
        Self {
            servers: vec![
                LspServerConfig::rust_analyzer(),
                LspServerConfig::typescript(),
                LspServerConfig::pyright(),
            ],
            manager: OnceManager::default(),
        }
    }

    pub fn with_servers(servers: Vec<LspServerConfig>) -> Self {
        Self {
            servers,
            manager: OnceManager::default(),
        }
    }

    /// List of declared language-server configs.
    pub fn server_configs(&self) -> &[LspServerConfig] {
        &self.servers
    }
}

impl Default for LspExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for LspExtension {
    fn name(&self) -> &str {
        "lsp"
    }

    fn description(&self) -> &str {
        "Language Server Protocol integration (diagnostics today; semantic queries planned)"
    }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        // The tool needs the `LspManager` Arc but extensions receive the
        // bus via `configure()`, not `tools()`. Solve by delegating: the
        // tool reads the bus at execute time.
        vec![Box::new(LspDiagnosticsTool::new())]
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        // Default backend: in-memory stub. Real LSP-backed impl lands
        // later by registering a different `dyn LspManager` on the bus
        // (default-replacement contract — last writer wins).
        // TODO(real-lsp): spawn the configured servers via tokio process,
        //                 wire JSON-RPC, subscribe to publishDiagnostics,
        //                 register the resulting impl here in place of
        //                 `StubLspManager`.
        let mgr: Arc<dyn LspManager> = Arc::new(StubLspManager::new());
        let _ = self.manager.0.set(mgr.clone());
        ctx.bus_writer.provide::<dyn LspManager>(mgr);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture_diag() -> LspDiagnostic {
        LspDiagnostic {
            severity: LspSeverity::Error,
            line: 10,
            col: 4,
            message: "expected `;` after expression".into(),
            source: "rust-analyzer".into(),
        }
    }

    #[test]
    fn stub_returns_empty_for_unknown_path() {
        let m = StubLspManager::new();
        let d = m.diagnostics(Path::new("/nonexistent.rs"));
        assert!(d.is_empty());
    }

    #[test]
    fn stub_round_trips_diagnostics() {
        let m = StubLspManager::new();
        let p = Path::new("/src/foo.rs");
        m.set_diagnostics(p, vec![fixture_diag()]);
        let d = m.diagnostics(p);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].severity, LspSeverity::Error);
        assert_eq!(d[0].line, 10);
    }

    #[test]
    fn stub_overwrites_per_path() {
        let m = StubLspManager::new();
        let p = Path::new("/src/foo.rs");
        m.set_diagnostics(p, vec![fixture_diag()]);
        m.set_diagnostics(p, vec![]);
        assert!(m.diagnostics(p).is_empty());
    }

    #[test]
    fn diagnostic_serializes_with_snake_case_severity() {
        let json = serde_json::to_string(&fixture_diag()).unwrap();
        assert!(json.contains("\"severity\":\"error\""));
        let back: LspDiagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(back, fixture_diag());
    }

    #[test]
    fn extension_metadata_and_default_servers() {
        let e = LspExtension::new();
        assert_eq!(e.name(), "lsp");
        assert!(!e.description().is_empty());
        assert_eq!(e.server_configs().len(), 3, "rust + ts + pyright by default");
        let langs: Vec<&str> = e.server_configs().iter().map(|c| c.language.as_str()).collect();
        assert!(langs.contains(&"rust"));
        assert!(langs.contains(&"typescript"));
        assert!(langs.contains(&"python"));
    }
}
