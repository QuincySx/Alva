// INPUT: Private SQLite, ALVA component catalog and explicit MCP server types
// OUTPUT: HubToolSettings, ToolSettingsAction and HubStore tool-settings methods
// POS: L7 ALVA capability preferences; safety substrate stays mandatory and other engines stay untouched

use super::HubStore;
use alva_engine_adapter_alva::{ComponentToggles, McpServerConfig, McpTransportConfig, COMPONENTS};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const TOGGLEABLE: &[&str] = &[
    "core",
    "shell",
    "web",
    "browser",
    "skills",
    "planning",
    "utility",
    "task",
    "team",
    "sub-agents",
];

#[derive(Debug, Clone, Serialize)]
pub struct ToolComponent {
    pub id: String,
    pub enabled: bool,
    pub default_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpServer {
    pub id: String,
    pub display_name: String,
    pub command: String,
    pub args: Vec<String>,
    pub connect_timeout_secs: u32,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HubToolSettings {
    pub revision: i64,
    pub components: Vec<ToolComponent>,
    pub mcp_servers: Vec<ManagedMcpServer>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    toggles: BTreeMap<String, bool>,
    servers: BTreeMap<String, ManagedMcpServer>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolSettingsAction {
    SetComponent {
        revision: i64,
        id: String,
        enabled: bool,
    },
    CreateMcp {
        revision: i64,
        server: ManagedMcpServer,
    },
    UpdateMcp {
        revision: i64,
        server: ManagedMcpServer,
    },
    SetMcpEnabled {
        revision: i64,
        id: String,
        enabled: bool,
        confirmed: bool,
    },
    DeleteMcp {
        revision: i64,
        id: String,
    },
}

impl ToolSettingsAction {
    pub fn starts_execution(&self) -> bool {
        matches!(self, Self::SetMcpEnabled { enabled: true, .. })
    }
    fn revision(&self) -> i64 {
        match self {
            Self::SetComponent { revision, .. }
            | Self::CreateMcp { revision, .. }
            | Self::UpdateMcp { revision, .. }
            | Self::SetMcpEnabled { revision, .. }
            | Self::DeleteMcp { revision, .. } => *revision,
        }
    }
}

fn validate_server(server: &ManagedMcpServer) -> Result<(), String> {
    if server.id.is_empty()
        || server.id.len() > 64
        || !server
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || server.id.starts_with('-')
        || server.id.ends_with('-')
    {
        return Err("MCP ID must be lowercase kebab-case, at most 64 bytes".into());
    }
    if server.display_name.trim().is_empty() || server.display_name.chars().count() > 120 {
        return Err("MCP name must contain 1–120 characters".into());
    }
    if server.command.len() > 4096
        || !Path::new(&server.command).is_absolute()
        || server.command.contains(['\0', '\n', '\r'])
    {
        return Err(
            "MCP requires an absolute executable path; shell command lines are not accepted".into(),
        );
    }
    if server.args.len() > 32
        || server
            .args
            .iter()
            .any(|arg| arg.len() > 2048 || arg.contains('\0'))
        || server.args.iter().map(String::len).sum::<usize>() > 16384
    {
        return Err("MCP arguments exceed supported limits".into());
    }
    if !(1..=60).contains(&server.connect_timeout_secs) {
        return Err("MCP timeout must be 1–60 seconds".into());
    }
    Ok(())
}

fn read(connection: &Connection) -> Result<(i64, Config), String> {
    let row = connection
        .query_row(
            "SELECT revision, config_json FROM tool_settings WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|_| "Tool settings unavailable")?;
    let Some((revision, json)) = row else {
        return Ok((0, Config::default()));
    };
    if json.len() > 1024 * 1024 {
        return Err("Tool settings exceed limits".into());
    }
    let config: Config = serde_json::from_str(&json).map_err(|_| "Invalid tool settings")?;
    if config
        .toggles
        .keys()
        .any(|id| !TOGGLEABLE.contains(&id.as_str()))
        || config.servers.len() > 32
    {
        return Err("Invalid tool settings".into());
    }
    for (id, server) in &config.servers {
        if id != &server.id {
            return Err("Invalid MCP identity".into());
        }
        validate_server(server)?;
    }
    Ok((revision, config))
}

fn view(revision: i64, config: Config) -> HubToolSettings {
    HubToolSettings {
        revision,
        components: COMPONENTS
            .iter()
            .filter(|m| TOGGLEABLE.contains(&m.id))
            .map(|m| ToolComponent {
                id: m.id.into(),
                enabled: *config.toggles.get(m.id).unwrap_or(&m.default_on),
                default_enabled: m.default_on,
            })
            .collect(),
        mcp_servers: config.servers.into_values().collect(),
    }
}

impl HubToolSettings {
    pub fn toggles(&self) -> ComponentToggles {
        self.components
            .iter()
            .map(|m| (m.id.clone(), m.enabled))
            .collect()
    }
    pub fn enabled_mcp_servers(&self) -> Vec<McpServerConfig> {
        self.mcp_servers
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.runtime_config())
            .collect()
    }
}

impl ManagedMcpServer {
    pub fn runtime_config(&self) -> McpServerConfig {
        McpServerConfig {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            transport: McpTransportConfig::Stdio {
                command: self.command.clone(),
                args: self.args.clone(),
                env: Default::default(),
            },
            auto_connect: true,
            connect_timeout_secs: self.connect_timeout_secs,
        }
    }
}

impl HubStore {
    pub fn tool_settings(&self) -> Result<HubToolSettings, String> {
        let connection = self.lock()?;
        let (revision, config) = read(&connection)?;
        Ok(view(revision, config))
    }
    pub fn tool_settings_action(
        &self,
        action: ToolSettingsAction,
    ) -> Result<HubToolSettings, String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .map_err(|_| "Tool settings transaction unavailable")?;
        let (revision, mut config) = read(&transaction)?;
        if revision != action.revision() {
            return Err("Tool settings changed; refresh and review again".into());
        }
        match action {
            ToolSettingsAction::SetComponent { id, enabled, .. } => {
                if !TOGGLEABLE.contains(&id.as_str()) {
                    return Err(
                        "This component is not configurable; safety controls cannot be disabled"
                            .into(),
                    );
                }
                config.toggles.insert(id, enabled);
            }
            ToolSettingsAction::CreateMcp { mut server, .. } => {
                validate_server(&server)?;
                if config.servers.contains_key(&server.id) {
                    return Err("MCP ID already exists".into());
                }
                if config.servers.len() >= 32 {
                    return Err("At most 32 MCP servers are supported".into());
                }
                server.enabled = false;
                config.servers.insert(server.id.clone(), server);
            }
            ToolSettingsAction::UpdateMcp { mut server, .. } => {
                validate_server(&server)?;
                if !config.servers.contains_key(&server.id) {
                    return Err("MCP server not found".into());
                }
                server.enabled = false;
                config.servers.insert(server.id.clone(), server);
            }
            ToolSettingsAction::SetMcpEnabled {
                id,
                enabled,
                confirmed,
                ..
            } => {
                if enabled && !confirmed {
                    return Err(
                        "Confirm that this executable is trusted before enabling MCP".into(),
                    );
                }
                let server = config.servers.get_mut(&id).ok_or("MCP server not found")?;
                server.enabled = enabled;
            }
            ToolSettingsAction::DeleteMcp { id, .. } => {
                if config.servers.remove(&id).is_none() {
                    return Err("MCP server not found".into());
                }
            }
        }
        let next = revision
            .checked_add(1)
            .ok_or("Tool settings revision exhausted")?;
        transaction.execute("INSERT INTO tool_settings(singleton, revision, config_json) VALUES(1, ?1, ?2) ON CONFLICT(singleton) DO UPDATE SET revision = excluded.revision, config_json = excluded.config_json", params![next, serde_json::to_string(&config).map_err(|_| "Tool settings serialization failed")?]).map_err(|_| "Tool settings could not be saved")?;
        transaction
            .commit()
            .map_err(|_| "Tool settings could not be saved")?;
        Ok(view(next, config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn server() -> ManagedMcpServer {
        ManagedMcpServer {
            id: "fixture".into(),
            display_name: "Fixture".into(),
            command: "/usr/bin/python3".into(),
            args: vec!["--version".into()],
            connect_timeout_secs: 5,
            enabled: true,
        }
    }
    fn fixture() -> (tempfile::TempDir, HubStore) {
        let data = tempfile::tempdir().unwrap();
        let store = HubStore::open(data.path().join("hub.db")).unwrap();
        (data, store)
    }

    #[test]
    fn tool_preferences_are_optimistic_persistent_and_safety_is_mandatory() {
        let (data, store) = fixture();
        let defaults = store.tool_settings().unwrap();
        assert_eq!(defaults.revision, 0);
        assert!(defaults.mcp_servers.is_empty());
        assert!(defaults
            .components
            .iter()
            .any(|c| c.id == "shell" && c.enabled));
        assert!(defaults
            .components
            .iter()
            .any(|c| c.id == "browser" && !c.enabled));
        for id in [
            "permission",
            "tool-timeout",
            "loop-detection",
            "security",
            "subprocess-loader",
            "unknown",
        ] {
            assert!(store
                .tool_settings_action(ToolSettingsAction::SetComponent {
                    revision: 0,
                    id: id.into(),
                    enabled: false
                })
                .is_err());
        }
        let next = store
            .tool_settings_action(ToolSettingsAction::SetComponent {
                revision: 0,
                id: "shell".into(),
                enabled: false,
            })
            .unwrap();
        assert!(!next.toggles()["shell"]);
        assert!(store
            .tool_settings_action(ToolSettingsAction::SetComponent {
                revision: 0,
                id: "core".into(),
                enabled: false
            })
            .is_err());
        drop(store);
        let store = HubStore::open(data.path().join("hub.db")).unwrap();
        assert!(!store.tool_settings().unwrap().toggles()["shell"]);
    }

    #[test]
    fn mcp_crud_requires_fresh_authorization_and_never_executes_on_save() {
        let (_data, store) = fixture();
        let mut fixture = server();
        fixture.command = "/does/not/exist".into(); // save must not spawn anything
        let created = store
            .tool_settings_action(ToolSettingsAction::CreateMcp {
                revision: 0,
                server: fixture.clone(),
            })
            .unwrap();
        assert!(!created.mcp_servers[0].enabled);
        assert!(created.enabled_mcp_servers().is_empty());
        assert!(store
            .tool_settings_action(ToolSettingsAction::SetMcpEnabled {
                revision: 1,
                id: "fixture".into(),
                enabled: true,
                confirmed: false
            })
            .is_err());
        let enabled = store
            .tool_settings_action(ToolSettingsAction::SetMcpEnabled {
                revision: 1,
                id: "fixture".into(),
                enabled: true,
                confirmed: true,
            })
            .unwrap();
        assert_eq!(enabled.enabled_mcp_servers().len(), 1);
        assert!(
            matches!(&enabled.enabled_mcp_servers()[0].transport, McpTransportConfig::Stdio { env, .. } if env.is_empty())
        );
        let edited = store
            .tool_settings_action(ToolSettingsAction::UpdateMcp {
                revision: 2,
                server: fixture,
            })
            .unwrap();
        assert!(!edited.mcp_servers[0].enabled);
        assert!(store
            .tool_settings_action(ToolSettingsAction::DeleteMcp {
                revision: 2,
                id: "fixture".into()
            })
            .is_err());
        assert!(store
            .tool_settings_action(ToolSettingsAction::DeleteMcp {
                revision: 3,
                id: "fixture".into()
            })
            .unwrap()
            .mcp_servers
            .is_empty());
    }

    #[test]
    fn mcp_schema_validation_capacity_and_corrupt_storage_fail_closed() {
        let (_data, store) = fixture();
        let mut invalid = server();
        invalid.command = "npx -y example".into();
        assert!(validate_server(&invalid).is_err());
        invalid = server();
        invalid.args = vec!["x".repeat(2049)];
        assert!(validate_server(&invalid).is_err());
        invalid = server();
        invalid.id = "bad:id".into();
        assert!(validate_server(&invalid).is_err());
        invalid = server();
        invalid.connect_timeout_secs = 0;
        assert!(validate_server(&invalid).is_err());
        for index in 0..32 {
            let mut row = server();
            row.id = format!("fixture-{index}");
            store
                .tool_settings_action(ToolSettingsAction::CreateMcp {
                    revision: index,
                    server: row,
                })
                .unwrap();
        }
        assert!(store
            .tool_settings_action(ToolSettingsAction::CreateMcp {
                revision: 32,
                server: server()
            })
            .is_err());
        assert!(serde_json::from_value::<ToolSettingsAction>(serde_json::json!({"op":"set_component","revision":32,"id":"shell","enabled":false,"extra":1})).is_err());
        store
            .lock()
            .unwrap()
            .execute("UPDATE tool_settings SET config_json = '{invalid}'", [])
            .unwrap();
        assert!(store.tool_settings().is_err());
        assert!(store
            .tool_settings_action(ToolSettingsAction::DeleteMcp {
                revision: 32,
                id: "fixture-0".into()
            })
            .is_err());
    }

    #[test]
    fn schema_twenty_one_migration_adds_defaults_without_existing_preferences() {
        let (data, store) = fixture();
        store
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE tool_settings; PRAGMA user_version = 21;")
            .unwrap();
        drop(store);
        let store = HubStore::open(data.path().join("hub.db")).unwrap();
        assert_eq!(store.tool_settings().unwrap().revision, 0);
        assert!(store.list_automations().unwrap().is_empty());
    }
}
