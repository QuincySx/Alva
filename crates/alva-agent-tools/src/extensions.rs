//! Built-in extensions — capability packages for common agent setups.

use alva_agent_core::extension::{Extension, ExtensionAPI};

use crate::tool_presets;

/// Core file tools: read, write, edit, search, list.
pub struct CoreExtension;

impl Extension for CoreExtension {
    fn name(&self) -> &str { "core" }
    fn description(&self) -> &str { "Core file I/O tools (read, write, edit, search, list)" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::file_io());
    }
}

/// Shell execution.
pub struct ShellExtension;

impl Extension for ShellExtension {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str { "Shell command execution" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::shell());
    }
}

/// Human interaction (ask_human).
pub struct InteractionExtension;

impl Extension for InteractionExtension {
    fn name(&self) -> &str { "interaction" }
    fn description(&self) -> &str { "Human interaction tools" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::interaction());
    }
}

/// Task management (create, update, get, list, output, stop).
pub struct TaskExtension;

impl Extension for TaskExtension {
    fn name(&self) -> &str { "tasks" }
    fn description(&self) -> &str { "Task tracking and management" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::task_management());
    }
}

/// Team / multi-agent coordination.
pub struct TeamExtension;

impl Extension for TeamExtension {
    fn name(&self) -> &str { "team" }
    fn description(&self) -> &str { "Multi-agent team coordination" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::team());
    }
}

/// Planning tools (plan mode, worktree, todo).
pub struct PlanningExtension;

impl Extension for PlanningExtension {
    fn name(&self) -> &str { "planning" }
    fn description(&self) -> &str { "Planning mode, worktree, and TODO tracking" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::planning());
        api.add_tools(tool_presets::worktree());
    }
}

/// Utility tools (sleep, config, notebook, skill, etc.)
pub struct UtilityExtension;

impl Extension for UtilityExtension {
    fn name(&self) -> &str { "utility" }
    fn description(&self) -> &str { "Utility tools: sleep, config, notebook, skill, search, cron, remote" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::utility());
    }
}

/// Web tools (internet search, URL reading).
pub struct WebExtension;

impl Extension for WebExtension {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str { "Internet search and URL fetching" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::web());
    }
}

/// Browser automation tools.
pub struct BrowserExtension;

impl Extension for BrowserExtension {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str { "Browser automation (start, navigate, click, screenshot)" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::browser_tools());
    }
}

/// All standard tools in one extension.
/// Equivalent to: core + shell + interaction + tasks + team + planning + utility + web.
pub struct AllStandardExtension;

impl Extension for AllStandardExtension {
    fn name(&self) -> &str { "all-standard" }
    fn description(&self) -> &str { "All standard tools (file, shell, tasks, team, planning, utility, web)" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_tools(tool_presets::all_standard());
    }
}
