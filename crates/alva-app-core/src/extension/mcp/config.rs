// INPUT:  crate::extension::skills::skill_domain::mcp, crate::error, serde, std::collections, std::path, tokio::fs
// OUTPUT: McpConfig, McpServerEntry, McpTransportEntry
// POS:    Manages mcpServerConfig.json — the user-facing MCP Server configuration file.
//! MCP config reader/writer with Alva + ecosystem schema support.
//!
//! Reads and writes MCP server configuration files. Callers supply the path.
//!
//! Supported shapes:
//!
//! **Alva** (`servers` map):
//! ```json
//! {
//!   "servers": {
//!     "id": {
//!       "display_name": "Name",
//!       "transport": { "type": "stdio", "command": "npx", "args": [] }
//!     }
//!   }
//! }
//! ```
//!
//! **Ecosystem** (`mcpServers` map, Claude / Cursor / Open Plugins):
//! ```json
//! {
//!   "mcpServers": {
//!     "id": { "command": "npx", "args": ["-y", "pkg"] }
//!   }
//! }
//! ```

use crate::error::SkillError;
use crate::extension::skills::skill_domain::mcp::McpServerConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Top-level structure of mcpServerConfig.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    /// server_id -> McpServerEntry
    pub servers: HashMap<String, McpServerEntry>,
}

/// A single MCP server entry in the config file.
/// Closely mirrors McpServerConfig but serialized in the user-facing JSON format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    #[serde(default)]
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

/// Transport configuration in the JSON config file
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

impl McpConfig {
    /// Load config from a specific path. Returns empty config if file doesn't exist.
    pub async fn load(path: &Path) -> Result<Self, SkillError> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| SkillError::Io(format!("Failed to read {}: {}", path.display(), e)))?;

        Self::parse_str(&content).map_err(|e| {
            SkillError::Serialization(format!("Invalid MCP config {}: {e}", path.display()))
        })
    }

    /// Parse Alva `servers` and/or ecosystem `mcpServers` JSON.
    pub fn parse_str(content: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(content).map_err(|e| format!("JSON parse error: {e}"))?;
        Self::from_value(&value)
    }

    fn from_value(value: &Value) -> Result<Self, String> {
        let mut servers = HashMap::new();

        // Alva shape: { "servers": { id: McpServerEntry | loose } }
        if let Some(map) = value.get("servers").and_then(|v| v.as_object()) {
            for (id, entry_val) in map {
                if let Some(entry) = parse_server_entry(id, entry_val)? {
                    servers.insert(id.clone(), entry);
                }
            }
        }

        // Ecosystem shape: { "mcpServers": { id: { command, args } | { url } } }
        if let Some(map) = value.get("mcpServers").and_then(|v| v.as_object()) {
            for (id, entry_val) in map {
                if let Some(entry) = parse_server_entry(id, entry_val)? {
                    servers.insert(id.clone(), entry);
                }
            }
        }

        // Bare object with only server entries is not supported — require a known key.
        Ok(Self { servers })
    }

    /// Load and merge MCP configs from global + project paths.
    ///
    /// Project servers override global servers with the same ID.
    pub async fn load_merged(paths: &crate::paths::AlvaPaths) -> Result<Self, SkillError> {
        Self::load_merged_paths(&paths.mcp_discovery_paths()).await
    }

    /// Load and merge configs from an explicit path list (later overrides earlier).
    pub async fn load_merged_paths(paths: &[std::path::PathBuf]) -> Result<Self, SkillError> {
        let mut merged = Self::default();
        for path in paths {
            let cfg = Self::load(path).await?;
            for (id, entry) in cfg.servers {
                merged.servers.insert(id, entry);
            }
        }
        Ok(merged)
    }

    /// Save config to a specific path (creates parent directories if needed).
    /// Always writes the Alva `servers` shape.
    pub async fn save(&self, path: &Path) -> Result<(), SkillError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| SkillError::Io(format!("Failed to create directory: {e}")))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| SkillError::Serialization(e.to_string()))?;

        tokio::fs::write(path, content)
            .await
            .map_err(|e| SkillError::Io(format!("Failed to write {}: {}", path.display(), e)))?;

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

