// INPUT:  alva_kernel_abi::context
// OUTPUT: re-exports of all context value types
// POS:    Re-exports value types from alva_kernel_abi::context so that downstream code using alva_agent_context::types::* continues to work.
//! Context management types — re-exported from `alva_kernel_abi::context`.

pub use alva_kernel_abi::context::{
    BudgetInfo,
    CompressAction,
    ContextEntry,
    ContextLayer,
    ContextMetadata,
    ContextSnapshot,
    EntryOrigin,
    EntrySnapshot,
    EventMatch,
    EventQuery,
    Injection,
    InjectionContent,
    IngestAction,
    LayerStats,
    MemoryCategory,
    MemoryFact,
    MessageRange,
    MessageSelector,
    Priority,
    PromptSection,
    RuntimeContext,
    SessionEvent,
    SessionMessage,
    ToolPattern,
};
