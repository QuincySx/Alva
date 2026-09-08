// INPUT:  alva_kernel_abi (Task / Scope / Spawn / Phase / Analytics vocabulary)
// OUTPUT: Agent Contract —— TaskState, Phase, AnalyticsSink, SpawnCommunication, ScopeId, …
// POS:    Agent 契约层（ticket 12，expand 阶段）。这些类型描述"我们这一类
//         agent 产品长什么样"——任务、作用域、派生、阶段、埋点——而不是
//         "循环怎么转"。第三方拿内核去搭完全不同形态的 agent 时可以整层
//         不用。位于内核契约（alva-kernel-abi）之上、SDK 装配
//         （alva-agent-core）之下；只依赖内核契约层，不依赖任何装配层或
//         产品层。
//
//         层位说明：
//         - `scope::context`（ContextHooks / ContextSystem）是内核循环的
//           装配词汇，留在 alva-kernel-abi，不在本层。
//         - `session_record`（SessionRecord / ExecutionRecord）自
//           ticket 06 起是对话记录（ConversationLog::append_records）的
//           内核词汇，同样留在 alva-kernel-abi。
//
//         expand/contract 节奏：本票先把 agent 产品概念的公开路径立起来
//         （内核契约层旧路径在 ticket 16 前保持有效）；13/14 批迁移调用
//         点，16 收口后内核契约层不再承载这些概念。

pub mod analytics {
    pub use alva_kernel_abi::analytics::*;
}

pub mod phase {
    pub use alva_kernel_abi::phase::*;
}

pub mod task {
    pub use alva_kernel_abi::task::*;
}

pub mod scope {
    /// 派生期通信：给子 agent 挂载通信能力（blackboard / handoff）的插件契约。
    pub mod spawn {
        pub use alva_kernel_abi::scope::spawn::*;
    }

    pub use alva_kernel_abi::scope::{ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot};
}

pub use analytics::{AnalyticsEvent, AnalyticsSink, NoopAnalyticsSink};
pub use phase::{Phase, PhaseEffect};
pub use scope::spawn::{
    OnChildComplete, SpawnCommContext, SpawnCommError, SpawnCommHandle, SpawnCommunication,
    SpawnCommunicationRegistry, SpawnResult,
};
pub use scope::{ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot};
pub use task::{create_task_state, generate_task_id, TaskState, TaskStatus, TaskType};
