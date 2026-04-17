//! Built-in agent extensions.
//!
//! This crate consolidates every reference tool implementation (formerly
//! in `alva-agent-tools`) and every thin Extension wrapper (formerly in
//! `alva-app-core/src/extension/*.rs`). Callers compose only the features
//! they want. Heavy domain extensions (browser, memory) live in separate
//! `alva-app-extension-*` crates because they pull app-level concerns.

// Tool implementations and Extension wrappers are added task-by-task
// during the rest of the refactor.

pub mod truncate;
pub mod wrappers;

#[cfg(not(target_family = "wasm"))]
pub mod walkdir;

/// Local-OS `ToolFs` adapter. Native-only (`tokio::process` + `tokio::fs`).
/// Built-in tool implementations use `LocalToolFs::new(workspace)` as a
/// fallback when `ToolExecutionContext` doesn't provide a ToolFs handle.
#[cfg(not(target_family = "wasm"))]
pub mod local_fs;

#[cfg(not(target_family = "wasm"))]
pub use local_fs::LocalToolFs;

// MockToolFs re-exported for test modules inside migrated tools.
pub use alva_agent_core::MockToolFs;

// ---- core feature tools ----

#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod read_file;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod create_file;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod file_edit;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod list_files;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod find_files;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod grep_search;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod execute_shell;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod ask_human;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod todo_write;

// Plan mode primitives are pure signaling — wasm-safe.
#[cfg(feature = "core")]
pub mod enter_plan_mode;
#[cfg(feature = "core")]
pub mod exit_plan_mode;

// Placeholder agent_tool — pure struct, wasm-safe.
#[cfg(feature = "core")]
pub mod agent_tool;

// ---- utility feature tools ----

#[cfg(feature = "utility")]
pub mod config_tool;
#[cfg(feature = "utility")]
pub mod skill_tool;
#[cfg(feature = "utility")]
pub mod tool_search;
#[cfg(all(feature = "utility", not(target_family = "wasm")))]
pub mod sleep_tool;

// ---- web feature tools ----

#[cfg(all(feature = "web", not(target_family = "wasm")))]
pub mod internet_search;
#[cfg(all(feature = "web", not(target_family = "wasm")))]
pub mod read_url;

// ---- notebook feature tool ----

#[cfg(all(feature = "notebook", not(target_family = "wasm")))]
pub mod notebook_edit;

// ---- worktree feature tools ----

#[cfg(all(feature = "worktree", not(target_family = "wasm")))]
pub mod enter_worktree;
#[cfg(all(feature = "worktree", not(target_family = "wasm")))]
pub mod exit_worktree;

// ---- team feature tools ----

#[cfg(feature = "team")]
pub mod team_create;
#[cfg(feature = "team")]
pub mod team_delete;
#[cfg(feature = "team")]
pub mod send_message;

// ---- task feature tools ----

#[cfg(feature = "task")]
pub mod task_create;
#[cfg(feature = "task")]
pub mod task_update;
#[cfg(feature = "task")]
pub mod task_get;
#[cfg(feature = "task")]
pub mod task_list;
#[cfg(feature = "task")]
pub mod task_output;
#[cfg(feature = "task")]
pub mod task_stop;

// ---- schedule feature tools ----

#[cfg(feature = "schedule")]
pub mod schedule_cron;
#[cfg(feature = "schedule")]
pub mod remote_trigger;

// ---------------------------------------------------------------------------
// Tool presets — grouped by capability domain
// ---------------------------------------------------------------------------

/// Pre-built tool sets for common use cases. Each preset returns only the
/// tools whose feature is currently enabled; disabled groups return an
/// empty Vec, so callers can unconditionally compose `all_standard()`
/// regardless of feature selection.
pub mod tool_presets {
    use alva_kernel_abi::tool::Tool;

