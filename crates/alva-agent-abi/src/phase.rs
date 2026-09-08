use serde::{Deserialize, Serialize};

/// Stable runtime timeline points that plugins and observers may target.
///
/// This is kernel vocabulary: the kernel runtime owns when these points
/// occur and in what order. Higher layers may register contributions to
/// these phases, but they should not define a separate timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    RunStart,
    InputCommitted,
    IterationStart,
    PrepareLlmRequest,
    LlmCallStart,
    BeforeLlmCall,
    AroundLlmCall,
    AfterLlmCall,
    LlmCallEnd,
    AssistantCommitted,
    ToolBatchDeclared,
    ToolBatchPlanned,
    ToolUseDeclared,
    BeforeToolCall,
    AroundToolCall,
    AfterToolCall,
    ToolResultCommitted,
    ToolBatchEnd,
    AfterTurn,
    IterationEnd,
    RunEnd,
}

/// The kind of effect a contribution is allowed to have at a phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseEffect {
    /// Read-only observation. Failures should not affect the run.
    Observe,
    /// Mutate the phase payload.
    Mutate,
    /// Wrap a call boundary such as LLM or tool execution.
    Wrap,
    /// Decide whether a phase operation may proceed.
    Decide,
}
