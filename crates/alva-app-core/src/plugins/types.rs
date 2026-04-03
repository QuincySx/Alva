// INPUT:  serde, std::collections::HashMap, std::path::PathBuf
// OUTPUT: PluginScope, PluginStatus, PluginDefinition, PluginInstallRequest, PluginSource, PluginEvent
// POS:    Plugin system type definitions — scope, status, manifest, installation, and lifecycle events.

//! Plugin system types — scope, status, definition, installation and lifecycle events.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Plugin scope determines where the plugin is installed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginScope {
    /// User-level (~/.claude/plugins/)
    User,
    /// Project-level (.claude/plugins/)
    Project,
    /// Local (git-ignored)
    Local,
    /// Managed (by organization policy)
    Managed,
}

/// Plugin status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    Enabled,
    Disabled,
    Installing,
    Error,
    Uninstalled,
}

/// Plugin definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDefinition {
    /// Unique plugin identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Plugin description
    pub description: String,
    /// Plugin version (semver)
    pub version: String,
    /// Plugin author
    pub author: Option<String>,
    /// Installation scope
    pub scope: PluginScope,
    /// Current status
    pub status: PluginStatus,
    /// Plugin entry point (directory path)
    pub path: PathBuf,
    /// Configuration values
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
    /// Commands provided by this plugin
    #[serde(default)]
    pub commands: Vec<String>,
    /// Tools provided by this plugin
    #[serde(default)]
    pub tools: Vec<String>,
    /// Skills provided by this plugin
    #[serde(default)]
    pub skills: Vec<String>,
}

/// Plugin installation request
#[derive(Debug, Clone)]
pub struct PluginInstallRequest {
    pub source: PluginSource,
    pub scope: PluginScope,
}

/// Plugin source for installation
#[derive(Debug, Clone)]
pub enum PluginSource {
    /// Install from directory path
    Directory(PathBuf),
    /// Install from npm package
    Npm(String),
    /// Install from git repository
    Git(String),
    /// Install from marketplace
    Marketplace(String),
}

/// Plugin event for lifecycle tracking
#[derive(Debug, Clone)]
pub enum PluginEvent {
    Installed(String),
    Uninstalled(String),
    Enabled(String),
    Disabled(String),
    Updated(String, String), // id, new_version
    Error(String, String),   // id, error_message
}
