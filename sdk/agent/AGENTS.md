# L2.3–L3 · Go Agent SDK
> 内核之上的 Agent 产品词汇和纯 SDK 装配。

## 地位

不包含默认 harness 决策；第三方可直接使用 `core.Builder`。

## 逻辑

`abi` 承担 Agent 语义，`core` 负责 Plugin 注册/晚期接线与运行；kernel 不反向依赖本层。

## 约束

不引入 harness/platform/apps/integration 或具体 Provider 实现，完整 Task/Scope/Spawn 按真实消费者逐项移植，不造空壳。
新 API 不提供旧 Rust 名称的历史别名或兼容格式。
core装配在回调前捕获配方、失败/取消/panic封闭贡献；配置描述符独立于调用方，服务实现仍按显式所有权共享。ABI及真实消费者共同拒绝缺失可执行能力，不增加统一服务总包或新生命周期。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| ABI | `abi/` | Task 状态/时间/cursor、Scope/Spawn、phase、provider工厂/credential、typed Analytics与token估计；不是native runtime |
| Analytics | `analytics/` | 可选Plugin/实际hooks、typed非阻塞有界sink，非耐久审计 |
| Core | `core/` | Plugin/Registrar/Builder、单函数typed hooks/单一装配排序、快照和原子内存会话 |
| Context | `context/` | 七hooks/四层store/压缩与引用能力，独立policy输入/累积priority及锁外counter；scope子域管理spawn树/黑板 |
| Memory | `memory/` | backend/embedding/混合检索、单次权重政策与阶段间取消，可选原子file replacement |
| Security | `security/` | 路径/HITL/模式与真实invocation审批合并，拒绝缺失能力及resolver迟到取消，无OS沙箱伪装 |
| Graph | `graph/` | 与AgentLoop平行的typed BSP状态图、Send、checkpoint/retry/channels；状态锁外callback、取消不提交与transform输出所有权回归 |
