# alva-agent-abi
> Agent 契约层：Task / Scope+Spawn / Phase / Analytics / Provider 注册表——"我们这一类 agent 产品"的概念词汇。

## 地位
位于 `alva-kernel-abi`（内核契约）之上、`alva-agent-core`（SDK 装配）之下。只依赖内核契约层与 L0 总线词汇；被 SDK 装配层与产品层消费。内核（kernel-abi / kernel-core）不得依赖本层——CI Rule 19 焊死（ADR-0008）。

## 逻辑
1. `task.rs`：TaskType / TaskStatus / TaskState 与 id 工厂——后台/异步任务模型。
2. `phase.rs`：Phase / PhaseEffect——插件可瞄准的稳定运行期时间线词汇。
3. `analytics.rs`：AnalyticsEvent / AnalyticsSink（bus 能力）——遥测事件通道；具体 sink 在上层（app-core JsonlSink），采集在中间件（R02）。
4. `scope/`：ScopeId / ScopeSnapshot / ChildScopeConfig / ScopeError 与 spawn 期通信契约（SpawnCommunication / SpawnCommContext / SpawnCommunicationRegistry / OnChildComplete）——子 agent 挂载通信能力的插件词汇。
5. `provider/`：Provider trait / ProviderRegistry / ProviderError / CredentialSource——provider+model 二元解析的注册表（实现见 alva-llm-provider）。
6. `token_estimation.rs`：TokenEstimator / SimpleTokenEstimator——token 估算词汇。

## 层位说明（不属本层）
- `scope::context`（ContextHooks / ContextSystem）与 `session_record`（SessionRecord / ConversationLog 词汇）是内核循环词汇，留在 alva-kernel-abi。

## 约束
- 不得依赖 alva-agent-core、alva-app-*、alva-host-* 或任何具体实现 crate。
- 公开表面变化必须随 commit 更新 `scripts/public-surface/alva-agent-abi.txt` 快照。
