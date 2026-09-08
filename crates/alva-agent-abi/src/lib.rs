// INPUT:  alva_kernel_bus (BusHandle), alva_macros (bus_cap), alva_kernel_abi (ContextHooks, LanguageModel, scope::context)
// OUTPUT: Agent Contract —— TaskState, Phase, AnalyticsSink, ProviderRegistry, SpawnCommunication, ScopeId, …
// POS:    Agent 契约层。这些类型描述"我们这一类 agent 产品长什么样"——任务、
//         作用域、派生、阶段、埋点、provider 注册——而不是"循环怎么转"。
//         第三方拿内核去搭完全不同形态的 agent 时可以整层不用。位于内核契约
//         （alva-kernel-abi）之上、SDK 装配（alva-agent-core）之下；只依赖
//         内核契约层与 L0 总线词汇，不依赖任何装配层或产品层。
//
//         层位说明：
//         - `scope::context`（ContextHooks / ContextSystem）是内核循环的
//           装配词汇，留在 alva-kernel-abi，不在本层。
//         - `session_record`（SessionRecord / ExecutionRecord）自
//           ticket 06 起是对话记录（ConversationLog::append_records）的
//           内核词汇，同样留在 alva-kernel-abi。

pub mod analytics;
pub mod phase;
pub mod provider;
pub mod scope;
pub mod task;
pub mod token_estimation;

pub use analytics::{AnalyticsEvent, AnalyticsSink, NoopAnalyticsSink};
pub use phase::{Phase, PhaseEffect};
pub use provider::{CredentialSource, Provider, ProviderError, ProviderRegistry, StaticCredential};
pub use scope::spawn::{
    OnChildComplete, SpawnCommContext, SpawnCommError, SpawnCommHandle, SpawnCommunication,
    SpawnCommunicationRegistry, SpawnResult,
};
pub use scope::{ChildScopeConfig, ScopeError, ScopeId, ScopeSnapshot};
pub use task::{create_task_state, generate_task_id, TaskState, TaskStatus, TaskType};
pub use token_estimation::{SimpleTokenEstimator, TokenEstimator};
