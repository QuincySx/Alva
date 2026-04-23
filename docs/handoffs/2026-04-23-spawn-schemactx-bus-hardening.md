# Handoff: SpawnInput 改造 + ToolSchemaContext + Bus 防毒瘤第一层

> **Session**: 2026-04-22 ~ 2026-04-23
> **状态**: 全部已 merge 到主分支（workspace check + 全 lib tests 通过 + CI 依赖防火墙通过）
> **下一 thread 读完这一份即可接着推进**

---

## 一、这轮的主题

让 Alva SDK 在"多 agent 委派 / 通信 / model 指定"方面更通用，同时把 bus 的毒瘤风险用工程纪律管住。具体推了 **6 个 task**（#12 ~ #17）。

## 二、已完成的 6 件事

### 1. `SpawnInput` 加 `model` 字段（子 agent 可选自定义 model）

**文件**: `crates/alva-app-core/src/extension/agent_spawn.rs`

- `SpawnInput.model: Option<String>` —— `"provider/model_id"` 格式
- 空则继承父（默认行为不变）
- 从 bus 拿 `ProviderRegistry` resolve → 传给 `run_child_agent`
- **model 不进 `SpawnScopeImpl`**（scope 只管 lifecycle，model 是 execute 级覆盖）
- 资源统计跨树视角**不考虑**（子用自己的 model 就算子自己的）

### 2. 硬编码 `board` 字段 → 抽象 `comms: Vec<CommSpec>`

**新增**: `crates/alva-kernel-abi/src/scope/spawn/communication.rs`

定义 `SpawnCommunication` trait + 配套类型：
- `SpawnCommContext` / `SpawnCommHandle` / `SpawnCommError`
- `OnChildComplete` trait（子完成后回调）
- `SpawnResult`（避免依赖 kernel-core::ChildAgentResult，消除依赖环）
- `SpawnCommunicationRegistry` trait

`SpawnInput.board` 删除，换成：
```rust
comms: Vec<CommSpec>   // CommSpec { kind: String, config: Value }
```

每个 `CommSpec` 是一个插件化通信能力。Blackboard 成为 `BlackboardCommunication` impl，**可选装**。

**未来能插进来的 kinds**（已留扩展点，未实现）：
- `handoff-recap`（父压缩 recap 喂子）
- `callback`（子完成主动通知父 session）
- `parent-watch`（子订阅父新消息 = 单向继承）
- `bus-event`（子 emit 到 bus 任意订阅）
- `shared-workspace`（父子共享 workspace 子目录）

### 3. 撤掉 ad-hoc setter，全部走 Extension

**背景**：第一版引入了 `BaseAgentBuilder.with_provider_registry()` setter + build() 里默认装载，违反 AGENTS.md 的"没有 ad-hoc setter"哲学。

**修正**：
- **删**：`with_provider_registry()` setter 和相关字段
- **删**：`build()` 里"默认 provide 空 DefaultSpawnCommRegistry"
- **新增** 两个 Extension：
  - `ProviderRegistryExtension::new(Arc<ProviderRegistry>)`
  - `SpawnCommRegistryExtension::new()`
- `BlackboardCommExtension` 已经正确（从 bus 拿 registry，拿不到 `tracing::warn!` 跳过，不 panic），未改。

用户 API 变成纯 Extension 形态：

```rust
BaseAgent::builder()
    .workspace(ws)
    .extension(Box::new(CoreExtension))
    .extension(Box::new(ProviderRegistryExtension::new(registry)))   // 要用 model 就装
    .extension(Box::new(SpawnCommRegistryExtension::new()))          // 要用 comms 就装
    .extension(Box::new(BlackboardCommExtension::new()))             // 要 blackboard 就装
    .build(model).await?;
```

零默认装载，用户按需加。

### 4. Tool trait 加 `ToolSchemaContext`（方案 A）

**背景**：`Tool::parameters_schema(&self) -> Value` 只有 `&self`，拿不到运行时 bus。当需要动态 enum（例如 `AgentSpawnTool.comms.kind` 要从 `SpawnCommunicationRegistry` 拿可用 kinds 列表）时，只能靠"tool 构造时持 Arc"的 hack。

**新增抽象**: `crates/alva-kernel-abi/src/tool/schema.rs`
```rust
pub struct ToolSchemaContext<'a> {
    pub bus: Option<&'a BusHandle>,
}
impl ToolSchemaContext<'_> {
    pub fn empty() -> Self { ... }
    pub fn with_bus(bus: &BusHandle) -> Self { ... }
}
```

**Tool trait 新增默认方法（零 breaking）**:
```rust
fn parameters_schema_with(&self, ctx: &ToolSchemaContext) -> Value {
    // default fallback
    let _ = ctx;
    self.parameters_schema()
}
fn apply_schema_overrides_with(&self, schema: &mut Value, ctx: &ToolSchemaContext) {
    // default fallback
    let _ = ctx;
    self.apply_schema_overrides(schema)
}
```

