// INPUT:  crate::types, crate::error, serde, std::collections, std::path, tokio::fs
// OUTPUT: McpConfigFile, McpServerEntry, McpTransportEntry
// POS:    Manages mcpServerConfig.json — the user-facing MCP Server configuration file.
//! mcpServerConfig.json reader/writer
//!
//! Reads and writes MCP server configuration files. Callers supply the path.

use crate::error::McpError;
use crate::types::{McpServerConfig, McpTransportConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Top-level structure of mcpServerConfig.json.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfigFile {
    /// server_id -> McpServerEntry
    pub servers: HashMap<String, McpServerEntry>,
}

/// A single MCP server entry in the config file.
/// Closely mirrors McpServerConfig but serialized in the user-facing JSON format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub display_name: String,
    pub transport: McpTransportEntry,
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    #[serde(default = "default_timeout")]
    pub connect_timeout_secs: u32,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u32 {
    30
}

/// Transport configuration in the JSON config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpTransportEntry {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Sse {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

impl McpConfigFile {
    /// Load config from a specific path. Returns empty config if file doesn't exist.
    pub async fn load(path: &Path) -> Result<Self, McpError> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| McpError::Io(format!("Failed to read {}: {}", path.display(), e)))?;

        let config: McpConfigFile = serde_json::from_str(&content)
            .map_err(|e| McpError::Serialization(format!("Invalid mcpServerConfig.json: {}", e)))?;

        Ok(config)
    }

    /// Save config to a specific path (creates parent directories if needed).
    pub async fn save(&self, path: &Path) -> Result<(), McpError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| McpError::Io(format!("Failed to create directory: {e}")))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| McpError::Serialization(e.to_string()))?;

        tokio::fs::write(path, content)
            .await
            .map_err(|e| McpError::Io(format!("Failed to write {}: {}", path.display(), e)))?;

        Ok(())
    }

    /// Convert all entries to McpServerConfig domain objects.
    pub fn to_server_configs(&self) -> Vec<McpServerConfig> {
        self.servers
            .iter()
            .map(|(id, entry)| entry.to_server_config(id))
            .collect()
    }

    /// Add or update a server entry.
    pub fn upsert(&mut self, id: String, entry: McpServerEntry) {
        self.servers.insert(id, entry);
    }

    /// Remove a server entry.
    pub fn remove(&mut self, id: &str) -> Option<McpServerEntry> {
        self.servers.remove(id)
    }
}

impl McpServerEntry {
    /// Convert to the domain-level McpServerConfig.
    pub fn to_server_config(&self, id: &str) -> McpServerConfig {
        let transport = match &self.transport {
            McpTransportEntry::Stdio { command, args, env } => McpTransportConfig::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: env.clone(),
            },
            McpTransportEntry::Sse { url, headers } => McpTransportConfig::Sse {
                url: url.clone(),
                headers: headers.clone(),
            },
        };

        McpServerConfig {
            id: id.to_string(),
            display_name: self.display_name.clone(),
            transport,
            auto_connect: self.auto_connect,
            connect_timeout_secs: self.connect_timeout_secs,
        }
    }
}
