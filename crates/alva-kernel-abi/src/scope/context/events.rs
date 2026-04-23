// POS: Bus events — emitted by context management for observability and coordination.

/// Bus Event: token usage exceeded the configured budget threshold.
///
/// **Emitter**: `CompactionMiddleware::before_llm_call`
/// (`alva-agent-context/src/middleware.rs`) and any future
/// `ContextHooks::on_budget_exceeded` implementation.
/// **Subscribers**: UI layer (redraw token gauge), metrics pipeline,
/// policy middleware that wants to react (e.g. preempt the next turn).
/// **Semantic**: observational — signal only, no required action.
/// Downstream compaction is orchestrated by the same middleware and
/// does not wait for subscribers.
#[crate::bus_event]
#[derive(Clone, Debug)]
pub struct TokenBudgetExceeded {
    pub agent_id: String,
    pub usage_ratio: f32,
    pub used_tokens: usize,
    pub budget_tokens: usize,
}
impl alva_kernel_bus::BusEvent for TokenBudgetExceeded {}

/// Bus Event: context compaction finished — one emission per compaction.
///
/// **Emitter**: `CompactionMiddleware::before_llm_call`
/// (`alva-agent-context/src/middleware.rs`) after running its
/// compaction strategy.
/// **Subscribers**: UI layer (redraw token usage, show "compacted"
/// indicator), metrics pipeline.
/// **Semantic**: informational — conversation context has shrunk.
/// Subscribers should refresh any cached token totals.
#[crate::bus_event]
#[derive(Clone, Debug)]
pub struct ContextCompacted {
    pub agent_id: String,
    pub strategy: String,
    pub tokens_before: usize,
    pub tokens_after: usize,
}
impl alva_kernel_bus::BusEvent for ContextCompacted {}

/// Bus Event: memory facts extracted from a conversation turn.
///
/// **Emitter**: reserved for future use by a memory-extraction pipeline
/// (likely `DefaultContextHooks::after_turn` or a dedicated memory
/// middleware). No production emitter today.
/// **Subscribers**: UI layer (show "N facts saved" toast), metrics.
/// **Semantic**: informational — N memory facts just landed in the
/// memory backend. Subscribers may re-query memory to reflect the new
/// state.
#[crate::bus_event]
#[derive(Clone, Debug)]
pub struct MemoryExtracted {
    pub agent_id: String,
    pub fact_count: usize,
}
impl alva_kernel_bus::BusEvent for MemoryExtracted {}
