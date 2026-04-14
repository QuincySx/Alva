// INPUT:  alva_kernel_abi::ToolRegistry, all tool modules
// OUTPUT: register_builtin_tools
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
//! Browser tools moved to a separate crate `alva-agent-browser-tools`
//! so chromiumoxide (which pulls mio) doesn't block this crate from
//! compiling on wasm32.

// ---- wasm-safe tools (no fs / shell / stdin / direct tokio::time) ----
pub mod mock_fs;
pub mod truncate;

// Pure-data / signaling tools — no platform I/O
pub mod agent_tool;
pub mod config_tool;
pub mod enter_plan_mode;
pub mod exit_plan_mode;
pub mod remote_trigger;
pub mod schedule_cron;
pub mod send_message;
pub mod skill_tool;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_output;
pub mod task_stop;
pub mod task_update;
pub mod team_create;
pub mod team_delete;
pub mod tool_search;

// ---- native-only tools (depend on local_fs / stdin / tokio::time) ----
#[cfg(not(target_family = "wasm"))]
pub mod ask_human;        // stdin
#[cfg(not(target_family = "wasm"))]
pub mod create_file;       // local_fs
#[cfg(not(target_family = "wasm"))]
pub mod execute_shell;     // local_fs (shell exec)
#[cfg(not(target_family = "wasm"))]
pub mod file_edit;         // local_fs
#[cfg(not(target_family = "wasm"))]
pub mod find_files;        // local_fs (walkdir)
#[cfg(not(target_family = "wasm"))]
pub mod grep_search;       // local_fs (walkdir)
#[cfg(not(target_family = "wasm"))]
pub mod list_files;        // local_fs
#[cfg(not(target_family = "wasm"))]
pub mod read_file;         // local_fs
#[cfg(not(target_family = "wasm"))]
pub mod view_image;        // local_fs
#[cfg(not(target_family = "wasm"))]
pub mod enter_worktree;    // local_fs (git via shell)
#[cfg(not(target_family = "wasm"))]
pub mod exit_worktree;     // local_fs (git via shell)
#[cfg(not(target_family = "wasm"))]
pub mod notebook_edit;     // local_fs
#[cfg(not(target_family = "wasm"))]
pub mod sleep_tool;        // tokio::time::sleep + select!
#[cfg(not(target_family = "wasm"))]
pub mod todo_write;        // local_fs (writes CLAUDE.md)

#[cfg(not(target_family = "wasm"))]
pub mod local_fs;
#[cfg(not(target_family = "wasm"))]
pub use local_fs::{walk_dir, walk_dir_filtered, LocalToolFs};

pub use mock_fs::MockToolFs;

#[cfg(all(not(target_family = "wasm"), feature = "native"))]
pub mod internet_search;
#[cfg(all(not(target_family = "wasm"), feature = "native"))]
pub mod read_url;

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

    // ---- native-only presets (depend on local_fs / stdin / shell) ----

    /// Core file tools: read, write, edit, search, list. **Native only.**
    #[cfg(not(target_family = "wasm"))]
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
    #[cfg(target_family = "wasm")]
    pub fn file_io() -> Vec<Box<dyn Tool>> { Vec::new() }

    /// Shell execution. **Native only.**
    #[cfg(not(target_family = "wasm"))]
    pub fn shell() -> Vec<Box<dyn Tool>> {
        vec![Box::new(execute_shell::ExecuteShellTool)]
    }
    #[cfg(target_family = "wasm")]
    pub fn shell() -> Vec<Box<dyn Tool>> { Vec::new() }

    /// Human interaction (stdin). **Native only.**
    #[cfg(not(target_family = "wasm"))]
    pub fn interaction() -> Vec<Box<dyn Tool>> {
        vec![Box::new(ask_human::AskHumanTool)]
    }
    #[cfg(target_family = "wasm")]
    pub fn interaction() -> Vec<Box<dyn Tool>> { Vec::new() }

    /// Git worktree tools. **Native only.**
    #[cfg(not(target_family = "wasm"))]
    pub fn worktree() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(enter_worktree::EnterWorktreeTool),
            Box::new(exit_worktree::ExitWorktreeTool),
        ]
    }
    #[cfg(target_family = "wasm")]
    pub fn worktree() -> Vec<Box<dyn Tool>> { Vec::new() }

    // ---- wasm-safe presets (pure data / signaling) ----

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

    /// Planning and mode switching. todo_write writes to CLAUDE.md so
    /// it's gated to native; the rest are pure mode signaling.
    pub fn planning() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(enter_plan_mode::EnterPlanModeTool),
            Box::new(exit_plan_mode::ExitPlanModeTool),
        ];
        #[cfg(not(target_family = "wasm"))]
        tools.push(Box::new(todo_write::TodoWriteTool));
        tools
    }

    /// Utility tools: sleep, config, notebook, skill, tool_search, schedule,
    /// remote. sleep_tool / notebook_edit are gated to native (they use
    /// tokio::time and local_fs respectively); the rest are pure data.
    pub fn utility() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = vec![
            Box::new(config_tool::ConfigTool),
            Box::new(skill_tool::SkillTool),
            Box::new(tool_search::ToolSearchTool),
            Box::new(schedule_cron::ScheduleCronTool),
            Box::new(remote_trigger::RemoteTriggerTool),
        ];
        #[cfg(not(target_family = "wasm"))]
        {
            tools.push(Box::new(sleep_tool::SleepTool));
            tools.push(Box::new(notebook_edit::NotebookEditTool));
        }
        tools
    }

    /// Web tools (native + feature flag): internet search, URL fetching.
    #[cfg(all(not(target_family = "wasm"), feature = "native"))]
    pub fn web() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(internet_search::InternetSearchTool),
            Box::new(read_url::ReadUrlTool),
        ]
    }

    #[cfg(not(all(not(target_family = "wasm"), feature = "native")))]
    pub fn web() -> Vec<Box<dyn Tool>> {
        vec![]
    }

    /// All standard tools available on the current target. On native this
    /// is file_io + shell + interaction + task + team + planning + worktree
    /// + utility + web. On wasm32 the native-only presets degrade to empty
    /// Vecs, so the result is just task + team + planning(no todo) + utility
    /// (no sleep/notebook). Browser is **never** included — depend on
    /// `alva-app-extension-browser` for that.
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

/// Register all built-in tools into a ToolRegistry. Does NOT include
/// browser tools — depend on `alva-agent-browser-tools` for those.
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    for tool in tool_presets::all_standard() {
        registry.register(tool);
    }
    // Legacy: include placeholder AgentTool
    registry.register(Box::new(agent_tool::AgentTool));
}
