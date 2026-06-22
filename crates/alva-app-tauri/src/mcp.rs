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
    parse_mcp_config(&bytes)
}

/// Map a raw mcp.json byte payload into the UI's display shape.
///
/// Extracted from `load_mcp_servers` so tests can exercise the
/// field-default + fallback logic (kind → "stdio", command_or_url
/// three-way fallback, id → name, etc.) without touching the
/// filesystem. Malformed JSON logs a warn + returns an empty Vec —
/// the UI then shows "no MCP servers configured" rather than
/// surfacing a JSON parse error.
fn parse_mcp_config(bytes: &[u8]) -> Vec<McpServerInfo> {
    let Ok(cfg) = serde_json::from_slice::<McpConfigFile>(bytes) else {
        tracing::warn!("failed to parse mcp.json (malformed JSON)");
        return Vec::new();
    };
    cfg.servers
        .into_iter()
        .map(|e| {
            let kind = e.kind.unwrap_or_else(|| "stdio".into());
            let command_or_url = e.command.or(e.url).unwrap_or_else(|| "(unset)".into());
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

#[cfg(test)]
mod tests {
    //! Tests for `parse_mcp_config` — the pure mapping layer underneath
    //! `load_mcp_servers`. Each test pins one of the implicit defaults /
    //! fallbacks the UI relies on; changing any of these silently would
    //! show users a wrong picture of their configured MCP servers.
    use super::*;

    #[test]
    fn parse_empty_servers_returns_empty_vec() {
        let out = parse_mcp_config(br#"{ "servers": [] }"#);
        assert!(out.is_empty());
    }

    #[test]
    fn parse_missing_servers_field_returns_empty_vec() {
        // serde(default) on `servers` lets us accept a config with no
        // `servers` key at all (e.g. a future field-only addition).
        let out = parse_mcp_config(br#"{}"#);
        assert!(out.is_empty());
    }

    #[test]
    fn parse_malformed_json_returns_empty_vec_not_panic() {
        let out = parse_mcp_config(b"not json at all");
        assert!(out.is_empty());
    }

    #[test]
    fn parse_stdio_default_kind_and_id_falls_back_to_name() {
        // Minimal entry — only name + command. Kind missing → "stdio",
        // id missing → name, enabled missing → true.
        let out = parse_mcp_config(
            br#"{ "servers": [{ "name": "fs", "command": "/usr/local/bin/mcp-fs" }] }"#,
        );
        assert_eq!(out.len(), 1);
        let s = &out[0];
        assert_eq!(s.name, "fs");
        assert_eq!(s.id, "fs", "id must fall back to name when omitted");
        assert_eq!(s.kind, "stdio", "kind must default to 'stdio'");
        assert_eq!(s.command_or_url, "/usr/local/bin/mcp-fs");
        assert!(s.enabled, "enabled must default to true");
    }

    #[test]
    fn parse_url_only_routes_to_command_or_url() {
        // command absent + url present → command_or_url == url. UI
        // shows one string regardless of stdio/http kind.
        let out = parse_mcp_config(
            br#"{ "servers": [{ "name": "github", "kind": "http", "url": "https://api.example/mcp" }] }"#,
        );
        assert_eq!(out.len(), 1);
        let s = &out[0];
        assert_eq!(s.kind, "http");
        assert_eq!(s.command_or_url, "https://api.example/mcp");
    }

    #[test]
    fn parse_command_wins_over_url_when_both_present() {
        // Pinned precedence: `command` takes priority. Without this
        // ordering, an entry with both fields would silently render
        // its URL even when stdio is intended.
        let out = parse_mcp_config(
            br#"{ "servers": [{
                "name": "both",
                "command": "the-command",
                "url": "https://shadow.example"
            }] }"#,
        );
        assert_eq!(out[0].command_or_url, "the-command");
    }

    #[test]
    fn parse_neither_command_nor_url_yields_unset_marker() {
        // Both absent → "(unset)" placeholder, NOT empty string. The
        // UI uses this exact marker as a visual signal that the user
        // has a broken/incomplete entry in their config.
        let out = parse_mcp_config(br#"{ "servers": [{ "name": "broken" }] }"#);
        assert_eq!(out[0].command_or_url, "(unset)");
    }

    #[test]
    fn parse_explicit_id_overrides_name_fallback() {
        let out = parse_mcp_config(
            br#"{ "servers": [{ "id": "custom-id", "name": "display-name", "command": "x" }] }"#,
        );
        let s = &out[0];
        assert_eq!(s.id, "custom-id");
        assert_eq!(s.name, "display-name");
    }

    #[test]
    fn parse_explicit_enabled_false_overrides_default() {
        let out = parse_mcp_config(
            br#"{ "servers": [{ "name": "off", "command": "x", "enabled": false }] }"#,
        );
        assert!(!out[0].enabled);
    }

    #[test]
    fn parse_aggregates_multiple_servers_preserving_order() {
        // List with two distinct entries — verify both round-trip with
        // independent defaults and that order is preserved (UI lists in
        // declared order).
        let out = parse_mcp_config(
            br#"{ "servers": [
                { "name": "first", "command": "a" },
                { "name": "second", "kind": "sse", "url": "https://b" }
            ] }"#,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "first");
        assert_eq!(out[0].kind, "stdio");
        assert_eq!(out[1].name, "second");
        assert_eq!(out[1].kind, "sse");
    }
}
