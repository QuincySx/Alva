// INPUT:  serde, std::fs, dirs (home_dir)
// OUTPUT: McpServerInfo + load_mcp_servers helper
// POS:    MVP MCP config reader. Reads `~/.alva/mcp.json` if it exists and
//         returns the declared servers for display. Writing / live-connect is
//         a next-batch concern.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
pub struct McpServerInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub command_or_url: String,
    pub enabled: bool,
}

#[derive(Deserialize, Default)]
struct McpConfigFile {
    #[serde(default)]
    servers: Vec<McpServerEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct McpServerEntry {
    #[serde(default)]
    id: Option<String>,
    name: String,
    /// Either "stdio" or "http"/"sse". Falls back to "stdio".
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".alva").join("mcp.json"))
}

pub fn load_mcp_servers() -> Vec<McpServerInfo> {
    let Some(path) = config_path() else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    let Ok(cfg) = serde_json::from_slice::<McpConfigFile>(&bytes) else {
        tracing::warn!(path = %path.display(), "failed to parse mcp.json");
        return Vec::new();
    };
    cfg.servers
        .into_iter()
        .map(|e| {
            let kind = e.kind.unwrap_or_else(|| "stdio".into());
            let command_or_url = e
                .command
                .or(e.url)
                .unwrap_or_else(|| "(unset)".into());
            McpServerInfo {
                id: e.id.unwrap_or_else(|| e.name.clone()),
                name: e.name,
                kind,
                command_or_url,
                enabled: e.enabled,
            }
        })
        .collect()
}
