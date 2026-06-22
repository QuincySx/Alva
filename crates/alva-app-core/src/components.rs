// INPUT:  BaseAgentBuilder, ComponentToggles, ComponentContext, extension::* plugins, builtins/host-native middleware
// OUTPUT: ComponentKind, ComponentMeta, COMPONENTS, ComponentToggles, is_on, ComponentContext, apply_components
// POS:    Single source of truth for the flat, per-component on/off catalog. CLI/Tauri/tests all call `apply_components`
//         instead of hand-copying `.plugin()/.middleware()` chains. substrate (approval/checkpoint wiring + auto
//         memory/security/system_context) is NOT in this table — it is always present and wired by the caller.

//! Flat component catalog + assembly switchboard.
//!
//! A *component* is one user-toggleable agent capability (a `Plugin` that
//! registers tools/services, or a `Middleware`). The catalog ([`COMPONENTS`])
//! is pure data: id / label / description / category / default-on / kind.
//! The construction logic lives in [`apply_components`]'s `match id` — data
//! (what exists, how it displays, whether it defaults on) is kept separate
//! from construction (how to build it). Adding a component = one row here +
//! one match arm.
//!
//! **Not in this table (substrate, always present):** the `ApprovalPlugin`
//! (REPL needs its `approval_rx`), the checkpoint *callback* wiring, and the
//! auto-attached memory / security / system-context plugins that
//! `BaseAgentBuilder::build` installs. The `checkpoint` row below is the
//! *auto-archiving* `CheckpointMiddleware`, a distinct, toggleable thing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alva_kernel_abi::ProviderRegistry;

use crate::base_agent::BaseAgentBuilder;
use crate::settings::HooksSettings;

/// Whether a component attaches as a bus `Plugin` or a `Middleware`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    Plugin,
    Middleware,
}

/// Display + default-toggle metadata for one toggleable component.
#[derive(Debug, Clone, Copy)]
pub struct ComponentMeta {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub default_on: bool,
    pub kind: ComponentKind,
}

