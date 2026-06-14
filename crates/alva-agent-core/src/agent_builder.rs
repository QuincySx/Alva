//! AgentBuilder — SDK-level builder that assembles an `Agent` from
//! extensions, tools, middleware, model, and kernel config.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession};
use alva_kernel_abi::{
    AgentError, Bus, BusHandle, BusWriter, LanguageModel, ModelConfig, Tool, ToolRegistry,
};
use alva_kernel_core::middleware::{Middleware, MiddlewareContext, MiddlewareStack};
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::extension::{
    Extension, ExtensionAsPlugin, ExtensionBridgeMiddleware, ExtensionHost, LateContext, Plugin,
    Registrar,
};

/// SDK-level builder for assembling an `Agent`.
///
/// This is the layer at which `alva-agent-core` assembles an agent without
/// any harness-level opinions. Callers (third-party harnesses or tests)
/// compose their own model, extensions, and middleware here. Opinionated
/// wrappers like `alva_app_core::BaseAgentBuilder` delegate to this.
pub struct AgentBuilder {
    model: Option<Arc<dyn LanguageModel>>,
    system_prompt: String,
    workspace: Option<PathBuf>,
    model_config: ModelConfig,
    max_iterations: u32,
    context_window: usize,

    plugins: Vec<Box<dyn Plugin>>,
    extra_tools: Vec<Box<dyn Tool>>,
    extra_middleware: Vec<Arc<dyn Middleware>>,

    bus: Option<BusHandle>,
    bus_writer: Option<BusWriter>,
    session: Option<Arc<dyn AgentSession>>,

