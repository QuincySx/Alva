// INPUT:  proxy::RemotePluginProxy, manifest::PluginManifest, tokio::fs, alva_agent_core::extension::*
// OUTPUT: SubprocessLoaderPlugin, LoaderError
// POS:    Phase 3 — the one Plugin the host registers; internally manages N subprocess plugins.

//! The first-party `Plugin` that scans a directory for plugin
//! packages, starts each one as a subprocess, and forwards host
//! middleware hooks to all loaded plugins.
//!
//! From the host's point of view this is just one more `Plugin`
//! — it goes through the normal `Plugin::register` lifecycle.
//! For each remote event subscription it registers a normal
//! `PhaseContribution`. Until the kernel has a native phase executor,
//! executable subscriptions also get an agent-core phase handler.
//!
//! ## Directory layout
//!
//! The loader accepts a **list** of extension directories, in
//! priority order. This matches the `SkillsPlugin` and
//! `McpPlugin` convention: typical setups pass the project dir
//! first and the global dir second, so project plugins shadow
//! same-named global plugins.
//!
//! ```text
//! <extensions_dir>/
//! ├── my-memory/
//! │   ├── alva.toml        # PluginManifest
//! │   └── main.py          # entry file declared in alva.toml
//! └── shell-guard/
//!     ├── alva.toml
//!     └── main.py
//! ```
//!
//! Plugins without an `alva.toml` are silently skipped; invalid
//! manifests log a warning and are ignored. One bad plugin never
//! prevents the others from loading — loading is best-effort
//! and never hard-fails. Duplicate plugin names across dirs keep
//! the first occurrence and log a warning. Within one directory,
//! plugin folders are loaded in sorted folder-name order so mutation
//! chains are deterministic.
//!
//! ## Hook dispatch
//!
//! Subprocess plugins are reached through phase contributions. Inside
//! `Plugin::register`, the loader async-loads the subprocesses
//! (`load_plugins`), then maps every remote `eventSubscriptions` entry to
//! a `PhaseContribution`. Each executable subscription registers an
//! `AepPhaseHandler`; agent-core compiles it into the current middleware
//! stack while the kernel still runs middleware hooks. `before_tool_call`
//! runs through the wrap chain in load order; `after_tool_call` runs in
//! reverse load order, matching the kernel middleware onion model.
//!
//! ### Registration timing
//!
//! Plugins load **asynchronously** inside `Plugin::register`, while
//! `Registrar::phase` / `Registrar::middleware` registration must happen
//! before the builder drains the host. The order works out: the builder
//! `await`s every plugin's `register` during the register phase and only
//! drains contributions **after** all of them have completed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use alva_agent_core::extension::{PhaseContribution, PhaseOrder, Plugin, Registrar};
use alva_kernel_abi::{Phase, PhaseEffect};
use async_trait::async_trait;

use crate::aep_bridge::AepPhaseHandler;
use crate::manifest::PluginManifest;
use crate::proxy::{ProxyError, RemotePluginProxy};
use crate::remote_tool::RemoteToolAdapter;

/// First-party `Plugin` that loads third-party subprocess plugins.
pub struct SubprocessLoaderPlugin {
    state: Arc<LoaderState>,
}

/// Shared state between the loader and its loaded plugin handles.
///
/// Held in an `Arc` so callers can hold a cheap reference. The plugin
/// list is filled in by `register` (async) via `load_plugins`.
struct LoaderState {
    extensions_dirs: Vec<PathBuf>,
    plugins: RwLock<Vec<Arc<RemotePluginProxy>>>,
}

impl SubprocessLoaderPlugin {
    /// Create a loader that will scan the given directories during
    /// `register`, in order. Earlier entries shadow later ones on
    /// name conflicts (first-wins) — typical callers pass the
    /// project dir before the global dir.
    pub fn new(extensions_dirs: Vec<PathBuf>) -> Self {
        Self {
            state: Arc::new(LoaderState {
                extensions_dirs,
                plugins: RwLock::new(Vec::new()),
            }),
        }
    }

