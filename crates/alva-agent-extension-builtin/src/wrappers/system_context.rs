//! Default `SystemContextExtension` — injects workspace context files
//! (CLAUDE.md / AGENTS.md) and git status into the system prompt.
//!
//! Mirrors pi-mono's `buildSystemPrompt` project-context section and
//! AMP's AGENTS.md guidance block: every turn should see the workspace
//! rules and the current git state so the model can reason about them.
//!
//! The agent-core already appends a canonical `# Environment` block
//! (date + workspace path) as a hard floor; this extension layers
//! richer content on top via `HostAPI::append_system_prompt`.
//!
//! Replace by registering another `Extension` with
//! `name() == "system_context"` — the builder's name-based dedup will
//! skip this default.
//!
//! Native-only: gated out on wasm32 because the underlying context
//! collectors shell out to `git` and read the filesystem.

use std::sync::{Arc, RwLock};

use alva_agent_context::system_context::{get_system_context, get_user_context};
use alva_agent_core::extension::{Extension, ExtensionContext, HostAPI};
use alva_kernel_abi::scope::context::ContextLayer;
use async_trait::async_trait;

/// Injects workspace-level context (CLAUDE.md / AGENTS.md, git status)
/// into the system prompt during the configure phase.
pub struct SystemContextExtension {
    /// Captured during `activate()` so `configure()` can call
    /// `append_system_prompt` without re-threading the API.
    api: Arc<RwLock<Option<HostAPI>>>,
}

impl SystemContextExtension {
    pub fn new() -> Self {
        Self {
            api: Arc::new(RwLock::new(None)),
        }
    }
}

impl Default for SystemContextExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for SystemContextExtension {
    fn name(&self) -> &str {
        "system_context"
    }

    fn description(&self) -> &str {
        "Workspace context (CLAUDE.md/AGENTS.md + git status) for system prompt"
    }

    fn activate(&self, api: &HostAPI) {
        *self.api.write().unwrap() = Some(api.clone());
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        // An empty workspace path means the builder had no workspace set.
        // Nothing meaningful to gather — leave the prompt alone.
        if ctx.workspace.as_os_str().is_empty() {
            return;
        }
        let api = match self.api.read().unwrap().clone() {
            Some(api) => api,
            None => {
                tracing::warn!(
                    "SystemContextExtension.configure called without activate; skipping"
                );
                return;
            }
        };

        let user_ctx = get_user_context(&ctx.workspace).await;
        let sys_ctx = get_system_context(&ctx.workspace).await;

        // Split contributions by stability — CLAUDE.md is stable
        // (rarely edited), git status is volatile (changes per
        // commit). Routing each to the right layer lets the prompt
        // cache cover the stable bulk while only the git-status
        // segment becomes a per-turn cache miss.
        if let Some(md) = user_ctx.get("claudeMd") {
            let trimmed = md.trim();
            if !trimmed.is_empty() {
                api.append_system_prompt(
                    ContextLayer::AlwaysPresent,
                    format!("<project_context>\n{}\n</project_context>", trimmed),
                );
            }
        }
        if let Some(status) = sys_ctx.get("gitStatus") {
            let trimmed = status.trim();
            if !trimmed.is_empty() {
                api.append_system_prompt(
                    ContextLayer::RuntimeInject,
                    format!("<git_status>\n{}\n</git_status>", trimmed),
                );
            }
        }
    }
}

// (Layer-routing test moved into alva-agent-core's
// `assemble_system_prompt` unit tests since that's where the layer
// semantics actually live.)
