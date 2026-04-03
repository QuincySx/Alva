// INPUT:  alva_types::ToolRegistry, all tool modules
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

use alva_types::ToolRegistry;

/// Batch-register tools into a ToolRegistry.
#[macro_export]
macro_rules! register_tools {
    ($registry:expr, $($tool:expr),* $(,)?) => {
        $( $registry.register(Box::new($tool)); )*
    };
}

/// Register all built-in tools into a ToolRegistry
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    register_tools!(
        registry,
        execute_shell::ExecuteShellTool,
        create_file::CreateFileTool,
        file_edit::FileEditTool,
        read_file::ReadFileTool,
        find_files::FindFilesTool,
        grep_search::GrepSearchTool,
        list_files::ListFilesTool,
        ask_human::AskHumanTool,
        view_image::ViewImageTool,
        // Phase 3 tools
        task_create::TaskCreateTool,
        task_update::TaskUpdateTool,
        task_get::TaskGetTool,
        task_list::TaskListTool,
        task_output::TaskOutputTool,
        task_stop::TaskStopTool,
        team_create::TeamCreateTool,
        team_delete::TeamDeleteTool,
        agent_tool::AgentTool,
        send_message::SendMessageTool,
        skill_tool::SkillTool,
        tool_search::ToolSearchTool,
        sleep_tool::SleepTool,
        enter_plan_mode::EnterPlanModeTool,
        exit_plan_mode::ExitPlanModeTool,
        notebook_edit::NotebookEditTool,
        config_tool::ConfigTool,
        todo_write::TodoWriteTool,
        schedule_cron::ScheduleCronTool,
        remote_trigger::RemoteTriggerTool,
        enter_worktree::EnterWorktreeTool,
        exit_worktree::ExitWorktreeTool,
    );

    #[cfg(feature = "native")]
    register_tools!(
        registry,
        internet_search::InternetSearchTool,
        read_url::ReadUrlTool,
    );
}

/// Register all built-in tools including browser tools into a ToolRegistry.
///
/// Browser tools share a `BrowserManager` instance for coordinating Chrome lifecycle.
#[cfg(feature = "browser")]
pub fn register_all_tools(registry: &mut ToolRegistry) {
    // Standard tools
    register_builtin_tools(registry);

    // Browser tools — all share the same BrowserManager
    let manager = browser::browser_manager::shared_browser_manager();
    registry.register(Box::new(browser::BrowserStartTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserStopTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserNavigateTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserActionTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserSnapshotTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserScreenshotTool {
        manager: manager.clone(),
    }));
    registry.register(Box::new(browser::BrowserStatusTool {
        manager: manager.clone(),
    }));
}

#[cfg(not(feature = "browser"))]
pub fn register_all_tools(registry: &mut ToolRegistry) {
    register_builtin_tools(registry);
}