    /// Scan + start every plugin in the configured directories and
    /// store the resulting proxies on `self`. Returns the number of
    /// plugins loaded.
    ///
    /// This does **not** register phase contributions — that happens in
    /// [`Plugin::register`], which calls this helper and then wires every
    /// remote subscription through the `Registrar`. Tests and integration
    /// harnesses that want to drive the loader without the full lifecycle
    /// can call this directly.
    pub async fn load_plugins(&self) -> Result<usize, LoaderError> {
        let plugins = scan_and_start(&self.state.extensions_dirs).await?;
        let count = plugins.len();

        let mut slot = self
            .state
            .plugins
            .write()
            .map_err(|_| LoaderError::StatePoisoned)?;
        *slot = plugins;
        Ok(count)
    }

    /// Number of currently-loaded plugins (after `load_plugins` /
    /// `register` has run).
    pub fn loaded_count(&self) -> usize {
        self.state.plugins.read().map(|p| p.len()).unwrap_or(0)
    }

    /// Consume the loader, calling `shutdown` on every plugin.
    ///
    /// Best-effort: errors on individual plugins are logged but do
    /// not stop the loop.
    pub async fn shutdown_all(self) -> Result<(), LoaderError> {
        let plugins = {
            let mut slot = self
                .state
                .plugins
                .write()
                .map_err(|_| LoaderError::StatePoisoned)?;
            std::mem::take(&mut *slot)
        };
        for plugin in plugins {
            // Try to take ownership of each Arc — if another
            // handler has a reference, skip graceful shutdown and
            // rely on kill_on_drop to clean up.
            match Arc::try_unwrap(plugin) {
                Ok(owned) => {
                    if let Err(e) = owned.shutdown().await {
                        tracing::warn!(error = %e, "plugin shutdown failed");
                    }
                }
                Err(arc) => {
                    tracing::warn!(
                        plugin = %arc.name(),
                        "plugin Arc still held elsewhere; relying on kill_on_drop"
                    );
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Plugin for SubprocessLoaderPlugin {
    fn name(&self) -> &str {
        "subprocess-loader"
    }

    fn description(&self) -> &str {
        "Dynamic loader for JS/Python plugins over AEP"
    }

    async fn register(&self, r: &Registrar) {
        // Async-load the subprocess plugins, then register each remote
        // event subscription as a phase contribution. While the kernel
        // still runs middleware hooks, executable subscriptions also get
        // a generated adapter middleware.
        // Best-effort: a load failure logs and registers nothing — one
        // broken loader never aborts the whole agent build.
        match self.load_plugins().await {
            Ok(count) => {
                tracing::info!(
                    count = count,
                    dirs = ?self.state.extensions_dirs,
                    "loaded subprocess plugins"
                );
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    dirs = ?self.state.extensions_dirs,
                    "failed to load subprocess plugins"
                );
                return;
            }
        }

        let plugins = match self.state.plugins.read() {
            Ok(guard) => guard.clone(),
            Err(_) => {
                tracing::error!("loader state poisoned; skipping remote phase registration");
                return;
            }
        };
        for plugin in plugins {
            for tool_def in &plugin.init_result().tools {
                r.tool(RemoteToolAdapter::new(plugin.clone(), tool_def.clone()));
            }
            for subscription in &plugin.init_result().event_subscriptions {
                let Some((phase, effect, order)) = aep_subscription_phase(subscription) else {
                    tracing::warn!(
                        plugin = %plugin.name(),
                        event = %subscription,
                        "plugin subscribed to unknown AEP event; it will never fire"
                    );
                    continue;
                };
                let contribution = PhaseContribution::new(
                    format!("aep:{}:{}", plugin.name(), subscription),
                    phase,
                    effect,
                    order,
                );
                if aep_subscription_has_middleware_adapter(subscription) {
                    r.phase_handler(Arc::new(AepPhaseHandler::new(
                        plugin.clone(),
                        subscription.clone(),
                        contribution,
                    )));
                } else {
                    r.phase(contribution);
                }
            }
        }
    }
}

// ===========================================================
// Subscription validation + directory scan
// ===========================================================

/// Map an AEP event subscription name (what plugins declare in their
/// `eventSubscriptions` list) to the canonical host event-type string.
///
/// The loader uses this to recognise (and warn about)
/// subscription names the host does not understand. Unknown names
/// return `None` so a future-protocol plugin does not break loading.
/// The non-obvious mappings: `on_agent_*` drop the `on_` prefix, and
/// `on_user_message` maps to `"input"` (the historical event-type name
/// for the user's message).
pub(crate) fn aep_to_core_event_type(aep_name: &str) -> Option<&'static str> {
    match aep_name {
        "before_tool_call" => Some("before_tool_call"),
        "after_tool_call" => Some("after_tool_call"),
        "on_agent_start" => Some("agent_start"),
        "on_agent_end" => Some("agent_end"),
        "on_user_message" => Some("input"),
        "on_llm_call_start" => Some("llm_call_start"),
        "on_llm_call_end" => Some("llm_call_end"),
        _ => None,
    }
}

fn aep_subscription_phase(aep_name: &str) -> Option<(Phase, PhaseEffect, PhaseOrder)> {
    aep_to_core_event_type(aep_name)?;
    match aep_name {
        "before_tool_call" => Some((Phase::BeforeToolCall, PhaseEffect::Wrap, PhaseOrder::Hooks)),
        "after_tool_call" => Some((Phase::AfterToolCall, PhaseEffect::Mutate, PhaseOrder::Hooks)),
        "on_agent_start" => Some((Phase::RunStart, PhaseEffect::Observe, PhaseOrder::Hooks)),
        "on_agent_end" => Some((Phase::RunEnd, PhaseEffect::Observe, PhaseOrder::Hooks)),
        "on_user_message" => Some((
            Phase::InputCommitted,
            PhaseEffect::Observe,
            PhaseOrder::Hooks,
        )),
        "on_llm_call_start" => Some((Phase::BeforeLlmCall, PhaseEffect::Mutate, PhaseOrder::Hooks)),
        "on_llm_call_end" => Some((Phase::AfterLlmCall, PhaseEffect::Mutate, PhaseOrder::Hooks)),
        _ => unreachable!("aep_to_core_event_type accepted an unmapped subscription"),
    }
}

fn aep_subscription_has_middleware_adapter(aep_name: &str) -> bool {
    matches!(
        aep_name,
        "before_tool_call"
            | "after_tool_call"
            | "on_agent_start"
            | "on_agent_end"
            | "on_user_message"
            | "on_llm_call_start"
            | "on_llm_call_end"
    )
}

/// Walk each directory in `dirs` (in order), parse every
/// subdirectory's `alva.toml`, and start a subprocess for each valid
/// plugin. Broken plugins log a warning and are skipped — one rotten
/// file never breaks the whole loader. Duplicate plugin names across
/// dirs keep the first occurrence (first-wins).
async fn scan_and_start(dirs: &[PathBuf]) -> Result<Vec<Arc<RemotePluginProxy>>, LoaderError> {
    let mut plugins: Vec<Arc<RemotePluginProxy>> = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for dir in dirs {
        scan_one_dir(dir, &mut plugins, &mut seen_names).await?;
    }

    Ok(plugins)
}

async fn scan_one_dir(
    dir: &Path,
    plugins: &mut Vec<Arc<RemotePluginProxy>>,
    seen_names: &mut std::collections::HashSet<String>,
) -> Result<(), LoaderError> {
    if !dir.exists() {
        tracing::debug!(dir = %dir.display(), "extensions dir does not exist, skipping scan");
        return Ok(());
    }

    for plugin_dir in plugin_dirs_in_dir(dir).await? {
        let manifest_path = plugin_dir.join("alva.toml");
        if !manifest_path.exists() {
            tracing::debug!(
                path = %plugin_dir.display(),
                "directory has no alva.toml, skipping"
            );
            continue;
        }

        let manifest_str = match tokio::fs::read_to_string(&manifest_path).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    path = %manifest_path.display(),
                    error = %e,
                    "failed to read manifest"
                );
                continue;
            }
        };

        let manifest: PluginManifest = match toml::from_str(&manifest_str) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    path = %manifest_path.display(),
                    error = %e,
                    "failed to parse manifest"
                );
                continue;
            }
        };

        let plugin_name = manifest.name.clone();

        if !seen_names.insert(plugin_name.clone()) {
            tracing::warn!(
                plugin = %plugin_name,
                dir = %plugin_dir.display(),
                "plugin name already loaded from an earlier directory, skipping"
            );
            continue;
        }

        match RemotePluginProxy::start(plugin_dir.clone(), manifest).await {
            Ok(proxy) => {
                tracing::info!(
                    plugin = %plugin_name,
                    dir = %plugin_dir.display(),
                    "plugin started"
                );
                plugins.push(Arc::new(proxy));
            }
            Err(e) => {
                tracing::error!(
                    plugin = %plugin_name,
                    error = %e,
                    "failed to start plugin"
                );
                // On failure, release the name so a later entry could try again.
                seen_names.remove(&plugin_name);
            }
        }
    }

    Ok(())
}

