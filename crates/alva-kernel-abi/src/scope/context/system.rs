// POS: ContextSystem container — bundles context hooks, handle, and the
// optional apply executor (ticket 10: 注入/压缩的执行在上下文能力库，内核只问时机).

use std::sync::Arc;

use super::{ContextHandle, ContextHooks};

/// 注入与压缩动作的**执行器**。内核在正确的时机询问上下文系统（ingest /
/// assemble / on_budget_exceeded），但对询问结果的落地（注入怎么排、压缩
/// 怎么改写消息列表）不再是内核的职责——执行器由上下文能力库提供并挂到
/// `ContextSystem` 上。`None` 表示该上下文系统自带执行语义，内核只透传。
pub trait ContextApply: Send + Sync {
    /// Apply hook-produced injections to the layered system prompt and the
    /// working message list.
    fn apply_injections(
        &self,
        injections: Vec<super::Injection>,
        system_prompt: &mut Vec<String>,
        messages: &mut Vec<crate::AgentMessage>,
    );

    /// Apply budget-exceeded compression actions to the working message list.
    fn apply_compressions<'a>(
        &'a self,
        actions: Vec<super::CompressAction>,
        messages: &'a mut Vec<crate::AgentMessage>,
        handle: &'a dyn ContextHandle,
        agent_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;
}

/// Bundles context hooks, handle, and an optional apply executor. Durable
/// state is wired through the authoritative AgentSession owned by AgentState.
pub struct ContextSystem {
    hooks: Arc<dyn ContextHooks>,
    handle: Arc<dyn ContextHandle>,
    apply: Option<Arc<dyn ContextApply>>,
}

impl ContextSystem {
    pub fn new(hooks: Arc<dyn ContextHooks>, handle: Arc<dyn ContextHandle>) -> Self {
        Self {
            hooks,
            handle,
            apply: None,
        }
    }

    /// Attach the apply executor (the context capability library does this).
    pub fn with_apply(mut self, apply: Arc<dyn ContextApply>) -> Self {
        self.apply = Some(apply);
        self
    }

    pub fn hooks(&self) -> &dyn ContextHooks {
        self.hooks.as_ref()
    }

    pub fn handle(&self) -> &dyn ContextHandle {
        self.handle.as_ref()
    }

    pub fn apply(&self) -> Option<&dyn ContextApply> {
        self.apply.as_deref()
    }

    /// Replace hooks (e.g., swap in a ContextHooksChain).
    pub fn set_hooks(&mut self, hooks: Arc<dyn ContextHooks>) {
        self.hooks = hooks;
    }
}
