# L2.3 · sdk/agent/abi
> 通用内核之外的 Agent 产品词汇和服务契约。

## 地位
L2.3；允许依赖 kernel ABI，内核不得反向依赖本层。

## 逻辑
Task 显式 pending→running→terminal 转移，保留七种kind、随机前缀ID、真实开始/结束/暂停元数据和单调output cursor；Scope/Spawn、稳定Phase timeline、Command metadata 与 ProviderRegistry
是 Agent 产品契约，不是“循环怎么转”的必要条件。
ProviderRegistry是同步安全的真实工厂注册表，同provider ID显式替换（不改变Plugin/tool严格重名契约），缺失模型/空factory结果（含typed-nil function）有typed ProviderError。
CredentialSource仅显式获取；StaticCredential不读env并拒绝fmt/JSON泄露。Analytics是typed可选非阻塞sink，不是durable审计。

## 约束
本层拥有纯值/契约和纯注册机制；Phase只作时间线词汇，不带Effect或执行注册协议；实际scope树/黑板在context/scope、typed middleware装配在sdk/agent/core、telemetry收集在analytics。
具体provider HTTP、JSONL/OTLP sink、后台Task执行器不在此层；不把typed metadata声明冒充对应native运行期已迁移。
SimpleTokenEstimator覆盖工具参数/非可见content，但仍是尺寸启发式，不是provider tokenizer或真实媒体token计费。
SpawnCommunication是显式opt-in attach/hooks/on-complete；有界Registry按kind严格重名检查，拒绝typed-nil，metadata只在锁外读取并固定本次kind，不默认继承父会话读取权。ChildCompleteFunc的nil值调用返回错误。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Agent vocabulary | `contracts.go` | Scope/Spawn/Phase/Command |
| Tasks | `task.go` | kinds、随机identity、状态时间、暂停与output cursor |
| Provider factories | `provider.go` | registry、typed失败与显式credential source |
| Analytics contract | `analytics.go` | typed event union、deep clone、非阻塞sink契约 |
| Token estimates | `tokens.go` | 文本/CJK和完整显式wire content启发式 |
| Regression | `contracts_test.go` | 非法跳跃与终态复活拒绝 |
| Capability regressions | `capabilities_test.go` | 真实factory替换、credential防泄露、Task状态与完整token估计 |
| Spawn communication | `spawn.go` | ChildScopeConfig/Snapshot、attach/完成callback、有界typed registry |
| Spawn regressions | `spawn_test.go` | typed-nil拒绝、nil完成callback报错、metadata锁外单次绑定 |