async fn plugin_dirs_in_dir(dir: &Path) -> Result<Vec<PathBuf>, LoaderError> {
    let mut entries = tokio::fs::read_dir(dir).await.map_err(LoaderError::Io)?;
    let mut plugin_dirs = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(LoaderError::Io)? {
        let plugin_dir = entry.path();
        let file_type = match entry.file_type().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(
                    path = %plugin_dir.display(),
                    error = %e,
                    "could not stat entry, skipping"
                );
                continue;
            }
        };
        if file_type.is_dir() {
            plugin_dirs.push(plugin_dir);
        }
    }
    Ok(sort_plugin_dirs(plugin_dirs))
}

fn sort_plugin_dirs(mut dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    dirs.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
        a_name.cmp(b_name).then_with(|| a.cmp(b))
    });
    dirs
}

// ===========================================================
// Public CLI-facing APIs (alva plugins list / exec)
// ===========================================================

/// Metadata discovered for one plugin without starting its subprocess.
/// Returned from [`discover_plugins`] — fast, cheap, suitable for
/// listing in `alva plugins list`.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    /// Absolute plugin directory (contains `alva.toml` + entry file).
    pub dir: PathBuf,
    /// Parsed `alva.toml`.
    pub manifest: PluginManifest,
}

/// Walk each directory and return the manifest metadata for every
/// plugin subdirectory that has a valid `alva.toml`. Does NOT start
/// subprocesses — use [`start_plugin`] for that.
///
/// Broken manifests are logged and skipped; the scan never fails
/// wholesale. Duplicate plugin names across directories yield
/// duplicate `DiscoveredPlugin` entries (caller decides dedup policy).
pub async fn discover_plugins(dirs: &[PathBuf]) -> Vec<DiscoveredPlugin> {
    let mut out = Vec::new();
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        let Ok(plugin_dirs) = plugin_dirs_in_dir(dir).await else {
            continue;
        };
        for plugin_dir in plugin_dirs {
            let manifest_path = plugin_dir.join("alva.toml");
            if !manifest_path.exists() {
                continue;
            }
            let Ok(s) = tokio::fs::read_to_string(&manifest_path).await else {
                continue;
            };
            let Ok(manifest) = toml::from_str::<PluginManifest>(&s) else {
                tracing::warn!(
                    path = %manifest_path.display(),
                    "failed to parse manifest"
                );
                continue;
            };
            out.push(DiscoveredPlugin {
                dir: plugin_dir,
                manifest,
            });
        }
    }
    out
}

