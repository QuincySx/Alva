// INPUT:  proxy::RemoteExtensionProxy, manifest::PluginManifest, tokio::fs, alva_agent_core::extension::*
// OUTPUT: SubprocessLoaderExtension, LoaderError
// POS:    Phase 3 — the one Extension the host registers; internally manages N subprocess plugins.

//! The first-party `Extension` that scans a directory for plugin
//! packages, starts each one as a subprocess, and forwards host
//! events to all loaded plugins.
//!
//! From the host's point of view this is just one more `Extension`
//! — it goes through the normal `activate` / `configure` lifecycle
//! and participates in the same event dispatch mechanism as
//! `CheckpointExtension` or `PlanModeExtension`.
//!
//! ## Directory layout
//!
//! The loader accepts a **list** of extension directories, in
//! priority order. This matches the `SkillsExtension` and
//! `McpExtension` convention: typical setups pass the project dir
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
//! prevents the others from loading — `configure` is best-effort
//! and never hard-fails. Duplicate plugin names across dirs keep
//! the first occurrence and log a warning.
//!
//! ## Event dispatch
//!
//! During `activate`, this extension subscribes to **every**
//! `ExtensionEvent` variant the host currently defines. Each
//! registered handler iterates over all loaded plugins
//! sequentially; the first plugin that returns `Block` wins (same
//! semantics as `ExtensionHost::emit`'s own loop). This matches the
//! user's mental model where plugins compose the way first-party
//! extensions already do.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use alva_agent_core::extension::{
    Extension, ExtensionContext, HostAPI,
};
use async_trait::async_trait;

use crate::manifest::PluginManifest;
use crate::proxy::{ProxyError, RemoteExtensionProxy};

/// First-party `Extension` that loads third-party subprocess plugins.
pub struct SubprocessLoaderExtension {
    state: Arc<LoaderState>,
}

/// Shared state between the extension and all handler closures.
///
/// Held in an `Arc` so `activate`'s handler closures can own a
/// reference without borrowing `&self`. The plugin list is filled in
/// by `configure` (async) after handlers have already been
/// registered by `activate` (sync).
struct LoaderState {
    extensions_dirs: Vec<PathBuf>,
    plugins: RwLock<Vec<Arc<RemoteExtensionProxy>>>,
    /// Cloned from the `HostAPI` handed to us during `activate()`.
    /// Used by `configure` to register per-plugin handlers via
    /// `api.on_as(plugin_name, ...)`.
    api: RwLock<Option<HostAPI>>,
}

impl SubprocessLoaderExtension {
    /// Create a loader that will scan the given directories during
    /// `configure`, in order. Earlier entries shadow later ones on
    /// name conflicts (first-wins) — typical callers pass the
    /// project dir before the global dir.
    pub fn new(extensions_dirs: Vec<PathBuf>) -> Self {
        Self {
            state: Arc::new(LoaderState {
                extensions_dirs,
                plugins: RwLock::new(Vec::new()),
                api: RwLock::new(None),
            }),
        }
    }

    /// Force-load plugins now without going through the `configure`
    /// lifecycle. Tests and integration harnesses use this; real
    /// hosts rely on the lifecycle calling `configure` for them.
    ///
    /// If `activate()` has already been called (which stores the
    /// HostAPI handle), loaded plugins are additionally registered
    /// with the host's handler table under their own names.
    pub async fn load_plugins(&self) -> Result<usize, LoaderError> {
        let plugins = scan_and_start(&self.state.extensions_dirs).await?;
        let count = plugins.len();

        // Register per-plugin handlers if we have a HostAPI.
        {
            let api_guard = self
                .state
                .api
                .read()
                .map_err(|_| LoaderError::StatePoisoned)?;
            if let Some(ref api) = *api_guard {
                for plugin in &plugins {
                    register_plugin_handlers(api, plugin);
                }
            }
        }

        let mut slot = self
            .state
            .plugins
            .write()
            .map_err(|_| LoaderError::StatePoisoned)?;
        *slot = plugins;
        Ok(count)
    }

    /// Number of currently-loaded plugins (after `load_plugins` /
    /// `configure` has run).
    pub fn loaded_count(&self) -> usize {
        self.state
            .plugins
            .read()
            .map(|p| p.len())
            .unwrap_or(0)
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
impl Extension for SubprocessLoaderExtension {
    fn name(&self) -> &str {
        "subprocess-loader"
    }

    fn description(&self) -> &str {
        "Dynamic loader for JS/Python plugins over AEP"
    }

    fn activate(&self, api: &HostAPI) {
        // Store the API handle so configure() can register per-plugin
        // handlers via `api.on_as(plugin_name, ...)`. We deliberately
        // register ZERO handlers here — the loader itself is plumbing
        // and should be invisible in the host's handler registry.
        // Individual plugins appear by their own name once configure()
        // (or load_plugins()) runs.
        let mut api_slot = self
            .state
            .api
            .write()
            .unwrap_or_else(|e| e.into_inner());
        *api_slot = Some(api.clone());
        tracing::debug!(
            dirs = ?self.state.extensions_dirs,
            "SubprocessLoaderExtension: api stored, waiting for configure"
        );
    }

    async fn configure(&self, _ctx: &ExtensionContext) {
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
            }
        }
    }
}

// ===========================================================
// Per-plugin handler registration + directory scan
// ===========================================================

