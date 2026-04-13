// POS: Bus events — emitted by context management for observability and coordination.

/// Emitted when token usage exceeds the configured budget threshold.
///
/// Sender: ContextHooks (on_budget_exceeded)
/// Receiver: UI layer, compression middleware, metrics
#[derive(Clone, Debug)]
pub struct TokenBudgetExceeded {
    pub agent_id: String,
    pub usage_ratio: f32,
    pub used_tokens: usize,
    pub budget_tokens: usize,
}
impl alva_kernel_bus::BusEvent for TokenBudgetExceeded {}

/// Emitted after context compression is applied.
///
/// Sender: ContextHooks (assemble/on_budget_exceeded)
/// Receiver: UI layer, metrics
#[derive(Clone, Debug)]
pub struct ContextCompacted {
    pub agent_id: String,
    pub strategy: String,
    pub tokens_before: usize,
    pub tokens_after: usize,
}
impl alva_kernel_bus::BusEvent for ContextCompacted {}

/// Emitted after memory facts are extracted from conversation.
///
/// Sender: DefaultContextHooks (after_turn)
/// Receiver: UI layer, metrics
#[derive(Clone, Debug)]
pub struct MemoryExtracted {
    pub agent_id: String,
    pub fact_count: usize,
}
impl alva_kernel_bus::BusEvent for MemoryExtracted {}