/// Parse one server entry from either Alva tagged transport or ecosystem flat fields.
fn parse_server_entry(id: &str, value: &Value) -> Result<Option<McpServerEntry>, String> {
    if value.is_null() {
        return Ok(None);
    }

    // Prefer full Alva shape when `transport` is present.
    if value.get("transport").is_some() {
        let mut entry: McpServerEntry =
            serde_json::from_value(value.clone()).map_err(|e| format!("server `{id}`: {e}"))?;
        if entry.display_name.is_empty() {
            entry.display_name = id.to_string();
        }
        return Ok(Some(entry));
    }

    // Ecosystem / loose: command+args or url
    let obj = value
        .as_object()
        .ok_or_else(|| format!("server `{id}`: expected object"))?;

    let display_name = obj
        .get("display_name")
        .or_else(|| obj.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(id)
        .to_string();

    let auto_connect = obj
        .get("auto_connect")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let connect_timeout_secs = obj
        .get("connect_timeout_secs")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or_else(default_timeout);

    let transport = if let Some(command) = obj.get("command").and_then(|v| v.as_str()) {
        let args = obj
            .get("args")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let env = obj
            .get("env")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        McpTransportEntry::Stdio {
            command: command.to_string(),
            args,
            env,
        }
    } else if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
        let headers = obj
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        McpTransportEntry::Sse {
            url: url.to_string(),
            headers,
        }
    } else {
        // type: "sse" with url already handled; type stdio without command is invalid
        return Err(format!(
            "server `{id}`: need `transport`, `command`, or `url`"
        ));
    };

    Ok(Some(McpServerEntry {
        display_name,
        transport,
        auto_connect,
        connect_timeout_secs,
    }))
}

impl McpServerEntry {
    /// Convert to the domain-level McpServerConfig
    pub fn to_server_config(&self, id: &str) -> McpServerConfig {
        use crate::extension::skills::skill_domain::mcp::McpTransportConfig;

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

        let display_name = if self.display_name.is_empty() {
            id.to_string()
        } else {
            self.display_name.clone()
        };

        McpServerConfig {
            id: id.to_string(),
            display_name,
            transport,
            auto_connect: self.auto_connect,
            connect_timeout_secs: self.connect_timeout_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_alva_servers_shape() {
        let cfg = McpConfig::parse_str(
            r#"{
              "servers": {
                "local": {
                  "display_name": "Local",
                  "transport": {
                    "type": "stdio",
                    "command": "npx",
                    "args": ["-y", "foo"]
                  }
                }
              }
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.servers.len(), 1);
        let e = cfg.servers.get("local").unwrap();
        assert_eq!(e.display_name, "Local");
        match &e.transport {
            McpTransportEntry::Stdio { command, args, .. } => {
                assert_eq!(command, "npx");
                assert_eq!(args, &vec!["-y".to_string(), "foo".to_string()]);
            }
            _ => panic!("expected stdio"),
        }
    }

    #[test]
    fn parse_mcp_servers_ecosystem_shape() {
        let cfg = McpConfig::parse_str(
            r#"{
              "mcpServers": {
                "context7": {
                  "command": "npx",
                  "args": ["-y", "@upstash/context7-mcp"]
                }
              }
            }"#,
        )
        .unwrap();
        let e = cfg.servers.get("context7").unwrap();
        assert_eq!(e.display_name, "context7");
        match &e.transport {
            McpTransportEntry::Stdio { command, args, .. } => {
                assert_eq!(command, "npx");
                assert!(args.iter().any(|a| a.contains("context7")));
            }
            _ => panic!("expected stdio"),
        }
    }

    #[test]
    fn parse_mcp_servers_url() {
        let cfg = McpConfig::parse_str(
            r#"{ "mcpServers": { "remote": { "url": "https://example.com/sse" } } }"#,
        )
        .unwrap();
        match &cfg.servers["remote"].transport {
            McpTransportEntry::Sse { url, .. } => assert!(url.contains("example.com")),
            _ => panic!("expected sse"),
        }
    }

    #[test]
    fn ecosystem_overrides_when_merged_manually() {
        let mut a =
            McpConfig::parse_str(r#"{ "mcpServers": { "x": { "command": "a" } } }"#).unwrap();
        let b = McpConfig::parse_str(
            r#"{ "servers": {
              "x": {
                "display_name": "X",
                "transport": { "type": "stdio", "command": "b" }
              }
            } }"#,
        )
        .unwrap();
        for (id, e) in b.servers {
            a.servers.insert(id, e);
        }
        match &a.servers["x"].transport {
            McpTransportEntry::Stdio { command, .. } => assert_eq!(command, "b"),
            _ => panic!(),
        }
    }
}