/// Register one handler per subscribed event for this plugin, each
/// attributed to the plugin's own name via `HostAPI::on_as`. This
/// makes individual plugins visible in the host's handler registry
/// and event-source logging.
fn register_plugin_handlers(api: &HostAPI, plugin: &Arc<RemoteExtensionProxy>) {
    for aep_name in &plugin.init_result().event_subscriptions {
        let Some(core_event_type) = aep_to_core_event_type(aep_name) else {
            tracing::warn!(
                plugin = %plugin.name(),
                event = %aep_name,
                "plugin subscribed to unknown AEP event; skipping handler"
            );
            continue;
        };
        let proxy = Arc::clone(plugin);
        let plugin_name = plugin.name().to_string();
        api.on_as(&plugin_name, core_event_type, move |event| {
            proxy.dispatch_event_sync(event)
        });
        tracing::debug!(
            plugin = %plugin.name(),
            event_type = core_event_type,
            "registered handler"
        );
    }
}

/// Map an AEP event subscription name (what plugins declare in their
/// `eventSubscriptions` list) to the core `ExtensionEvent::event_type`
/// string (what `HostAPI::on_as` uses to key into the handler table).
fn aep_to_core_event_type(aep_name: &str) -> Option<&'static str> {
    match aep_name {
        "before_tool_call" => Some("before_tool_call"),
        "after_tool_call" => Some("after_tool_call"),
        "on_agent_start" => Some("agent_start"),
        "on_agent_end" => Some("agent_end"),
        "on_user_message" => Some("input"),
        _ => None,
    }
}

/// Walk each directory in `dirs` (in order), parse every
/// subdirectory's `alva.toml`, and start a subprocess for each valid
/// plugin. Broken plugins log a warning and are skipped — one rotten
/// file never breaks the whole loader. Duplicate plugin names across
/// dirs keep the first occurrence (first-wins).
async fn scan_and_start(
    dirs: &[PathBuf],
) -> Result<Vec<Arc<RemoteExtensionProxy>>, LoaderError> {
    let mut plugins: Vec<Arc<RemoteExtensionProxy>> = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for dir in dirs {
        scan_one_dir(dir, &mut plugins, &mut seen_names).await?;
    }

    Ok(plugins)
}

async fn scan_one_dir(
    dir: &Path,
    plugins: &mut Vec<Arc<RemoteExtensionProxy>>,
    seen_names: &mut std::collections::HashSet<String>,
) -> Result<(), LoaderError> {
    if !dir.exists() {
        tracing::debug!(dir = %dir.display(), "extensions dir does not exist, skipping scan");
        return Ok(());
    }

    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(LoaderError::Io)?;

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
        if !file_type.is_dir() {
            continue;
        }

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

        match RemoteExtensionProxy::start(plugin_dir.clone(), manifest).await {
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
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else { continue };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let plugin_dir = entry.path();
            if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let manifest_path = plugin_dir.join("alva.toml");
            if !manifest_path.exists() {
                continue;
            }
            let Ok(s) = tokio::fs::read_to_string(&manifest_path).await else { continue };
            let Ok(manifest) = toml::from_str::<PluginManifest>(&s) else {
                tracing::warn!(
                    path = %manifest_path.display(),
                    "failed to parse manifest"
                );
                continue;
            };
            out.push(DiscoveredPlugin { dir: plugin_dir, manifest });
        }
    }
    out
}

/// Start a single plugin from its directory (containing `alva.toml`
/// + entry file). Used by CLI tools like `alva plugins exec` that
/// need one plugin running without the full Extension lifecycle.
///
/// Returns the running [`RemoteExtensionProxy`] — caller is
/// responsible for `.shutdown()` when done.
pub async fn start_plugin(plugin_dir: PathBuf) -> Result<Arc<RemoteExtensionProxy>, LoaderError> {
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
    let proxy = RemoteExtensionProxy::start(plugin_dir, manifest).await?;
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
    //! 1. **Reverse-mapping wire ⇄ core**: this helper is the INVERSE
    //!    of `proxy.rs::core_event_to_aep_name` (forward direction
    //!    pinned in proxy.rs tests). Two non-obvious asymmetries
    //!    distinguish it from a naïve identity / strip-prefix:
    //!    - `on_agent_*` → `agent_*` (drop `on_` prefix; tool/* are
    //!      symmetric with no prefix to strip)
    //!    - `on_user_message` → `"input"` (NOT `"user_message"`; core
    //!      event_type() for Input is literally "input"; a naïve
    //!      `strip_prefix("on_")` would silently break Input routing)
    //!
    //!    Forward-compat: unknown names return None (NOT a fallback),
    //!    so future-protocol plugins don't crash dispatch on names
    //!    the host doesn't know.
    //!
    //!    One parametric test pins all 5 known mappings + 3 unknown
    //!    cases in one pass. Round-trip composition with
    //!    `core_event_to_aep_name` can't be expressed here because
    //!    that function is private to proxy.rs; the two halves stay
    //!    in lockstep via the two crates' separate tests.
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
        // Table-driven over 5 known mappings + 3 unknown/edge cases.
        // The "drops on_ prefix for agent_*" + "on_user_message →
        // input (NOT user_message)" asymmetries are the load-bearing
        // pins documented in the mod docstring; each row has a label
        // so panic output names the broken contract.
        let cases: &[(&str, Option<&str>, &str)] = &[
            // ── symmetric (no on_ prefix to strip)
            ("before_tool_call", Some("before_tool_call"), "symmetric tool call"),
            ("after_tool_call", Some("after_tool_call"), "symmetric tool call"),
            // ── drops on_ prefix
            ("on_agent_start", Some("agent_start"), "on_ prefix dropped"),
            ("on_agent_end", Some("agent_end"), "on_ prefix dropped"),
            // ── CRITICAL ASYMMETRY: NOT a naïve strip_prefix("on_")
            ("on_user_message", Some("input"), "on_user_message → input (NOT user_message)"),
            // ── forward-compat: unknown names return None
            ("on_llm_call_start", None, "future-protocol name → None"),
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