/// The single source of truth: every toggleable component.
///
/// Order here is the order they attach in [`apply_components`]. It mirrors the
/// historical CLI `build_agent` chain (Core → Shell → hygiene mw → Permission →
/// Compaction → Skills → Web → infra → collab) plus the components that were
/// not yet wired into the CLI (mcp / hooks / checkpoint / loader / interaction /
/// planning / utility / analytics / browser).
pub static COMPONENTS: &[ComponentMeta] = &[
    // ── core file + shell tools ──────────────────────────────────────────
    ComponentMeta {
        id: "core",
        label: "Core file tools",
        description: "Read / create / edit / list / find / grep — file inspect & mutation.",
        category: "tools",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "shell",
        label: "Shell",
        description: "execute_shell — run shell commands (build / test / scripts).",
        category: "tools",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    // ── hygiene middleware (keep the loop healthy) ───────────────────────
    ComponentMeta {
        id: "loop-detection",
        label: "Loop detection",
        description: "Aborts runaway repeated tool calls.",
        category: "safety",
        default_on: true,
        kind: ComponentKind::Middleware,
    },
    ComponentMeta {
        id: "dangling-tool-call",
        label: "Dangling tool-call guard",
        description: "Repairs orphaned tool calls so the loop does not wedge.",
        category: "safety",
        default_on: true,
        kind: ComponentKind::Middleware,
    },
    ComponentMeta {
        id: "tool-timeout",
        label: "Tool timeout",
        description: "Caps per-tool wall-clock time so a stuck tool cannot hang the agent.",
        category: "safety",
        default_on: true,
        kind: ComponentKind::Middleware,
    },
    // ── safety / long-session ────────────────────────────────────────────
    ComponentMeta {
        id: "permission",
        label: "Permission (HITL / plan)",
        description: "Human-in-the-loop approval + plan mode; publishes PermissionModeService.",
        category: "safety",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "compaction",
        label: "Compaction",
        description: "Compresses long-conversation context to stay within the window.",
        category: "context",
        default_on: true,
        kind: ComponentKind::Middleware,
    },
    // ── knowledge / retrieval ────────────────────────────────────────────
    ComponentMeta {
        id: "skills",
        label: "Skills",
        description: "Progressive skill loading (project + bundled skills tree).",
        category: "context",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "web",
        label: "Web",
        description: "internet_search + read_url.",
        category: "tools",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    // ── infra (no tools of their own, enable other features) ─────────────
    ComponentMeta {
        id: "provider-registry",
        label: "Provider registry",
        description: "Lets sub-agents/tasks target named providers via model spec. Skipped if no registry supplied.",
        category: "infra",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "tool-lock",
        label: "Tool-lock registry",
        description: "Coordinates exclusive tool locks across concurrent agents.",
        category: "infra",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    // ── collaboration / multi-agent ──────────────────────────────────────
    ComponentMeta {
        id: "task",
        label: "Tasks",
        description: "task_create / update / get / list / output / stop.",
        category: "collab",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "team",
        label: "Team",
        description: "team_create / team_delete / send_message — multi-agent coordination.",
        category: "collab",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "sub-agents",
        label: "Sub-agents",
        description: "`agent` tool — spawn child agents.",
        category: "collab",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    // ── extensibility / hooks / archiving ────────────────────────────────
    ComponentMeta {
        id: "hooks",
        label: "Hooks",
        description: "User lifecycle hooks (PreToolUse / PostToolUse / Session*). Adds no tools.",
        category: "ext",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "checkpoint",
        label: "Auto-checkpoint",
        description: "Auto-archives edited files via CheckpointMiddleware. Adds no tools.",
        category: "context",
        default_on: true,
        kind: ComponentKind::Middleware,
    },
    ComponentMeta {
        id: "interaction",
        label: "Interaction",
        description: "ask_human — solicit input mid-task.",
        category: "tools",
        default_on: true,
        kind: ComponentKind::Plugin,
    },
    // ── default-off (heavy / niche / accuracy-impacting) ─────────────────
    ComponentMeta {
        id: "mcp",
        label: "MCP",
        description: "Model Context Protocol servers — dynamically adds many tools; biggest accuracy impact.",
        category: "ext",
        default_on: false,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "subprocess-loader",
        label: "Subprocess extension loader",
        description: "Loads third-party AEP extensions running as subprocesses.",
        category: "ext",
        default_on: false,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "browser",
        label: "Browser",
        description: "Headless browser automation (chromiumoxide) — heavy dependency.",
        category: "tools",
        default_on: false,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "analytics",
        label: "Analytics",
        description: "Usage telemetry. Adds no tools.",
        category: "infra",
        default_on: false,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "planning",
        label: "Planning",
        description: "todo_write + mode signaling. May overlap with core/shell workflows.",
        category: "tools",
        default_on: false,
        kind: ComponentKind::Plugin,
    },
    ComponentMeta {
        id: "utility",
        label: "Utility",
        description: "Misc utility tools. May overlap with core/shell.",
        category: "tools",
        default_on: false,
        kind: ComponentKind::Plugin,
    },
];

/// `id -> enabled`, overriding [`ComponentMeta::default_on`]. Absent = default.
pub type ComponentToggles = HashMap<String, bool>;

/// Is `meta` enabled given `toggles`? Falls back to its `default_on`.
pub fn is_on(toggles: &ComponentToggles, meta: &ComponentMeta) -> bool {
    *toggles.get(meta.id).unwrap_or(&meta.default_on)
}

/// Harness-supplied inputs needed to construct parameterized components.
///
/// Parameterized components read their args from here. Components whose inputs
/// are absent (`provider_registry` / `skills` = `None`) gracefully skip with a
/// log line rather than panicking.
pub struct ComponentContext {
    /// Agent workspace root (informational / for callers building paths).
    pub workspace: PathBuf,
    /// Provider registry for `provider-registry`. `None` → that component skips.
    pub provider_registry: Option<Arc<ProviderRegistry>>,
    /// `(primary skills dir, optional bundled override)` for `skills`.
    /// `None` → `skills` skips.
    pub skills: Option<(PathBuf, Option<PathBuf>)>,
    /// MCP config file paths for `mcp`.
    pub mcp_config_paths: Vec<PathBuf>,
    /// Max spawn depth for `sub-agents`.
    pub subagent_depth: u32,
    /// User hook settings for `hooks`.
    pub hooks_settings: HooksSettings,
    /// Third-party AEP extension dirs for `subprocess-loader`.
    pub subprocess_ext_dirs: Vec<PathBuf>,
}

impl ComponentContext {
    /// A minimal context: no provider registry, no skills, empty path lists,
    /// `subagent_depth = 3`, default hook settings. Handy for tests and for
    /// callers that only want the tool-registering components.
    pub fn minimal(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            provider_registry: None,
            skills: None,
            mcp_config_paths: Vec::new(),
            subagent_depth: 3,
            hooks_settings: HooksSettings::default(),
            subprocess_ext_dirs: Vec::new(),
        }
    }
}

/// Attach every enabled component (per `toggles`) onto `b`.
///
/// This is the single assembly switchboard. It walks [`COMPONENTS`] in order
/// and, for each enabled row, builds the corresponding `Plugin`/`Middleware`
/// from `ctx`. Parameterized components with missing inputs skip with a log.
pub fn apply_components(
    mut b: BaseAgentBuilder,
    toggles: &ComponentToggles,
    ctx: &ComponentContext,
) -> BaseAgentBuilder {
    use crate::extension as ext;

    for meta in COMPONENTS {
        if !is_on(toggles, meta) {
            continue;
        }
        b = match meta.id {
            // ── Plugins ──────────────────────────────────────────────────
            "core" => b.plugin(Box::new(ext::CorePlugin)),
            "shell" => b.plugin(Box::new(ext::ShellPlugin)),
            "interaction" => b.plugin(Box::new(ext::InteractionPlugin)),
            "web" => b.plugin(Box::new(ext::WebPlugin)),
            "utility" => b.plugin(Box::new(ext::UtilityPlugin)),
            "planning" => b.plugin(Box::new(ext::PlanningPlugin)),
            "browser" => b.plugin(Box::new(ext::BrowserPlugin)),
            "task" => b.plugin(Box::new(ext::TaskPlugin::default())),
            "team" => b.plugin(Box::new(ext::TeamPlugin::default())),
            "analytics" => b.plugin(Box::new(ext::AnalyticsPlugin::new())),
            "permission" => b.plugin(Box::new(ext::PermissionPlugin::new())),
            "tool-lock" => b.plugin(Box::new(ext::ToolLockRegistryPlugin::new())),
            "sub-agents" => b.plugin(Box::new(ext::SubAgentPlugin::new(ctx.subagent_depth))),
            "hooks" => b.plugin(Box::new(ext::HooksPlugin::new(ctx.hooks_settings.clone()))),
            "mcp" => b.plugin(Box::new(ext::McpPlugin::new(ctx.mcp_config_paths.clone()))),
            "provider-registry" => match &ctx.provider_registry {
                Some(reg) => b.plugin(Box::new(ext::ProviderRegistryPlugin::new(reg.clone()))),
                None => {
                    tracing::info!(
                        component = "provider-registry",
                        "enabled but no ProviderRegistry in ComponentContext; skipping"
                    );
                    b
                }
            },
            "skills" => match &ctx.skills {
                Some((primary, bundled)) => b.plugin(Box::new(ext::SkillsPlugin::with_bundled(
                    primary.clone(),
                    bundled.clone(),
                ))),
                None => {
                    tracing::info!(
                        component = "skills",
                        "enabled but no skills paths in ComponentContext; skipping"
                    );
                    b
                }
            },
            "subprocess-loader" => b.plugin(Box::new(
                alva_app_extension_loader::loader::SubprocessLoaderPlugin::new(
                    ctx.subprocess_ext_dirs.clone(),
                ),
            )),
            // ── Middleware ───────────────────────────────────────────────
            "loop-detection" => b.middleware(Arc::new(
                alva_kernel_core::builtins::LoopDetectionMiddleware::new(),
            )),
            "dangling-tool-call" => b.middleware(Arc::new(
                alva_kernel_core::builtins::DanglingToolCallMiddleware::new(),
            )),
            "tool-timeout" => b.middleware(Arc::new(
                alva_kernel_core::builtins::ToolTimeoutMiddleware::default(),
            )),
            "compaction" => b.middleware(Arc::new(
                alva_host_native::middleware::CompactionMiddleware::default(),
            )),
            "checkpoint" => b.middleware(Arc::new(
                alva_host_native::middleware::CheckpointMiddleware::new(),
            )),
            other => {
                tracing::warn!(
                    component = other,
                    "unknown component id in COMPONENTS; skipping"
                );
                b
            }
        };
    }
    b
}

#[cfg(test)]
mod tests {
    //! Verifies the catalog → assembly path: default-on components register
    //! their representative tools, default-off ones do not, explicit toggles
    //! override, and a missing `provider_registry` skips gracefully (without
    //! disabling `sub-agents`).
    use super::*;
    use crate::base_agent::BaseAgentBuilder;
    use alva_agent_core::AgentAssemblySnapshot;
    use alva_test::mock_provider::MockLanguageModel;

    async fn build_with(toggles: ComponentToggles) -> Vec<String> {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = ComponentContext::minimal(tmp.path());
        let builder = apply_components(
            BaseAgentBuilder::new().workspace(tmp.path()),
            &toggles,
            &ctx,
        );
        let model = Arc::new(MockLanguageModel::new());
        let agent = builder.build(model).await.expect("agent builds");
        agent.tool_names()
    }

    async fn build_snapshot_with(toggles: ComponentToggles) -> AgentAssemblySnapshot {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = ComponentContext::minimal(tmp.path());
        let builder = apply_components(
            BaseAgentBuilder::new().workspace(tmp.path()),
            &toggles,
            &ctx,
        );
        let model = Arc::new(MockLanguageModel::new());
        let agent = builder.build(model).await.expect("agent builds");
        agent.assembly_snapshot()
    }

    /// Every catalog id is unique (the `match id` switchboard assumes it).
    #[test]
    fn component_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for m in COMPONENTS {
            assert!(seen.insert(m.id), "duplicate component id: {}", m.id);
        }
    }

    /// `is_on` honors explicit toggles, then falls back to `default_on`.
    #[test]
    fn is_on_uses_toggle_then_default() {
        let core = COMPONENTS.iter().find(|m| m.id == "core").unwrap();
        let browser = COMPONENTS.iter().find(|m| m.id == "browser").unwrap();
        let empty = ComponentToggles::new();
        assert!(is_on(&empty, core), "core default-on");
        assert!(!is_on(&empty, browser), "browser default-off");

        let mut t = ComponentToggles::new();
        t.insert("core".into(), false);
        t.insert("browser".into(), true);
        assert!(!is_on(&t, core), "core explicitly off");
        assert!(is_on(&t, browser), "browser explicitly on");
    }

    /// Empty toggles → all default-on components attach; default-off ones
    /// (browser/mcp) stay out.
    #[tokio::test]
    async fn defaults_register_expected_tools() {
        let names = build_with(ComponentToggles::new()).await;
        // Representative default-on tools.
        assert!(
            names.contains(&"execute_shell".to_string()),
            "shell on: {names:?}"
        );
        assert!(names.contains(&"read_url".to_string()), "web on: {names:?}");
        assert!(
            names.contains(&"task_create".to_string()),
            "task on: {names:?}"
        );
        assert!(
            names.contains(&"agent".to_string()),
            "sub-agents on: {names:?}"
        );
        // Default-off components must NOT contribute tools.
        assert!(
            !names.iter().any(|n| n.starts_with("browser_")),
            "browser default-off: {names:?}"
        );
    }

    /// Explicitly disabling a default-on component drops its tools.
    #[tokio::test]
    async fn explicit_off_drops_tool() {
        let mut t = ComponentToggles::new();
        t.insert("shell".into(), false);
        let names = build_with(t).await;
        assert!(
            !names.contains(&"execute_shell".to_string()),
            "shell explicitly off: {names:?}"
        );
        // Sibling default-on tool still present.
        assert!(
            names.contains(&"read_url".to_string()),
            "web still on: {names:?}"
        );
    }

    /// Explicitly enabling a default-off component attaches its tools.
    #[tokio::test]
    async fn explicit_on_adds_tool() {
        let mut t = ComponentToggles::new();
        t.insert("browser".into(), true);
        let names = build_with(t).await;
        assert!(
            names.iter().any(|n| n.starts_with("browser_")),
            "browser explicitly on: {names:?}"
        );
    }

    /// Component-driven assembly is visible in the agent snapshot, including
    /// direct middleware components that do not go through a Plugin wrapper.
    #[tokio::test]
    async fn snapshot_records_component_plugins_and_direct_middleware() {
        let mut t = ComponentToggles::new();
        t.insert("shell".into(), false);
        t.insert("loop-detection".into(), true);

        let snapshot = build_snapshot_with(t).await;
        assert!(
            snapshot.plugin_names.iter().any(|name| name == "core"),
            "default-on core component should be visible: {:?}",
            snapshot.plugin_names
        );
        assert!(
            !snapshot.plugin_names.iter().any(|name| name == "shell"),
            "explicitly disabled shell component should be absent: {:?}",
            snapshot.plugin_names
        );
        assert!(
            snapshot
                .direct_middleware_names
                .iter()
                .any(|name| name == "builtins_loop_detection"),
            "direct middleware component should be attributed separately: {:?}",
            snapshot.direct_middleware_names
        );
    }

    /// `provider_registry = None` → provider-registry skips (no panic), and
    /// sub-agents still registers its `agent` tool.
    #[tokio::test]
    async fn missing_provider_registry_skips_gracefully() {
        // minimal() already sets provider_registry = None; build all defaults.
        let names = build_with(ComponentToggles::new()).await;
        assert!(
            names.contains(&"agent".to_string()),
            "sub-agents still on: {names:?}"
        );
    }
}