**关键巧思：`PrebakedSchemaTool` wrapper**
- 在 `kernel-core/run.rs` 的 LLM inference 前，**一次性 bake** 每个 tool 的 dynamic schema（调 `tool.parameters_schema_with(&ctx)`）
- 用 `PrebakedSchemaTool` 包 `Arc<dyn Tool>`，wrapper 的 `parameters_schema()` 返回 baked Value
- Provider 的 `to_*_tools` adapter **不改**，看到 wrapper 的 `parameters_schema()` 就拿到 dynamic schema
- **LanguageModel 接口没动**，所有 provider + mock + test 零改动

**`AgentSpawnTool` 迁移**：
- 保留 `Arc<SpawnScopeImpl>`（spawn_child 还要用）
- `apply_schema_overrides_with`：从 `ctx.bus.get::<dyn SpawnCommunicationRegistry>()` 拿 `list()`，注入到 `comms.items.properties.kind.enum`
- 同时保留无 ctx 的 `apply_schema_overrides`（offline schema dump 仍工作）
- 新增 2 个测试验证 dynamic enum 真的注入

### 5. 评估 `load_skill` / `workflow` / `send_to_thread` 三个 dynamic-enum 工具可行性

**结论**：三个 tool **都不需要扩 `ToolSchemaContext` 字段**，现有 `bus: Option<&BusHandle>` 够用。

| Tool | 状态 | 新代码量估 | 瓶颈 |
|---|---|---|---|
| `load_skill` | ✅ **架构直接支撑** | ~50 行 | 只需 bus provide `SkillStore` + 加 sync list snapshot 方法（当前 `list()` 是 async）|
| `workflow` | ⚠️ **要补基建** | 200-500 行 | `WorkflowRegistry` **零实现**，而且 "workflow 是什么" 未定（SkillKind subtype / CompiledGraph / 独立 domain？）|
| `send_to_thread` | ⚠️/❌ **kernel 级新能力** | ~500 行 | Schema 层容易，但 cross-session 消息路由需要新设计（`PendingMessageQueue` 当前 BaseAgent 私有）|

**推荐顺序**：
1. 先做 `load_skill`（1 小时，兑现 `ToolSchemaContext` 机制第二个 user）
2. **停下来决策 "workflow 语义"** —— 这是设计问题，不是代码任务
3. Tool B：定了之后就是机械活
4. Tool C 最后：先把 cross-session 运行时作为独立 feature 设计

### 6. Bus 防毒瘤第一层（ABC）

**A. `docs/BUS-INVENTORY.md`**（新）
- 当前 **8 Caps + 3 Events** 全部登记
- 每项有 Provider / Consumer(s) / Why bus / Stability 四列
- **所有 "Why bus" 都填得出** —— 零 "不该在 bus 上" 的嫌疑
- 双/替代注册（`TokenCounter` 两个 entry point、`ApprovalNotifier` 同理）如实列在 "Double / alternate providers" 小节

**B. `scripts/check-bus-inventory.sh`**（新，~210 行）
- 扫代码里所有 `.provide::<T>` / `.provide(Arc::new(T))` / `impl BusEvent for T` 的类型名
- 与 `BUS-INVENTORY.md` 双向对比
- drift 就 fail
- 接入 `scripts/ci-check-deps.sh`
- 当前 HEAD 跑 exit 0

**C. Cap / Event 的 doc 格式强制**
- `docs/BUS-RULES.md` 加 "Cap 文档规则（强制）" block
- Review 清单加 2 项（三字段 doc check + inventory 同步 check）
- **回填** 10 个类型的 doc：`TokenCounter` / `PermissionModeService` / `PlanModeControl` / `PendingService` / `SpawnCommunicationRegistry` / `ProviderRegistry` / `MemoryService` / `SecurityGuard` / `ApprovalNotifier` / `CheckpointCallbackRef`
- **回填** 3 个 Event 的 doc：`TokenBudgetExceeded` / `ContextCompacted` / `MemoryExtracted`

**纠正上一轮的误报**：
- 我之前以为 `PermissionModeService` 双注册（`base_agent/agent.rs:227`）—— agent 核实后发现那是 `CheckpointCallbackRef`，不是 `PermissionModeService`。**实际只有一个 provide 点（PlanModeExtension）**。

---

## 三、关键架构决策（新 thread 要理解的）

1. **Schema 是运行时产物**：`Tool` trait 正式承认 schema 可能依赖 bus 上的 registry。机制是 `parameters_schema_with(&ToolSchemaContext)` + `PrebakedSchemaTool` wrapper。

2. **通信能力是插件**：`SpawnCommunication` trait 在 kernel-abi，每种通信模式（Blackboard / 未来的 handoff / callback / ...）是一个 impl。`SpawnInput.comms` 可叠加。

3. **Model 是 execute-time override，不进 scope**：`SpawnScopeImpl` 继续只管 lifecycle（tree / depth / timeout / session_id）。资源统计跨树的事**不考虑**。

