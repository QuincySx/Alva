// INPUT:  crate::types, crate::error, serde, std::collections, std::path (non-wasm), tokio::fs (non-wasm)
// OUTPUT: McpConfigFile (from_str + load/save on non-wasm), McpServerEntry, McpTransportEntry
// POS:    MCP Server config management — JSON parsing (all platforms) + file I/O (non-wasm only, cfg-gated).
//! MCP Server configuration reader/writer.
//!
//! `from_str()` works everywhere. `load()`/`save()` are cfg-gated for non-wasm platforms.

use crate::error::McpError;
use crate::types::{McpServerConfig, McpTransportConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(not(target_family = "wasm"))]
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
    /// Parse config from a JSON string. Works on all platforms.
    pub fn from_str(json: &str) -> Result<Self, McpError> {
        serde_json::from_str(json)
            .map_err(|e| McpError::Serialization(format!("Invalid config JSON: {e}")))
    }

    /// Load config from a specific path. Returns empty config if file doesn't exist.
    #[cfg(not(target_family = "wasm"))]
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
    #[cfg(not(target_family = "wasm"))]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> &'static str {
        r#"{
            "servers": {
                "browser": {
                    "display_name": "Browser",
                    "transport": {
                        "type": "stdio",
                        "command": "npx",
                        "args": ["-y", "@mcp/browser"],
                        "env": { "NODE_ENV": "production" }
                    },
                    "auto_connect": true,
                    "connect_timeout_secs": 60
                },
                "remote-api": {
                    "display_name": "Remote API",
                    "transport": {
                        "type": "sse",
                        "url": "https://example.com/sse",
                        "headers": { "Authorization": "Bearer tok123" }
                    },
                    "auto_connect": false,
                    "connect_timeout_secs": 15
                }
            }
        }"#
    }

    // ── from_str ────────────────────────────────────────────────────────

    #[test]
    fn from_str_parses_valid_json() {
        let cfg = McpConfigFile::from_str(sample_json()).unwrap();
        assert_eq!(cfg.servers.len(), 2);
        assert!(cfg.servers.contains_key("browser"));
        assert!(cfg.servers.contains_key("remote-api"));
    }

    #[test]
    fn from_str_empty_servers() {
        let cfg = McpConfigFile::from_str(r#"{"servers": {}}"#).unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn from_str_invalid_json_returns_error() {
        let err = McpConfigFile::from_str("not json").unwrap_err();
        match err {
            McpError::Serialization(msg) => assert!(msg.contains("Invalid config JSON")),
            other => panic!("expected Serialization, got {:?}", other),
        }
    }

    // ── defaults ────────────────────────────────────────────────────────

    #[test]
    fn defaults_applied_when_fields_missing() {
        let json = r#"{
            "servers": {
                "minimal": {
                    "display_name": "Minimal",
                    "transport": { "type": "stdio", "command": "echo" }
                }
            }
        }"#;
        let cfg = McpConfigFile::from_str(json).unwrap();
        let entry = &cfg.servers["minimal"];
        assert!(entry.auto_connect, "auto_connect should default to true");
        assert_eq!(
            entry.connect_timeout_secs, 30,
            "connect_timeout_secs should default to 30"
        );
    }

    // ── to_server_configs ───────────────────────────────────────────────

    #[test]
    fn to_server_configs_converts_all_entries() {
        let cfg = McpConfigFile::from_str(sample_json()).unwrap();
        let configs = cfg.to_server_configs();
        assert_eq!(configs.len(), 2);

        let browser = configs.iter().find(|c| c.id == "browser").unwrap();
        assert_eq!(browser.display_name, "Browser");
        assert!(browser.auto_connect);
        assert_eq!(browser.connect_timeout_secs, 60);

        match &browser.transport {
            McpTransportConfig::Stdio {
                command, args, env, ..
            } => {
                assert_eq!(command, "npx");
                assert_eq!(args, &vec!["-y".to_string(), "@mcp/browser".to_string()]);
                assert_eq!(env.get("NODE_ENV").unwrap(), "production");
            }
            _ => panic!("expected Stdio transport"),
        }

        let remote = configs.iter().find(|c| c.id == "remote-api").unwrap();
        assert_eq!(remote.display_name, "Remote API");
        assert!(!remote.auto_connect);
        match &remote.transport {
            McpTransportConfig::Sse { url, headers } => {
                assert_eq!(url, "https://example.com/sse");
                assert_eq!(headers.get("Authorization").unwrap(), "Bearer tok123");
            }
            _ => panic!("expected Sse transport"),
        }
    }

    // ── upsert / remove ─────────────────────────────────────────────────

    #[test]
    fn upsert_adds_new_entry() {
        let mut cfg = McpConfigFile::default();
        assert!(cfg.servers.is_empty());

        cfg.upsert(
            "new-server".to_string(),
            McpServerEntry {
                display_name: "New".into(),
                transport: McpTransportEntry::Stdio {
                    command: "cmd".into(),
                    args: vec![],
                    env: HashMap::new(),
                },
                auto_connect: false,
                connect_timeout_secs: 10,
            },
        );

        assert_eq!(cfg.servers.len(), 1);
        assert_eq!(cfg.servers["new-server"].display_name, "New");
    }

    #[test]
    fn upsert_overwrites_existing_entry() {
        let mut cfg = McpConfigFile::from_str(sample_json()).unwrap();
        let original_len = cfg.servers.len();

        cfg.upsert(
            "browser".to_string(),
            McpServerEntry {
                display_name: "Updated Browser".into(),
                transport: McpTransportEntry::Stdio {
                    command: "node".into(),
                    args: vec![],
                    env: HashMap::new(),
                },
                auto_connect: false,
                connect_timeout_secs: 5,
            },
        );

        assert_eq!(cfg.servers.len(), original_len);
        assert_eq!(cfg.servers["browser"].display_name, "Updated Browser");
    }

    #[test]
    fn remove_returns_entry_and_shrinks_map() {
        let mut cfg = McpConfigFile::from_str(sample_json()).unwrap();
        assert_eq!(cfg.servers.len(), 2);

        let removed = cfg.remove("browser");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().display_name, "Browser");
        assert_eq!(cfg.servers.len(), 1);
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let mut cfg = McpConfigFile::default();
        assert!(cfg.remove("nope").is_none());
    }

    // ── load / save (filesystem, non-wasm) ──────────────────────────────

    #[cfg(not(target_family = "wasm"))]
    #[tokio::test]
    async fn load_returns_default_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let cfg = McpConfigFile::load(&path).await.unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[cfg(not(target_family = "wasm"))]
    #[tokio::test]
    async fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("config.json");

        let mut cfg = McpConfigFile::default();
        cfg.upsert(
            "test-server".into(),
            McpServerEntry {
                display_name: "Test".into(),
                transport: McpTransportEntry::Sse {
                    url: "http://localhost:8080/sse".into(),
                    headers: HashMap::new(),
                },
                auto_connect: true,
                connect_timeout_secs: 30,
            },
        );

        cfg.save(&path).await.unwrap();
        assert!(path.exists());

        let loaded = McpConfigFile::load(&path).await.unwrap();
        assert_eq!(loaded.servers.len(), 1);
        assert_eq!(loaded.servers["test-server"].display_name, "Test");
    }

    #[cfg(not(target_family = "wasm"))]
    #[tokio::test]
    async fn load_invalid_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        tokio::fs::write(&path, "not json").await.unwrap();

        let err = McpConfigFile::load(&path).await.unwrap_err();
        match err {
            McpError::Serialization(msg) => assert!(msg.contains("Invalid")),
            other => panic!("expected Serialization, got {:?}", other),
        }
    }

    // ── McpServerEntry::to_server_config ────────────────────────────────

    #[test]
    fn entry_to_server_config_stdio() {
        let entry = McpServerEntry {
            display_name: "My Server".into(),
            transport: McpTransportEntry::Stdio {
                command: "python".into(),
                args: vec!["server.py".into()],
                env: HashMap::from([("KEY".into(), "VAL".into())]),
            },
            auto_connect: true,
            connect_timeout_secs: 45,
        };

        let config = entry.to_server_config("my-id");
        assert_eq!(config.id, "my-id");
        assert_eq!(config.display_name, "My Server");
        assert!(config.auto_connect);
        assert_eq!(config.connect_timeout_secs, 45);

        match &config.transport {
            McpTransportConfig::Stdio { command, args, env } => {
                assert_eq!(command, "python");
                assert_eq!(args, &vec!["server.py".to_string()]);
                assert_eq!(env["KEY"], "VAL");
            }
            _ => panic!("expected Stdio"),
        }
    }

    #[test]
    fn entry_to_server_config_sse() {
        let entry = McpServerEntry {
            display_name: "SSE Server".into(),
            transport: McpTransportEntry::Sse {
                url: "https://api.example.com/sse".into(),
                headers: HashMap::from([("X-Key".into(), "secret".into())]),
            },
            auto_connect: false,
            connect_timeout_secs: 10,
        };

        let config = entry.to_server_config("sse-id");
        assert_eq!(config.id, "sse-id");
        assert!(!config.auto_connect);

        match &config.transport {
            McpTransportConfig::Sse { url, headers } => {
                assert_eq!(url, "https://api.example.com/sse");
                assert_eq!(headers["X-Key"], "secret");
            }
            _ => panic!("expected Sse"),
        }
    }

    // ── serde roundtrip for transport entry ─────────────────────────────

    #[test]
    fn transport_entry_serde_roundtrip_stdio() {
        let entry = McpTransportEntry::Stdio {
            command: "node".into(),
            args: vec!["index.js".into()],
            env: HashMap::new(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: McpTransportEntry = serde_json::from_str(&json).unwrap();
        match parsed {
            McpTransportEntry::Stdio { command, .. } => assert_eq!(command, "node"),
            _ => panic!("expected Stdio"),
        }
    }

    #[test]
    fn transport_entry_serde_roundtrip_sse() {
        let entry = McpTransportEntry::Sse {
            url: "http://localhost/sse".into(),
            headers: HashMap::from([("Auth".into(), "tok".into())]),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: McpTransportEntry = serde_json::from_str(&json).unwrap();
        match parsed {
            McpTransportEntry::Sse { url, headers } => {
                assert_eq!(url, "http://localhost/sse");
                assert_eq!(headers["Auth"], "tok");
            }
            _ => panic!("expected Sse"),
        }
    }
}
