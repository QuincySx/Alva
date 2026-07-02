//! Built-in agent extensions.
//!
//! This crate consolidates every reference tool implementation (formerly
//! in `alva-agent-tools`) and every thin Extension wrapper (formerly in
//! `alva-app-core/src/extension/*.rs`). Callers compose only the features
//! they want. Heavy domain extensions (browser, memory) live in separate
//! `alva-app-extension-*` crates because they pull app-level concerns.

// Tool implementations and Extension wrappers are added task-by-task
// during the rest of the refactor.

pub mod media;
pub mod services;
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
pub mod ask_human;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod create_file;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod execute_shell;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod file_edit;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod find_files;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod grep_search;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod list_files;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod read_file;
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
#[cfg(all(feature = "utility", not(target_family = "wasm")))]
pub mod sleep_tool;
#[cfg(feature = "utility")]
pub mod tool_search;

// ---- web feature tools ----

#[cfg(all(feature = "web", not(target_family = "wasm")))]
pub mod internet_search;
#[cfg(all(feature = "web", not(target_family = "wasm")))]
pub mod read_url;
#[cfg(all(feature = "web", not(target_family = "wasm")))]
pub mod understand_video;

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
pub mod send_message;
#[cfg(feature = "team")]
pub mod team_create;
#[cfg(feature = "team")]
pub mod team_delete;

// ---- task feature tools ----

#[cfg(feature = "task")]
pub mod task_create;
#[cfg(feature = "task")]
pub mod task_get;
#[cfg(feature = "task")]
pub mod task_list;
#[cfg(feature = "task")]
pub mod task_output;
#[cfg(feature = "task")]
pub mod task_stop;
#[cfg(feature = "task")]
pub mod task_update;

// ---- schedule feature tools ----

#[cfg(feature = "schedule")]
pub mod remote_trigger;
#[cfg(feature = "schedule")]
pub mod schedule_cron;

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
            tools.push(Box::new(crate::understand_video::UnderstandVideoTool));
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

#[cfg(test)]
mod tool_preset_tests {
    //! Tests for `tool_presets::*` — pin the tool sets each Extension
    //! advertises. Adding a new tool to a module but forgetting to add
    //! it to its preset would silently leave that tool unreachable to
    //! agents using the wrapping Extension; the agent would only see
    //! `ToolNotFound` at runtime.
    //!
    //! Tests assume all features are enabled (workspace runs with
    //! `cargo test --all-features`).
    use super::tool_presets;

    fn names(tools: Vec<Box<dyn alva_kernel_abi::tool::Tool>>) -> Vec<String> {
        let mut names: Vec<String> = tools.iter().map(|t| t.name().to_string()).collect();
        names.sort();
        names
    }

    fn contains_all(actual: &[String], expected: &[&str]) {
        for e in expected {
            assert!(
                actual.iter().any(|n| n == e),
                "expected tool {e:?} in preset, got {actual:?}"
            );
        }
    }

    // -- Individual presets ------------------------------------------------

    #[cfg(all(feature = "core", not(target_family = "wasm")))]
    #[test]
    fn file_io_preset_includes_all_six_core_file_tools() {
        let got = names(tool_presets::file_io());
        contains_all(
            &got,
            &[
                "read_file",
                "create_file",
                "file_edit",
                "list_files",
                "find_files",
                "grep_search",
            ],
        );
        assert_eq!(
            got.len(),
            6,
            "file_io must have exactly 6 tools, got {got:?}"
        );
    }

    #[cfg(all(feature = "core", not(target_family = "wasm")))]
    #[test]
    fn shell_preset_includes_execute_shell() {
        let got = names(tool_presets::shell());
        contains_all(&got, &["execute_shell"]);
    }

    #[cfg(all(feature = "core", not(target_family = "wasm")))]
    #[test]
    fn interaction_preset_includes_ask_human() {
        let got = names(tool_presets::interaction());
        contains_all(&got, &["ask_human"]);
    }

    #[cfg(all(feature = "worktree", not(target_family = "wasm")))]
    #[test]
    fn worktree_preset_includes_enter_and_exit() {
        let got = names(tool_presets::worktree());
        contains_all(&got, &["enter_worktree", "exit_worktree"]);
    }

    #[cfg(feature = "task")]
    #[test]
    fn task_preset_includes_all_six_task_tools() {
        let got = names(tool_presets::task_management());
        contains_all(
            &got,
            &[
                "task_create",
                "task_update",
                "task_get",
                "task_list",
                "task_output",
                "task_stop",
            ],
        );
    }

    #[cfg(feature = "team")]
    #[test]
    fn team_preset_includes_create_delete_send() {
        let got = names(tool_presets::team());
        contains_all(&got, &["team_create", "team_delete", "send_message"]);
    }

    #[cfg(feature = "core")]
    #[test]
    fn planning_preset_includes_plan_mode_signals() {
        // Pin: enter_plan_mode + exit_plan_mode are wasm-safe (pure
        // mode signaling); todo_write is native-only but feature-gated
        // separately. At least the two mode tools must always show
        // up when `core` is on.
        let got = names(tool_presets::planning());
        contains_all(&got, &["enter_plan_mode", "exit_plan_mode"]);
    }

    #[cfg(feature = "utility")]
    #[test]
    fn utility_preset_includes_config_skill_tool_search() {
        let got = names(tool_presets::utility());
        contains_all(&got, &["config", "skill", "tool_search"]);
    }

    #[cfg(all(feature = "web", not(target_family = "wasm")))]
    #[test]
    fn web_preset_includes_internet_search_and_read_url() {
        let got = names(tool_presets::web());
        contains_all(&got, &["internet_search", "read_url", "understand_video"]);
    }

    #[cfg(all(feature = "notebook", not(target_family = "wasm")))]
    #[test]
    fn notebook_preset_includes_notebook_edit() {
        let got = names(tool_presets::notebook());
        contains_all(&got, &["notebook_edit"]);
    }

    // -- all_standard is the union ---------------------------------------

    #[test]
    fn all_standard_count_equals_sum_of_individual_presets() {
        // Pin: all_standard composes from each preset by extending;
        // a refactor that dropped a preset extension would silently
        // shrink the standard set.
        let total: usize = tool_presets::file_io().len()
            + tool_presets::shell().len()
            + tool_presets::interaction().len()
            + tool_presets::task_management().len()
            + tool_presets::team().len()
            + tool_presets::planning().len()
            + tool_presets::worktree().len()
            + tool_presets::utility().len()
            + tool_presets::web().len()
            + tool_presets::notebook().len()
            + tool_presets::schedule().len();
        assert_eq!(
            tool_presets::all_standard().len(),
            total,
            "all_standard must equal sum of preset lengths"
        );
    }

    #[test]
    fn all_standard_includes_a_tool_from_each_enabled_preset() {
        // Pin: spot-check that one signature tool from each major
        // preset survives the all_standard() composition. Catches
        // a refactor that dropped, say, `.extend(web())`.
        let got = names(tool_presets::all_standard());
        #[cfg(all(feature = "core", not(target_family = "wasm")))]
        {
            assert!(got.contains(&"read_file".to_string()));
            assert!(got.contains(&"execute_shell".to_string()));
            assert!(got.contains(&"ask_human".to_string()));
        }
        #[cfg(all(feature = "web", not(target_family = "wasm")))]
        assert!(got.contains(&"internet_search".to_string()));
        #[cfg(feature = "task")]
        assert!(got.contains(&"task_create".to_string()));
        #[cfg(feature = "team")]
        assert!(got.contains(&"team_create".to_string()));
    }
}
