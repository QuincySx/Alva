// INPUT:  alva_kernel_abi::ToolRegistry, all tool modules
// OUTPUT: register_builtin_tools, register_all_tools
// POS:    Crate root — declares tool modules and provides registration functions.
//! Built-in tool implementations for the agent framework.
//!
//! Standard tools (always available):
//!   ask_human, create_file, execute_shell, file_edit, find_files,
//!   grep_search, list_files, read_file, view_image
//!
//! Native-only tools (feature = "native", disabled on wasm):
//!   internet_search, read_url
//!
//! Browser tools (feature = "browser"):
//!   browser_start, browser_stop, browser_navigate, browser_action,
//!   browser_snapshot, browser_screenshot, browser_status

pub mod ask_human;
pub mod create_file;
pub mod execute_shell;
pub mod file_edit;
pub mod find_files;
pub mod grep_search;
pub mod list_files;
pub mod mock_fs;
pub mod read_file;
pub mod truncate;
pub mod view_image;

// --- Phase 3: Additional tools ---
pub mod agent_tool;
pub mod config_tool;
pub mod enter_plan_mode;
pub mod enter_worktree;
pub mod exit_plan_mode;
pub mod exit_worktree;
pub mod notebook_edit;
pub mod remote_trigger;
pub mod schedule_cron;
pub mod send_message;
pub mod skill_tool;
pub mod sleep_tool;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_output;
pub mod task_stop;
pub mod task_update;
pub mod team_create;
pub mod team_delete;
pub mod todo_write;
pub mod tool_search;

#[cfg(not(target_family = "wasm"))]
pub mod local_fs;
#[cfg(not(target_family = "wasm"))]
pub use local_fs::{walk_dir, walk_dir_filtered, LocalToolFs};

pub use mock_fs::MockToolFs;

#[cfg(feature = "native")]
pub mod internet_search;
#[cfg(feature = "native")]
pub mod read_url;

#[cfg(feature = "browser")]
pub mod browser;

use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::ToolRegistry;

/// Batch-register tools into a ToolRegistry.
#[macro_export]
macro_rules! register_tools {
    ($registry:expr, $($tool:expr),* $(,)?) => {
        $( $registry.register(Box::new($tool)); )*
    };
}

// ---------------------------------------------------------------------------
// Tool presets — grouped by capability domain
// ---------------------------------------------------------------------------

/// Pre-built tool sets for common use cases.
/// Callers compose what they need via `BaseAgent::builder().tools(...)`.
pub mod tool_presets {
    use super::*;

    /// Core file tools: read, write, edit, search, list.
    pub fn file_io() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(read_file::ReadFileTool),
            Box::new(create_file::CreateFileTool),
            Box::new(file_edit::FileEditTool),
            Box::new(list_files::ListFilesTool),
            Box::new(find_files::FindFilesTool),
            Box::new(grep_search::GrepSearchTool),
            Box::new(view_image::ViewImageTool),
        ]
    }

    /// Shell execution.
    pub fn shell() -> Vec<Box<dyn Tool>> {
        vec![Box::new(execute_shell::ExecuteShellTool)]
    }

    /// Human interaction.
    pub fn interaction() -> Vec<Box<dyn Tool>> {
        vec![Box::new(ask_human::AskHumanTool)]
    }

    /// Task management: create, update, get, list, output, stop.
    pub fn task_management() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(task_create::TaskCreateTool),
            Box::new(task_update::TaskUpdateTool),
            Box::new(task_get::TaskGetTool),
            Box::new(task_list::TaskListTool),
            Box::new(task_output::TaskOutputTool),
            Box::new(task_stop::TaskStopTool),
        ]
    }

    /// Team / multi-agent coordination.
    pub fn team() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(team_create::TeamCreateTool),
            Box::new(team_delete::TeamDeleteTool),
            Box::new(send_message::SendMessageTool),
        ]
    }

    /// Planning and mode switching.
    pub fn planning() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(enter_plan_mode::EnterPlanModeTool),
            Box::new(exit_plan_mode::ExitPlanModeTool),
            Box::new(todo_write::TodoWriteTool),
        ]
    }

    /// Git worktree tools.
    pub fn worktree() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(enter_worktree::EnterWorktreeTool),
            Box::new(exit_worktree::ExitWorktreeTool),
        ]
    }

    /// Utility tools: sleep, config, notebook, skill, tool_search, schedule, remote.
    pub fn utility() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(sleep_tool::SleepTool),
            Box::new(config_tool::ConfigTool),
            Box::new(notebook_edit::NotebookEditTool),
            Box::new(skill_tool::SkillTool),
            Box::new(tool_search::ToolSearchTool),
            Box::new(schedule_cron::ScheduleCronTool),
            Box::new(remote_trigger::RemoteTriggerTool),
        ]
    }

    /// Web tools (native only): internet search, URL fetching.
    #[cfg(feature = "native")]
    pub fn web() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(internet_search::InternetSearchTool),
            Box::new(read_url::ReadUrlTool),
        ]
    }

    #[cfg(not(feature = "native"))]
    pub fn web() -> Vec<Box<dyn Tool>> {
        vec![]
    }

    /// Browser automation tools (browser feature only).
    #[cfg(feature = "browser")]
    pub fn browser_tools() -> Vec<Box<dyn Tool>> {
        let manager = browser::browser_manager::shared_browser_manager();
        vec![
            Box::new(browser::BrowserStartTool { manager: manager.clone() }),
            Box::new(browser::BrowserStopTool { manager: manager.clone() }),
            Box::new(browser::BrowserNavigateTool { manager: manager.clone() }),
            Box::new(browser::BrowserActionTool { manager: manager.clone() }),
            Box::new(browser::BrowserSnapshotTool { manager: manager.clone() }),
            Box::new(browser::BrowserScreenshotTool { manager: manager.clone() }),
            Box::new(browser::BrowserStatusTool { manager: manager.clone() }),
        ]
    }

    #[cfg(not(feature = "browser"))]
    pub fn browser_tools() -> Vec<Box<dyn Tool>> {
        vec![]
    }

    /// All standard tools (file_io + shell + interaction + task + team + planning
    /// + worktree + utility + web). Does NOT include browser or agent spawn.
    pub fn all_standard() -> Vec<Box<dyn Tool>> {
        let mut tools = Vec::new();
        tools.extend(file_io());
        tools.extend(shell());
        tools.extend(interaction());
        tools.extend(task_management());
        tools.extend(team());
        tools.extend(planning());
        tools.extend(worktree());
        tools.extend(utility());
        tools.extend(web());
        tools
    }
}

// ---------------------------------------------------------------------------
// Legacy registration functions (for backward compat during migration)
// ---------------------------------------------------------------------------

/// Register all built-in tools into a ToolRegistry.
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    for tool in tool_presets::all_standard() {
        registry.register(tool);
    }
    // Legacy: include placeholder AgentTool
    registry.register(Box::new(agent_tool::AgentTool));
}

/// Register all built-in tools including browser tools into a ToolRegistry.
pub fn register_all_tools(registry: &mut ToolRegistry) {
    register_builtin_tools(registry);
    for tool in tool_presets::browser_tools() {
        registry.register(tool);
    }
}
