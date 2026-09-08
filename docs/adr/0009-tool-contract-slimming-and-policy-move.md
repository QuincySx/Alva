# ADR-0009: Tool 契约瘦身与策略外移

- Status: Accepted
- Date: 2026-09-09
- Scope: alva-kernel-abi::tool, alva-macros (derive Tool), alva-agent-security,
  alva-agent-extension-builtin, alva-kernel-core 批执行器
- Supersedes: 内核边界重构 tickets 02/03/04

## Context

重构前 `Tool` trait 有 22 个方法。盘点的结果：

- **8 个死方法**（`is_search_or_read` / `user_facing_name` /
  `max_result_size_chars` / `should_defer` / `tool_prompt` / `aliases` /
  `is_enabled` / `is_destructive`）：前七个零重写零消费；`is_destructive`
  由派生宏替五十个工具生成，而全仓没有一行代码读它。
- **7 个模态契约**（图像/语音/视频/转录/审核/重排/嵌入）全仓零实现，却长在
  Provider trait 与注册表上。
- **调度问答拆成四个方法**（`is_concurrency_safe` / `resource_keys` /
  `execution_mode` / `manages_own_timeout`）——它们永远一起被问、一起被用。
- **schema 问答拆成四个方法**（`parameters_schema` / `parameters_schema_with`
  / `apply_schema_overrides` / `apply_schema_overrides_with`）加一个
  `definition()` 便捷方法——运行期动态 enum 与静态 schema 各占一条路径。
- **`check_permissions` 把两件事焊在一起**：决策（允许/询问/拒绝）与描述
  （"我拿这份入参会干什么"）。更糟的是它能返回 `Deny`——**工具可以硬拒
  自己而 harness 无法覆盖**。今天没爆只是因为除了它自己没人读这个返回值；
  request_escalation 靠它强制审批，意味着一个"权限门"事实上一万个调用点
  之外藏在工具结构体里。

## Decision

### 1. Tool 收敛为七个方法

```rust
fn name(&self) -> &str;
fn description(&self) -> &str;
fn parameters_schema(&self, ctx: &ToolSchemaContext) -> serde_json::Value;
async fn execute(&self, input, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError>;
fn scheduling(&self, input: &serde_json::Value) -> Scheduling;
fn describe_effect(&self, input: &serde_json::Value) -> EffectDescription;
fn is_read_only(&self, input: &serde_json::Value) -> bool;
```

- 调度：四个方法合成 `scheduling(input) -> Scheduling` 描述符（执行模式、
  并发安全、资源键、自管超时）。默认值与拆分前逐条一致。批执行器与超时
  中间件改读描述符，并行/锁/超时行为逐条不变。
- schema：`parameters_schema(ctx)` 成为唯一入口（带 ToolSchemaContext），
  动态 enum 走 `#[tool(schema = "method")]` 固有钩子；
  `definition()` 换成自由函数 `tool_definition(tool, ctx)`。
- 派生宏同步生成 7 方法面，`destructive` 属性删除。

### 2. 工具自述行为，harness 决定权限

- `check_permissions -> ToolPermissionResult` 删除，替换为
  `describe_effect -> EffectDescription`（只有 summary 一句话）。类型层面
  **不存在拒绝动词**——工具结构上无法单方面拒绝自己。
- 决策动词落在 harness 工具策略层：SecurityGuard 的危险工具名单（可配置、
  产品可覆盖任何判断）。`request_escalation` 从"工具自己声明 Ask"改为
  "harness 名单把关"，进入 `DEFAULT_DANGEROUS_TOOLS`。
- 触发宿主命令升级的工具的审批自述语句与改动前逐字一致（测试钉住）。

### 3. 为什么"工具可以拒绝自己"必须消除

同一工具在不同安全档下答案不同：interactive 档该问，bypass 档该放，
plan 档该拒。工具无权替产品拍板；如果工具内嵌 Deny，则 (a) 产品无法
覆盖——安全档升级（例如未来的自动审批模式）会被个别工具静默架开；
(b) 决策逻辑碎片化到每个工具体里，审计无法只读一处。(c) 它还制造了
错误的抽象直觉：看似"每个工具管好自己的权限"，实际是让机制层失去了
唯一的、可审计的策略点。消除方式不是"改 Deny 的语义"，而是让类型上
不存在 Deny。

## Alternatives Considered

- **保留 `is_destructive` 给 UI 着色用**：否决——"没有一行代码读"是盘出的
  事实而非猜测；真要着色时按当时的消费形态重新设计，不预支。
- **调度保留四个方法（更"自文档"）**：否决——四个答案永远一起消费
  （波次调度 + 锁获取 + 超时中间件各读其中几个），拆开只是让每个工具作者
  多面对三个钩子；描述符一个概念承载，默认值语义不变。
- **EffectDescription 保留结构化字段（资源、影响面）**：推迟——当前唯一
  消费者是审批提示语（一句话）；结构化等到有第二个真实消费者再按需求
  设计，避免重蹈"凭想象留扩展点"。

## Consequences

### Positive

- 第三方读得完的工具契约：7 个方法，每一个都有真实消费者。
- 权限决策单点化：SecurityGuard 名单 + 模式 + 规则 + 缓存，一处审计。
- schema 问答单路径，派生宏属性驱动的调度描述让工具作者少写样板。

### Negative

- 手写 `impl Tool` 的存量工具（26 处）随签名迁移——一次性机械成本，
  已在本重构内完成。
- `EffectDescription` 目前只有 summary 字符串；未来若审批 UI 需要结构化
  影响面（文件数、网络出口），是一个被有意推迟的扩展。