    /// Core file tools: read (with image support), write, edit, search, list.
    pub fn file_io() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(all(feature = "core", not(target_family = "wasm")))]
        {
            tools.push(Box::new(crate::read_file::ReadFileTool));
            tools.push(Box::new(crate::create_file::CreateFileTool));
            tools.push(Box::new(crate::file_edit::FileEditTool));
            tools.push(Box::new(crate::list_files::ListFilesTool));
            tools.push(Box::new(crate::find_files::FindFilesTool));
            tools.push(Box::new(crate::grep_search::GrepSearchTool));
        }
        tools
    }

    /// Shell execution.
    pub fn shell() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(all(feature = "core", not(target_family = "wasm")))]
        {
            tools.push(Box::new(crate::execute_shell::ExecuteShellTool));
        }
        tools
    }

    /// Human interaction (stdin).
    pub fn interaction() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(all(feature = "core", not(target_family = "wasm")))]
        {
            tools.push(Box::new(crate::ask_human::AskHumanTool));
        }
        tools
    }

    /// Git worktree tools.
    pub fn worktree() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(all(feature = "worktree", not(target_family = "wasm")))]
        {
            tools.push(Box::new(crate::enter_worktree::EnterWorktreeTool));
            tools.push(Box::new(crate::exit_worktree::ExitWorktreeTool));
        }
        tools
    }

    /// Task management: create, update, get, list, output, stop.
    pub fn task_management() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(feature = "task")]
        {
            tools.push(Box::new(crate::task_create::TaskCreateTool));
            tools.push(Box::new(crate::task_update::TaskUpdateTool));
            tools.push(Box::new(crate::task_get::TaskGetTool));
            tools.push(Box::new(crate::task_list::TaskListTool));
            tools.push(Box::new(crate::task_output::TaskOutputTool));
            tools.push(Box::new(crate::task_stop::TaskStopTool));
        }
        tools
    }

    /// Team / multi-agent coordination.
    pub fn team() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(feature = "team")]
        {
            tools.push(Box::new(crate::team_create::TeamCreateTool));
            tools.push(Box::new(crate::team_delete::TeamDeleteTool));
            tools.push(Box::new(crate::send_message::SendMessageTool));
        }
        tools
    }

    /// Planning and mode switching. `todo_write` is native-only; the rest
    /// are pure mode signaling (wasm-safe).
    pub fn planning() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(feature = "core")]
        {
            tools.push(Box::new(crate::enter_plan_mode::EnterPlanModeTool));
            tools.push(Box::new(crate::exit_plan_mode::ExitPlanModeTool));
        }
        #[cfg(all(feature = "core", not(target_family = "wasm")))]
        {
            tools.push(Box::new(crate::todo_write::TodoWriteTool));
        }
        tools
    }

    /// Utility tools: config, skill, tool_search, sleep.
    pub fn utility() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(feature = "utility")]
        {
            tools.push(Box::new(crate::config_tool::ConfigTool));
            tools.push(Box::new(crate::skill_tool::SkillTool));
            tools.push(Box::new(crate::tool_search::ToolSearchTool));
        }
        #[cfg(all(feature = "utility", not(target_family = "wasm")))]
        {
            tools.push(Box::new(crate::sleep_tool::SleepTool));
        }
        tools
    }

    /// Web tools: internet search, URL fetching.
    pub fn web() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(all(feature = "web", not(target_family = "wasm")))]
        {
            tools.push(Box::new(crate::internet_search::InternetSearchTool));
            tools.push(Box::new(crate::read_url::ReadUrlTool));
        }
        tools
    }

    /// Notebook tools.
    pub fn notebook() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(all(feature = "notebook", not(target_family = "wasm")))]
        {
            tools.push(Box::new(crate::notebook_edit::NotebookEditTool));
        }
        tools
    }

    /// Schedule/remote trigger tools.
    pub fn schedule() -> Vec<Box<dyn Tool>> {
        #[allow(unused_mut)]
        let mut tools: Vec<Box<dyn Tool>> = Vec::new();
        #[cfg(feature = "schedule")]
        {
            tools.push(Box::new(crate::schedule_cron::ScheduleCronTool));
            tools.push(Box::new(crate::remote_trigger::RemoteTriggerTool));
        }
        tools
    }

    /// All standard tools available under the currently enabled features.
    /// Browser is never included — depend on `alva-app-extension-browser`
    /// for that.
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
        tools.extend(notebook());
        tools.extend(schedule());
        tools
    }
}

// ---------------------------------------------------------------------------
// Legacy registration shim
// ---------------------------------------------------------------------------

/// Register all built-in tools into a `ToolRegistry`. Mirrors the legacy
/// `alva_agent_tools::register_builtin_tools` signature so host-native
/// can migrate gradually.
pub fn register_builtin_tools(registry: &mut alva_kernel_abi::ToolRegistry) {
    for tool in tool_presets::all_standard() {
        registry.register(tool);
    }
    #[cfg(feature = "core")]
    {
        registry.register(Box::new(crate::agent_tool::AgentTool));
    }
}