4. **零 ad-hoc setter**：所有 registry / service 注入都通过 Extension，builder 只有 `workspace / system_prompt / max_iterations / extension / with_approval_channel / with_memory` 这些 pre-existing 的基本项。

5. **Bus 上全部 dyn Trait / concrete struct 都在清单里**：`BUS-INVENTORY.md` 是 single source of truth，CI 强制同步。

6. **Bus 防线分三层，目前只上了第一层**：
   - **第一层**（已做）：inventory + CI 对账 + doc 格式强制
   - **第二层**（未做）：Cap 分级（stable/experimental/internal）+ snapshot test + same-crate bus lint
   - **第三层**（可能永远不做）：namespaced bus

---

## 四、当前状态

**代码**：
- `cargo check --workspace` → 干净（仅 pre-existing 无关 warning）
- `cargo test --lib` → 全 356 tests 绿
- `scripts/ci-check-deps.sh` → 通过
- `scripts/check-bus-inventory.sh` → 通过

**文档**：
- `docs/BUS-INVENTORY.md`（新）
- `docs/BUS-RULES.md`（补"Cap 文档规则" + review 清单）
- `docs/handoffs/`（新目录，本文件是首个）
- 各层 AGENTS.md 已级联更新（kernel-abi / agent-context / app-core）
- 源码文件三行头注释（INPUT/OUTPUT/POS）维护完毕

**遗留**：
- `check-bus-inventory.sh` 无法解析变量形式 provide（`.provide(notifier)` 里 notifier 是 local）—— 未来可上 `#[bus_cap]` attribute macro 做 AST 级精确 check
- Cap trait 的三字段 doc 未被 CI 硬强制（只强制 inventory）—— 可加 grep-based check
- `alva-app-core` 的 e2e 测试（`e2e_agent_test.rs` / `e2e_http_test.rs`）在 main 分支已坏，**不是本轮引入**。独立 issue。

---

## 五、新 thread 可选的推进方向

**Option 1（推荐）**：立刻做 `load_skill` 的 dynamic enum
- 让 `ToolSchemaContext` 机制有第二个 user，一劳永逸验证通用性
- ~50 行，1 小时
- 前置：bus 上 provide `Arc<SkillStore>`（只差一行），加 sync list snapshot 方法

**Option 2**：决策 "workflow" 语义
- 是 `SkillKind::Workflow` 子类型？
- 是预注册的 `CompiledGraph<S>`（alva-agent-graph）？
- 是独立 domain model（带参数 schema + 验证）？
- 这是**纯设计讨论**，不是代码任务

**Option 3**：跨 session 消息路由 feature
- `SessionRegistry` trait 在 kernel-abi
- Harness 侧实现（`alva-app-tauri` 的 `SqliteEvalSessionManager` 已经有部分能力）
- 跨 session 投递的 delivery 机制（`PendingMessageQueue` 怎么对外暴露）
- 这是中等规模 feature，500+ 行

**Option 4**：Bus 防线第二层
- 等 Cap 涨到 15+ 才值，现在不做
- 或者实际踩坑后再做

**Option 5（如果用户想先休息）**：消化 / 文档沉淀 / 没必要推新 feature

---

## 六、核心文件引用（绝对路径）

**Spawn / Communication**：
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-abi/src/scope/spawn/communication.rs`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-agent-context/src/scope/blackboard/communication.rs`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/extension/agent_spawn.rs`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/extension/spawn_comm_registry.rs`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/extension/blackboard_comm.rs`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-app-core/src/extension/provider_registry.rs`

**Schema context**：
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-abi/src/tool/schema.rs`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-abi/src/tool/types.rs`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-macros/src/lib.rs`
- `/Users/smallraw/Development/QuincyWork/alva-agent/crates/alva-kernel-core/src/run.rs`（`PrebakedSchemaTool` + `bake_tool_schemas`）

**Bus 防线**：
- `/Users/smallraw/Development/QuincyWork/alva-agent/docs/BUS-INVENTORY.md`
- `/Users/smallraw/Development/QuincyWork/alva-agent/docs/BUS-RULES.md`
- `/Users/smallraw/Development/QuincyWork/alva-agent/scripts/check-bus-inventory.sh`
- `/Users/smallraw/Development/QuincyWork/alva-agent/scripts/ci-check-deps.sh`

---

## 七、新 thread 快速加载上下文

```
cat /Users/smallraw/Development/QuincyWork/alva-agent/docs/handoffs/2026-04-23-spawn-schemactx-bus-hardening.md
cat /Users/smallraw/Development/QuincyWork/alva-agent/docs/BUS-INVENTORY.md
cat /Users/smallraw/Development/QuincyWork/alva-agent/docs/BUS-RULES.md
```

读这三份 + 根 `AGENTS.md` + `FRACTAL-DOCS.md`，context 够用。

具体代码读 `agent_spawn.rs` 和 `tool/schema.rs`。
