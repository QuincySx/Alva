// INPUT:  alva-agent-extension-builtin tool presets
// OUTPUT: catalog of component tools for the ToolPicker
// POS:    Migrated from alva-app-tauri/src/agent/discovery.rs
//         (tools_for_component / PluginToolInfo; no tauri).

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct PluginToolInfo {
    pub name: String,
    pub description: String,
}

fn tools_from_preset(preset: Vec<Box<dyn alva_kernel_abi::Tool>>) -> Vec<PluginToolInfo> {
    preset
        .iter()
        .map(|t| PluginToolInfo {
            name: t.name().to_string(),
            description: t.description().to_string(),
        })
        .collect()
}

/// The tool list each component contributes, keyed by `ComponentMeta::id`
/// — reconstructed from the same `tool_presets::*` groups that
/// `apply_components` actually attaches.
pub fn tools_for_component(id: &str) -> Vec<PluginToolInfo> {
    use alva_agent_extension_builtin::tool_presets;
    match id {
        "core" => tools_from_preset(tool_presets::file_io()),
        "shell" => tools_from_preset(tool_presets::shell()),
        "interaction" => tools_from_preset(tool_presets::interaction()),
        "web" => tools_from_preset(tool_presets::web()),
        "utility" => tools_from_preset(tool_presets::utility()),
        "planning" => {
            let mut t = tools_from_preset(tool_presets::planning());
            t.extend(tools_from_preset(tool_presets::worktree()));
            t
        }
        "task" => tools_from_preset(tool_presets::task_management()),
        "team" => tools_from_preset(tool_presets::team()),
        "browser" => tools_from_preset(alva_app_extension_browser::browser_tools()),
        "skills" => vec![PluginToolInfo {
            name: "skill".into(),
            description: "Load a skill by exact name".into(),
        }],
        "mcp" => vec![PluginToolInfo {
            name: "mcp_runtime".into(),
            description: "MCP operations: list_servers / list_tools / call_tool".into(),
        }],
        "sub-agents" => vec![PluginToolInfo {
            name: "agent".into(),
            description: "Spawn a sub-agent with a role and tool subset".into(),
        }],
        _ => Vec::new(),
    }
}

/// Every tool in the default component set, for the picker.
pub fn all_default_tools() -> Vec<PluginToolInfo> {
    alva_app_core::components::COMPONENTS
        .iter()
        .filter(|component| component.default_on)
        .flat_map(|component| tools_for_component(component.id))
        .collect()
}