/// Start a single plugin from its directory (containing `alva.toml`
/// + entry file). Used by CLI tools like `alva plugins exec` that
/// need one plugin running without the full loader lifecycle.
///
/// Returns the running [`RemotePluginProxy`] — caller is
/// responsible for `.shutdown()` when done.
pub async fn start_plugin(plugin_dir: PathBuf) -> Result<Arc<RemotePluginProxy>, LoaderError> {
    let manifest_path = plugin_dir.join("alva.toml");
    if !manifest_path.exists() {
        return Err(LoaderError::Manifest(format!(
            "no alva.toml in {}",
            plugin_dir.display()
        )));
    }
    let manifest_str = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(LoaderError::Io)?;
    let manifest: PluginManifest = toml::from_str(&manifest_str)
        .map_err(|e| LoaderError::Manifest(format!("parse {}: {e}", manifest_path.display())))?;
    let proxy = RemotePluginProxy::start(plugin_dir, manifest).await?;
    Ok(Arc::new(proxy))
}

// ===========================================================
// Error
// ===========================================================

#[derive(Debug, thiserror::Error)]
pub enum LoaderError {
    #[error("I/O error scanning extensions dir: {0}")]
    Io(#[source] std::io::Error),

    #[error("loader state mutex poisoned")]
    StatePoisoned,

    #[error("proxy error: {0}")]
    Proxy(#[from] ProxyError),

    #[error("manifest error: {0}")]
    Manifest(String),
}

#[cfg(test)]
mod tests {
    //! Tests for `aep_to_core_event_type` reverse-mapping +
    //! LoaderError contracts — 3 tests covering 3 distinct contracts.
    //!
    //! 1. **Subscription-name recognition**: `aep_to_core_event_type`
    //!    is what the loader uses to recognise (and warn
    //!    about) subscription names. Two non-obvious asymmetries
    //!    distinguish it from a naïve identity / strip-prefix:
    //!    - `on_agent_*` → `agent_*` (drop `on_` prefix; tool/* are
    //!      symmetric with no prefix to strip)
    //!    - `on_user_message` → `"input"` (NOT `"user_message"`; the
    //!      historical event-type for the user's message is literally
    //!      "input"; a naïve `strip_prefix("on_")` would break it)
    //!
    //!    Forward-compat: unknown names return None (NOT a fallback),
    //!    so future-protocol plugins don't break loading on names the
    //!    host doesn't know.
    //!
    //!    One parametric test pins all 7 known mappings + 2 unknown
    //!    cases in one pass.
    //!
    //! 2. **LoaderError wrapped Display chain-through**: only the
    //!    Proxy variant is pinned as representative of the
    //!    thiserror `#[error("X: {0}")]` chain-through pattern that
    //!    silently breaks if `{0}` is dropped. The same pattern
    //!    applies to the Io variant (with `#[source]`); the
    //!    literal-only StatePoisoned variant and the payload-only
    //!    Manifest variant test thiserror's derive macro itself
    //!    (deleted; see L152 dispatcher precedent).
    //!
    //! 3. **LoaderError From<ProxyError>**: the `#[from]` impl
    //!    produces the named variant for `?` callers; a variant
    //!    rename would silently break match arms in scan/start paths.
    use super::*;

    #[test]
    fn aep_to_core_event_type_maps_each_wire_name_per_table() {
        // Table-driven over 7 known mappings + 2 unknown/edge cases.
        // The "drops on_ prefix for agent_*" + "on_user_message →
        // input (NOT user_message)" asymmetries are the load-bearing
        // pins documented in the mod docstring; each row has a label
        // so panic output names the broken contract.
        let cases: &[(&str, Option<&str>, &str)] = &[
            // ── symmetric (no on_ prefix to strip)
            (
                "before_tool_call",
                Some("before_tool_call"),
                "symmetric tool call",
            ),
            (
                "after_tool_call",
                Some("after_tool_call"),
                "symmetric tool call",
            ),
            // ── drops on_ prefix
            ("on_agent_start", Some("agent_start"), "on_ prefix dropped"),
            ("on_agent_end", Some("agent_end"), "on_ prefix dropped"),
            // ── CRITICAL ASYMMETRY: NOT a naïve strip_prefix("on_")
            (
                "on_user_message",
                Some("input"),
                "on_user_message → input (NOT user_message)",
            ),
            // ── LLM hooks are now first-class phase subscriptions.
            (
                "on_llm_call_start",
                Some("llm_call_start"),
                "llm start phase",
            ),
            ("on_llm_call_end", Some("llm_call_end"), "llm end phase"),
            // ── forward-compat: unknown names return None
            ("totally_made_up_event", None, "unknown name → None"),
            ("", None, "empty string → None"),
        ];
        for (wire, expected, label) in cases {
            assert_eq!(
                aep_to_core_event_type(wire),
                *expected,
                "case {label:?} failed for wire name {wire:?}"
            );
        }
    }

    #[test]
    fn llm_subscriptions_are_executable_phase_handlers() {
        assert!(aep_subscription_has_middleware_adapter("on_llm_call_start"));
        assert!(aep_subscription_has_middleware_adapter("on_llm_call_end"));
    }

    #[test]
    fn after_tool_call_subscription_is_marked_mutating() {
        let (_, effect, _) =
            aep_subscription_phase("after_tool_call").expect("after_tool_call phase");
        assert_eq!(effect, PhaseEffect::Mutate);
    }

    #[test]
    fn before_tool_call_subscription_is_marked_wrapping() {
        let (_, effect, _) =
            aep_subscription_phase("before_tool_call").expect("before_tool_call phase");
        assert_eq!(effect, PhaseEffect::Wrap);
    }

    #[test]
    fn plugin_dirs_are_sorted_by_directory_name() {
        let sorted = sort_plugin_dirs(vec![
            PathBuf::from("/extensions/zeta"),
            PathBuf::from("/extensions/alpha"),
            PathBuf::from("/extensions/beta"),
        ]);
        let names: Vec<_> = sorted
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn loader_error_proxy_display_chains_inner_proxy_message_through() {
        // Representative wrapped-Display test: `#[error("proxy error:
        // {0}")]` chains ProxyError's Display via `{0}`. A refactor
        // dropping `{0}` would silently lose inner diagnostic. The
        // Io variant uses the same pattern (one representative
        // suffices); StatePoisoned + Manifest test thiserror's
        // literal / payload-interpolation derive (deleted; see
        // dispatcher L152 precedent).
        let bad = b"{not json";
        let serde_err = serde_json::from_slice::<serde_json::Value>(bad).unwrap_err();
        let proxy_err = ProxyError::Serialization(serde_err);
        let proxy_msg = proxy_err.to_string();
        let e = LoaderError::Proxy(proxy_err);
        let s = e.to_string();
        assert!(s.starts_with("proxy error:"), "prefix missing: {s}");
        assert!(s.contains(&proxy_msg), "inner proxy message lost: {s}");
    }

    #[test]
    fn from_proxy_error_produces_proxy_variant_for_question_mark_callers() {
        // Pin: `#[from] ProxyError` → callers using `?` get
        // LoaderError::Proxy(_). A variant rename would silently
        // break match arms in scan/start paths.
        let bad = b"{nope";
        let proxy_err = ProxyError::Serialization(
            serde_json::from_slice::<serde_json::Value>(bad).unwrap_err(),
        );
        let e: LoaderError = proxy_err.into();
        match e {
            LoaderError::Proxy(_) => {}
            other => panic!("expected Proxy variant, got {other:?}"),
        }
    }
}
