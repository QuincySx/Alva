// INPUT:  super::types, std::collections::HashMap, std::path, tokio, serde_json
// OUTPUT: PluginManager
// POS:    Plugin manager — discovers, installs, enables/disables, and tracks plugin lifecycle.

//! Plugin manager — discovers, installs, enables/disables, and tracks plugin lifecycle.

use super::types::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages plugin lifecycle
pub struct PluginManager {
    /// All known plugins by ID
    plugins: Arc<RwLock<HashMap<String, PluginDefinition>>>,
    /// Plugin search paths
    search_paths: Vec<PathBuf>,
    /// Event listeners
    event_handlers: Vec<Box<dyn Fn(&PluginEvent) + Send + Sync>>,
}

impl PluginManager {
    pub fn new(home_dir: &Path, workspace: &Path) -> Self {
        let search_paths = vec![
            home_dir.join(".claude").join("plugins"),
            workspace.join(".claude").join("plugins"),
        ];

        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            search_paths,
            event_handlers: Vec::new(),
        }
    }

    /// Discover plugins from search paths
    pub async fn discover(&self) -> Vec<PluginDefinition> {
        let mut found = Vec::new();

        for search_path in &self.search_paths {
            if !search_path.exists() {
                continue;
            }

            if let Ok(mut entries) = tokio::fs::read_dir(search_path).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Some(plugin) = self.load_plugin_manifest(&path).await {
                            found.push(plugin);
                        }
                    }
                }
            }
        }

        found
    }

    /// Load plugin manifest from directory
    async fn load_plugin_manifest(&self, path: &Path) -> Option<PluginDefinition> {
        let manifest_path = path.join("plugin.json");
        let content = tokio::fs::read_to_string(&manifest_path).await.ok()?;
        let mut plugin: PluginDefinition = serde_json::from_str(&content).ok()?;
        plugin.path = path.to_path_buf();
        Some(plugin)
    }

    /// Install a plugin
    pub async fn install(
        &self,
        request: PluginInstallRequest,
    ) -> Result<PluginDefinition, String> {
        let plugin = match &request.source {
            PluginSource::Directory(path) => self
                .load_plugin_manifest(path)
                .await
                .ok_or_else(|| format!("No plugin.json found in {}", path.display()))?,
            PluginSource::Npm(package) => {
                return Err(format!(
                    "npm plugin installation not yet implemented: {}",
                    package
                ));
            }
            PluginSource::Git(url) => {
                return Err(format!(
                    "git plugin installation not yet implemented: {}",
                    url
                ));
            }
            PluginSource::Marketplace(id) => {
                return Err(format!(
                    "marketplace plugin installation not yet implemented: {}",
                    id
                ));
            }
        };

        let id = plugin.id.clone();
        self.plugins.write().await.insert(id.clone(), plugin.clone());
        self.emit_event(PluginEvent::Installed(id));
        Ok(plugin)
    }

    /// Uninstall a plugin
    pub async fn uninstall(&self, plugin_id: &str) -> Result<(), String> {
        let mut plugins = self.plugins.write().await;
        if plugins.remove(plugin_id).is_some() {
            self.emit_event(PluginEvent::Uninstalled(plugin_id.to_string()));
            Ok(())
        } else {
            Err(format!("Plugin not found: {}", plugin_id))
        }
    }

    /// Enable a plugin
    pub async fn enable(&self, plugin_id: &str) -> Result<(), String> {
        let mut plugins = self.plugins.write().await;
        if let Some(plugin) = plugins.get_mut(plugin_id) {
            plugin.status = PluginStatus::Enabled;
            self.emit_event(PluginEvent::Enabled(plugin_id.to_string()));
            Ok(())
        } else {
            Err(format!("Plugin not found: {}", plugin_id))
        }
    }

    /// Disable a plugin
    pub async fn disable(&self, plugin_id: &str) -> Result<(), String> {
        let mut plugins = self.plugins.write().await;
        if let Some(plugin) = plugins.get_mut(plugin_id) {
            plugin.status = PluginStatus::Disabled;
            self.emit_event(PluginEvent::Disabled(plugin_id.to_string()));
            Ok(())
        } else {
            Err(format!("Plugin not found: {}", plugin_id))
        }
    }

    /// List all plugins
    pub async fn list(&self) -> Vec<PluginDefinition> {
        self.plugins.read().await.values().cloned().collect()
    }

    /// List enabled plugins
    pub async fn list_enabled(&self) -> Vec<PluginDefinition> {
        self.plugins
            .read()
            .await
            .values()
            .filter(|p| p.status == PluginStatus::Enabled)
            .cloned()
            .collect()
    }

    /// Get a specific plugin
    pub async fn get(&self, plugin_id: &str) -> Option<PluginDefinition> {
        self.plugins.read().await.get(plugin_id).cloned()
    }

    fn emit_event(&self, event: PluginEvent) {
        for handler in &self.event_handlers {
            handler(&event);
        }
    }

    /// Register an event handler
    pub fn on_event(&mut self, handler: Box<dyn Fn(&PluginEvent) + Send + Sync>) {
        self.event_handlers.push(handler);
    }
}
