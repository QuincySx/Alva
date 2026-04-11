//! Built-in extensions — capability packages for common agent setups.

use alva_agent_core::Extension;
use alva_types::tool::Tool;

use crate::tool_presets;

pub struct CoreExtension;
impl Extension for CoreExtension {
    fn name(&self) -> &str { "core" }
    fn description(&self) -> &str { "Core file I/O tools (read, write, edit, search, list)" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::file_io() }
}

pub struct ShellExtension;
impl Extension for ShellExtension {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str { "Shell command execution" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::shell() }
}

pub struct InteractionExtension;
impl Extension for InteractionExtension {
    fn name(&self) -> &str { "interaction" }
    fn description(&self) -> &str { "Human interaction tools" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::interaction() }
}

pub struct TaskExtension;
impl Extension for TaskExtension {
    fn name(&self) -> &str { "tasks" }
    fn description(&self) -> &str { "Task tracking and management" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::task_management() }
}

pub struct TeamExtension;
impl Extension for TeamExtension {
    fn name(&self) -> &str { "team" }
    fn description(&self) -> &str { "Multi-agent team coordination" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::team() }
}

pub struct PlanningExtension;
impl Extension for PlanningExtension {
    fn name(&self) -> &str { "planning" }
    fn description(&self) -> &str { "Planning mode, worktree, and TODO tracking" }
    fn tools(&self) -> Vec<Box<dyn Tool>> {
        let mut t = tool_presets::planning();
        t.extend(tool_presets::worktree());
        t
    }
}

pub struct UtilityExtension;
impl Extension for UtilityExtension {
    fn name(&self) -> &str { "utility" }
    fn description(&self) -> &str { "Utility tools: sleep, config, notebook, skill, search, cron, remote" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::utility() }
}

pub struct WebExtension;
impl Extension for WebExtension {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str { "Internet search and URL fetching" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::web() }
}

pub struct BrowserExtension;
impl Extension for BrowserExtension {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str { "Browser automation" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::browser_tools() }
}

/// All standard tools in one extension.
pub struct AllStandardExtension;
impl Extension for AllStandardExtension {
    fn name(&self) -> &str { "all-standard" }
    fn description(&self) -> &str { "All standard tools" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::all_standard() }
}