    context_system: Option<Arc<alva_kernel_abi::scope::context::ContextSystem>>,
    context_token_budget: Option<usize>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model: None,
            system_prompt: String::new(),
            workspace: None,
            model_config: ModelConfig::default(),
            max_iterations: 100,
            context_window: 0,
            plugins: Vec::new(),
            extra_tools: Vec::new(),
            extra_middleware: Vec::new(),
            bus: None,
            bus_writer: None,
            session: None,
            context_system: None,
            context_token_budget: None,
        }
    }

    pub fn model(mut self, m: Arc<dyn LanguageModel>) -> Self {
        self.model = Some(m);
        self
    }
    pub fn system_prompt(mut self, s: impl Into<String>) -> Self {
        self.system_prompt = s.into();
        self
    }
    pub fn workspace(mut self, p: impl Into<PathBuf>) -> Self {
        self.workspace = Some(p.into());
        self
    }
    pub fn model_config(mut self, cfg: ModelConfig) -> Self {
        self.model_config = cfg;
        self
    }
    pub fn max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n;
        self
    }
    pub fn context_window(mut self, n: usize) -> Self {
        self.context_window = n;
        self
    }
    pub fn extension(mut self, e: Box<dyn Extension>) -> Self {
        self.plugins.push(Box::new(ExtensionAsPlugin(e)));
        self
    }
    pub fn plugin(mut self, p: Box<dyn Plugin>) -> Self {
        self.plugins.push(p);
        self
    }
    pub fn tool(mut self, t: Box<dyn Tool>) -> Self {
        self.extra_tools.push(t);
        self
    }
    pub fn middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.extra_middleware.push(mw);
        self
    }
    pub fn with_bus(mut self, bus: BusHandle) -> Self {
        self.bus = Some(bus);
        self
    }
    pub fn with_bus_writer(mut self, bw: BusWriter) -> Self {
        self.bus = Some(bw.handle());
        self.bus_writer = Some(bw);
        self
    }
    pub fn session(mut self, s: Arc<dyn AgentSession>) -> Self {
        self.session = Some(s);
        self
    }
    pub fn with_context_system(
        mut self,
        cs: Arc<alva_kernel_abi::scope::context::ContextSystem>,
    ) -> Self {
        self.context_system = Some(cs);
        self
    }
    pub fn with_context_token_budget(mut self, budget: usize) -> Self {
        self.context_token_budget = Some(budget);
        self
    }

    /// 构建 Agent。运行 Plugin 生命周期（`register` → `finalize`），装配
    /// middleware，产出可运行的 `Agent`。旧的 `Extension` 实现经
    /// `ExtensionAsPlugin` 适配器桥接。
    pub async fn build(self) -> Result<Agent, AgentError> {
        // 1. Validate required inputs.
        let model = self
            .model
            .ok_or_else(|| AgentError::Other("AgentBuilder requires a model".into()))?;

        // 2. Set up the bus. Prefer a caller-supplied writer (so the caller
        //    can register capabilities on it). If only a `BusHandle` was
        //    provided we use that as the routing handle but still spin a
        //    fresh writer for the contexts — capability `provide()` calls
        //    made on that writer will not be visible on the caller's bus,
        //    which is a documented caveat of the handle-only path.
        //    Otherwise create a fresh in-process Bus.
        let (bus, bus_writer): (BusHandle, BusWriter) = if let Some(writer) = self.bus_writer {
            (writer.handle(), writer)
        } else if let Some(handle) = self.bus {
            let fresh = Bus::new();
            (handle, fresh.writer())
        } else {
            let fresh = Bus::new();
            (fresh.handle(), fresh.writer())
        };

        // 3. Create the ExtensionHost.
        let host = Arc::new(RwLock::new(ExtensionHost::new()));

        // 4. Register phase: 每个 plugin 一次性注册 tools/middleware/bus/prompt/command。
        //    `LateContext.workspace` (and the per-plugin Registrar) is
        //    non-Option, so when the caller didn't set one we default to
        //    `PathBuf::new()` (i.e. the empty path). Plugins that need a real
        //    workspace must check.
        let workspace_for_ctx = self.workspace.clone().unwrap_or_default();
        let mut all_tools: Vec<Box<dyn Tool>> = Vec::new();
        for p in &self.plugins {
            let reg = Registrar::new(
                host.clone(),
                p.name().to_string(),
                bus.clone(),
                bus_writer.clone(),
                workspace_for_ctx.clone(),
            );
            p.register(&reg).await;
            all_tools.extend(reg.take_tools());
        }
        all_tools.extend(self.extra_tools);

        // 5. Build the middleware stack: middlewares the plugins registered
        //    during register(), then user-supplied extras, then the bridge
        //    that routes lifecycle events into the ExtensionHost.
        let mut middleware_stack = MiddlewareStack::new();
        {
            let mut host_mut = host.write().unwrap();
            for mw in host_mut.take_middlewares() {
                middleware_stack.push_sorted(mw);
            }
        }
        for mw in self.extra_middleware {
            middleware_stack.push_sorted(mw);
        }
        // 过渡期保留 bridge：待所有 Extension 迁移到 Plugin 后移除（见注入机制统一迁移计划 Phase 4）。
        middleware_stack.push_sorted(Arc::new(ExtensionBridgeMiddleware::new(host.clone())));

        // 6. Register the collected tools into a ToolRegistry.
        let mut registry = ToolRegistry::new();
        for tool in all_tools {
            registry.register(tool);
        }

        // 7. Configure middleware that needs shared infrastructure.
        middleware_stack.configure_all(&MiddlewareContext {
            bus: Some(bus.clone()),
            workspace: self.workspace.clone(),
            session: None, // TODO(phase-2): per-middleware scoped session
        });

        // 8. Finalize phase: plugins can return additional tools that depend
        //    on seeing the final tool list. We hand them an Arc snapshot of
        //    the registry; any returned tools are folded back through the
        //    registry so name collisions dedupe the same way as the register
        //    path (last write wins).
        let late_ctx = LateContext {
            bus: bus.clone(),
            bus_writer: bus_writer.clone(),
            workspace: workspace_for_ctx,
            model: model.clone(),
            tools: registry.list_arc(),
            max_iterations: self.max_iterations,
        };
        for p in &self.plugins {
            for tool in p.finalize(&late_ctx).await {
                registry.register_arc(tool);
            }
        }
        let tools_arc: Vec<Arc<dyn Tool>> = registry.list_arc();

        // 9. Assemble the final system prompt:
        //      [user-provided base]
        //      [extension contributions, in registration order]
        //      [Environment block — kernel-managed invariant: cwd + date]
        //
        //      The Environment block is the agent-core's hard contract: a
        //      runnable agent should always know its workspace and the
        //      current date, regardless of what extensions contribute.
        //      Matches pi-mono's `buildSystemPrompt` floor (core always
        //      appends cwd + date even when the user supplies a custom
        //      prompt). Placed last so it wins on short-term recency.
        let system_prompt = assemble_system_prompt(
            self.system_prompt,
            {
                let mut host_mut = host.write().unwrap();
                host_mut.take_system_prompt_additions()
            },
            self.workspace.as_deref(),
        );

        // 10. Session — default to in-memory if not provided.
        let session: Arc<dyn AgentSession> = self
            .session
            .unwrap_or_else(|| Arc::new(InMemoryAgentSession::new()));

        // 11. AgentState
        let state = AgentState {
            model,
            tools: tools_arc,
            session,
            extensions: Extensions::new(),
        };

        // 12. AgentConfig
        let config = AgentConfig {
            middleware: middleware_stack,
            system_prompt,
            max_iterations: self.max_iterations,
            model_config: self.model_config,
            context_window: self.context_window,
            workspace: self.workspace,
            bus: Some(bus.clone()),
            context_system: self.context_system,
            context_token_budget: self.context_token_budget,
        };

        // 13. Wrap up. The bus_writer is intentionally dropped here — any
        //     capabilities the caller / extensions wanted to register on it
        //     have already been published, and the run loop only needs the
        //     read-side `BusHandle`.
        drop(bus_writer);

        // Snapshot tools for cheap external inspection (BaseAgent, tests).
        let tools_snapshot = {
            let st = state.tools.clone();
            st
        };

        Ok(Agent {
            state: Mutex::new(state),
            config: tokio::sync::RwLock::new(config),
            bus,
            host,
            tools: tools_snapshot,
        })
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Assemble the final system prompt as a layered `Vec<String>` so
/// providers that support prompt caching can split cleanly between
/// stable (cacheable) and dynamic (per-turn) content.
///
/// Output convention:
///   - `result[0]` = stable bucket: base prompt + every extension
///     contribution at layer L0 (`AlwaysPresent`) / L1 (`OnDemand`) /
///     L3 (`Memory`). Cacheable by Anthropic's `cache_control:
///     ephemeral`.
///   - `result[1]` = dynamic bucket: every extension contribution at
///     layer L2 (`RuntimeInject`) plus the kernel-managed Environment
///     block (cwd + today's date). Always rebuilt per turn.
///
/// If there are no dynamic contributions and no Environment block is
/// emitted, returns a single-element vec (entire prompt stable). If
/// there's no stable content either, returns an empty vec.
fn assemble_system_prompt(
    base: String,
    additions: Vec<(
        String,
        alva_kernel_abi::scope::context::ContextLayer,
        String,
    )>,
    workspace: Option<&std::path::Path>,
) -> Vec<String> {
    use alva_kernel_abi::scope::context::ContextLayer;

    let mut stable_parts: Vec<String> = Vec::new();
    let mut dynamic_parts: Vec<String> = Vec::new();

    let trimmed_base = base.trim();
    if !trimmed_base.is_empty() {
        stable_parts.push(trimmed_base.to_string());
    }
    for (_ext, layer, text) in additions {
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        match layer {
            ContextLayer::RuntimeInject => dynamic_parts.push(t.to_string()),
            // AlwaysPresent / OnDemand / Memory all stable.
            _ => stable_parts.push(t.to_string()),
        }
    }
    // Environment block is per-turn-volatile (today's date) — always
    // dynamic bucket.
    dynamic_parts.push(build_environment_block(workspace));

    let stable_joined = stable_parts.join("\n\n");
    let dynamic_joined = dynamic_parts.join("\n\n");

    let mut out: Vec<String> = Vec::new();
    if !stable_joined.is_empty() {
        out.push(stable_joined);
    }
    if !dynamic_joined.is_empty() {
        out.push(dynamic_joined);
    }
    out
}

/// Build the canonical "# Environment" block. Always includes the date;
/// includes `Working directory` only when a workspace path was set.
fn build_environment_block(workspace: Option<&std::path::Path>) -> String {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut lines: Vec<String> = Vec::with_capacity(3);
    lines.push("# Environment".to_string());
    lines.push(format!("Today's date: {}", date));
    if let Some(ws) = workspace {
        let ws_str = ws.display().to_string();
        if !ws_str.is_empty() {
            lines.push(format!("Working directory: {}", ws_str));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_block_without_workspace_has_date_only() {
        let block = build_environment_block(None);
        assert!(block.starts_with("# Environment"));
        assert!(block.contains("Today's date:"));
        assert!(!block.contains("Working directory"));
    }

    #[test]
    fn environment_block_with_workspace_includes_cwd() {
        let ws = std::path::Path::new("/tmp/some/ws");
        let block = build_environment_block(Some(ws));
        assert!(block.contains("Working directory: /tmp/some/ws"));
        assert!(block.contains("Today's date:"));
    }

    #[test]
    fn assemble_empty_base_still_emits_environment() {
        let out = assemble_system_prompt(String::new(), vec![], None);
        // No stable content → only dynamic segment (Environment).
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("# Environment"));
    }

    #[test]
    fn assemble_groups_stable_and_dynamic_correctly() {
        use alva_kernel_abi::scope::context::ContextLayer;
        let base = "You are Alva.".to_string();
        let adds = vec![
            (
                "ext_a".to_string(),
                ContextLayer::AlwaysPresent,
                "stable-context".to_string(),
            ),
            (
                "ext_b".to_string(),
                ContextLayer::RuntimeInject,
                "volatile-context".to_string(),
            ),
        ];
        let out = assemble_system_prompt(
            base,
            adds,
            Some(std::path::Path::new("/ws")),
        );
        // [stable bucket, dynamic bucket]
        assert_eq!(out.len(), 2);
        // Stable: base + ext_a
        assert!(out[0].contains("You are Alva."));
        assert!(out[0].contains("stable-context"));
        assert!(!out[0].contains("volatile-context"));
        assert!(!out[0].contains("# Environment"));
        // Dynamic: ext_b + Environment
        assert!(out[1].contains("volatile-context"));
        assert!(out[1].contains("# Environment"));
        assert!(out[1].contains("Working directory: /ws"));
    }

    #[test]
    fn assemble_skips_empty_additions() {
        use alva_kernel_abi::scope::context::ContextLayer;
        let out = assemble_system_prompt(
            "base".to_string(),
            vec![
                ("x".to_string(), ContextLayer::AlwaysPresent, "   ".to_string()),
                ("y".to_string(), ContextLayer::AlwaysPresent, "real".to_string()),
            ],
            None,
        );
        // Joined into stable+dynamic segments — peek at the whole thing.
        let joined = out.join("\n\n");
        assert!(joined.contains("base"));
        assert!(joined.contains("real"));
        assert!(joined.contains("# Environment"));
    }
}
