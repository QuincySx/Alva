//! Built-in extensions — each wraps a tool preset or middleware set.

use std::path::PathBuf;
use std::sync::Arc;

use alva_types::tool::Tool;
use alva_agent_core::middleware::Middleware;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::{Extension, ExtensionContext};

// ===========================================================================
// Tool extensions
// ===========================================================================

/// Core file I/O tools: read, write, edit, search, list.
pub struct CoreExtension;
#[async_trait]
impl Extension for CoreExtension {
    fn name(&self) -> &str { "core" }
    fn description(&self) -> &str { "Core file I/O tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::file_io() }
}

/// Shell execution tool.
pub struct ShellExtension;
#[async_trait]
impl Extension for ShellExtension {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str { "Shell execution" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::shell() }
}

/// Human interaction tool (ask_human).
pub struct InteractionExtension;
#[async_trait]
impl Extension for InteractionExtension {
    fn name(&self) -> &str { "interaction" }
    fn description(&self) -> &str { "Human interaction" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::interaction() }
}

/// Task management tools: create, update, get, list, output, stop.
pub struct TaskExtension;
#[async_trait]
impl Extension for TaskExtension {
    fn name(&self) -> &str { "task" }
    fn description(&self) -> &str { "Task management" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::task_management() }
}

/// Team / multi-agent coordination tools.
pub struct TeamExtension;
#[async_trait]
impl Extension for TeamExtension {
    fn name(&self) -> &str { "team" }
    fn description(&self) -> &str { "Team / multi-agent coordination" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::team() }
}

/// Planning and worktree tools.
pub struct PlanningExtension;
#[async_trait]
impl Extension for PlanningExtension {
    fn name(&self) -> &str { "planning" }
    fn description(&self) -> &str { "Planning and worktree tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        let mut tools = tool_presets::planning();
        tools.extend(tool_presets::worktree());
        tools
    }
}

/// Utility tools: sleep, config, notebook, skill, tool_search, schedule, remote.
pub struct UtilityExtension;
#[async_trait]
impl Extension for UtilityExtension {
    fn name(&self) -> &str { "utility" }
    fn description(&self) -> &str { "Utility tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::utility() }
}

/// Web tools: internet search, URL fetching.
pub struct WebExtension;
#[async_trait]
impl Extension for WebExtension {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str { "Web tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::web() }
}

/// Browser automation tools.
pub struct BrowserExtension;
#[async_trait]
impl Extension for BrowserExtension {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str { "Browser automation tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::browser_tools() }
}

/// All standard tools (file_io + shell + interaction + task + team + planning
/// + worktree + utility + web). Does NOT include browser.
pub struct AllStandardExtension;
#[async_trait]
impl Extension for AllStandardExtension {
    fn name(&self) -> &str { "all-standard" }
    fn description(&self) -> &str { "All standard tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::all_standard() }
}

// ===========================================================================
// Middleware extensions
// ===========================================================================

/// Guardrails: loop detection + dangling tool call + tool timeout.
pub struct GuardrailsExtension;
#[async_trait]
impl Extension for GuardrailsExtension {
    fn name(&self) -> &str { "guardrails" }
    fn description(&self) -> &str { "Loop detection, dangling tool call, and tool timeout" }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()),
            Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()),
            Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()),
        ]
    }
}

/// Context compaction middleware.
pub struct CompactionExtension;
#[async_trait]
impl Extension for CompactionExtension {
    fn name(&self) -> &str { "compaction" }
    fn description(&self) -> &str { "Context compaction" }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_runtime::middleware::CompactionMiddleware::default()),
        ]
    }
}

/// Checkpoint middleware — file backups before tool execution.
pub struct CheckpointExtension;
#[async_trait]
impl Extension for CheckpointExtension {
    fn name(&self) -> &str { "checkpoint" }
    fn description(&self) -> &str { "File checkpoint before tool execution" }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_runtime::middleware::CheckpointMiddleware::new()),
        ]
    }
}

/// Plan mode middleware — restricts tools to read-only when plan mode is active.
pub struct PlanModeExtension;
#[async_trait]
impl Extension for PlanModeExtension {
    fn name(&self) -> &str { "plan-mode" }
    fn description(&self) -> &str { "Plan mode (read-only tool restriction)" }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_runtime::middleware::PlanModeMiddleware::new(false)),
        ]
    }
}

/// Full production middleware stack: guardrails + compaction + checkpoint + plan mode.
pub struct ProductionExtension;
#[async_trait]
impl Extension for ProductionExtension {
    fn name(&self) -> &str { "production" }
    fn description(&self) -> &str { "Full production middleware stack" }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()),
            Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()),
            Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()),
            Arc::new(alva_agent_runtime::middleware::CompactionMiddleware::default()),
            Arc::new(alva_agent_runtime::middleware::CheckpointMiddleware::new()),
            Arc::new(alva_agent_runtime::middleware::PlanModeMiddleware::new(false)),
        ]
    }
}

// ===========================================================================
// Skill system extension
// ===========================================================================

use crate::skills::store::SkillStore;
use crate::skills::loader::SkillLoader;
use crate::skills::injector::SkillInjector;
use crate::skills::tools::{SearchSkillsTool, UseSkillTool};
use crate::skills::middleware::SkillInjectionMiddleware;
use crate::skills::skill_fs::FsSkillRepository;
use crate::skills::skill_ports::skill_repository::SkillRepository;

/// Skill system: discovery, loading, and context injection.
/// Provides SearchSkillsTool + UseSkillTool and SkillInjectionMiddleware.
pub struct SkillsExtension {
    store: Arc<SkillStore>,
    loader: Arc<SkillLoader>,
    injector: Arc<SkillInjector>,
}

impl SkillsExtension {
    /// Create a new SkillsExtension with the given skill directories.
    /// The first directory is used as primary (bundled/mbb/user subdirs).
    pub fn new(skill_dirs: Vec<PathBuf>) -> Self {
        let primary_dir = skill_dirs.first().cloned()
            .unwrap_or_else(|| PathBuf::from(".alva/skills"));

        let repo = Arc::new(FsSkillRepository::new(
            primary_dir.join("bundled"),
            primary_dir.join("mbb"),
            primary_dir.join("user"),
            primary_dir.join("state.json"),
        ));
        let store = Arc::new(SkillStore::new(repo.clone() as Arc<dyn SkillRepository>));
        let loader = Arc::new(SkillLoader::new(repo.clone() as Arc<dyn SkillRepository>));
        let injector = Arc::new(SkillInjector::new(SkillLoader::new(repo as Arc<dyn SkillRepository>)));

        Self { store, loader, injector }
    }

    /// Access the underlying SkillStore (e.g., for agent_template_service).
    pub fn store(&self) -> &Arc<SkillStore> {
        &self.store
    }
}

#[async_trait]
impl Extension for SkillsExtension {
    fn name(&self) -> &str { "skills" }
    fn description(&self) -> &str { "Skill discovery, loading, and context injection" }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(SearchSkillsTool { store: self.store.clone() }),
            Box::new(UseSkillTool { store: self.store.clone(), loader: self.loader.clone() }),
        ]
    }

    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(SkillInjectionMiddleware::with_defaults(
            self.store.clone(),
            self.injector.clone(),
        ))]
    }

    async fn configure(&self, _ctx: &ExtensionContext) {
        let _ = self.store.scan().await;
    }
}
