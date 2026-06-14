# 注入机制统一设计:Plugin + Middleware（砍掉 Event 第 4 层）

> 状态：设计已确认（方案 B + 重命名 + provide-only 规矩），待写实现计划。
> 范围：子项目 ②「注入机制统一」。子项目 ①「Preset 分档」与 ③ 暂不在本 spec。
> 日期：2026-06-14

---

## 1. 背景与问题（带证据）

当前一个 agent 的可扩展能力由 **4 个概念** 注入，其中一个是冗余的：

| 概念 | 角色 | 状态 |
|------|------|------|
| `Tool` | LLM 能调的动词 | 保留 |
| `Middleware`（Midway） | 运行期单点拦截原语，8 钩子，async，能 wrap | 保留 |
| `Extension` | 装配期跨层捆绑包（tools + middleware + bus 服务 + prompt） | 保留（重命名 Plugin） |
| `ExtensionEvent` | middleware 的同步、更弱镜像 | **删除** |

### 证据

1. **Event 层是 middleware 的残废镜像**：`events.rs` 只有 5 个事件
   （`AgentStart / AgentEnd / BeforeToolCall / AfterToolCall / Input`），其中 4 个
   与 middleware 钩子一一重复；handler 是同步闭包 `Fn(&ExtensionEvent) -> EventResult`
   （`host.rs:8`），**不能 await**；它本身用一个 `ExtensionBridgeMiddleware`
   （`bridge.rs:30-60`）实现——即"用 middleware 实现一个比 middleware 弱的东西"。
2. **Event 层 in-tree 0 使用者**：全仓只有 AEP 子进程 loader（`loader.rs:242`）
   通过 `on_as` 用它桥接第三方插件。没有任何 in-tree Rust extension 用过 `on()`。
3. **近半数 Extension 是机械样板**：普查 32 个 `impl Extension`，其中 20 个
   （**173 行，占 47%**）是纯 wrapper：
   - 7 个纯中间件 wrapper（`activate(){ api.middleware(...) }`，19 行）
   - 6 个纯 tool-group wrapper（`tools(){ preset() }`，32 行）
   - 7 个纯 bus-publish wrapper（`configure(){ bus.provide(...) }`，122 行）
4. **双阶段双句柄**：`activate(&HostAPI)`（同步）+ `configure(&ExtensionContext)`（异步）
   职责重叠，迫使聚合插件跨阶段 clone 句柄（`host.rs:135-136` 自述）。
5. **工具双出口**：`tools()` 和 `finalize()->Vec<Tool>` 都能返回 tool；`finalize`
   返回 tool 全仓仅 1 个用户（`SubAgentExtension`，`agent_spawn.rs:499`）。

### 为什么当初是两层（已确认的正当性）

`Middleware` 是单点拦截器，钩子只拿 `&mut AgentState`，**无法**在装配期注册工具 /
`provide` bus 服务 / 写 system prompt。`Extension` 是唯一能把"工具 + 中间件 + bus 服务"
绑在一起的跨层装配包。**这对互补关系是对的，本设计保留它**。冗余的只有 Event 第 4 层 +
"单点 middleware 被迫套 Extension 空壳"。

---

## 2. 目标 / 非目标

**目标**
- 概念 4 → 3（删 Event 层）。
- 插件作者面对**一套**拦截心智（middleware/hook），消除"同步事件陷阱"。
- 单点 middleware 可直接注册，无需空壳；删 20 个 wrapper 的 ~173 行样板。
- `Extension` 的双阶段双句柄收敛为单个 async 注册句柄。
- 语义重命名为产品级名称。

**非目标**
- 不做 Preset 分档（子项目 ①）。
- 不引入声明式 `capability!` 宏（方案 C，可在本设计之上后加，不返工）。
- 不改 typed Bus 能力模型、`MiddlewarePriority` 层级、panic 隔离、tool preset 分组——全保留。

---

## 3. 设计

### 3.1 新核心 trait：`Plugin`

```rust
#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }

    /// 唯一装配阶段：注册 tools / middleware / bus 服务 / system prompt / command。
    async fn register(&self, r: &Registrar<'_>);

    /// 可选的"晚期发现"——所有 plugin 注册完、model + 完整 tool 列表已知后才跑。
    /// 默认空。只有 MCP / SubAgent 这种动态发现的才实现。
    async fn discover_tools(&self, _cx: &LateContext) -> Vec<Arc<dyn Tool>> { vec![] }
}
```

- `tools()` 折进 `register()`（`r.tools(...)`）。
- `activate` + `configure` 合一为 `register()`。
- `finalize()` 双出口降级为可选 `discover_tools()`。
- 方法 6 → 3，生命周期 2 阶段 → 1 阶段（+ 1 个可选晚期回调）。

### 3.2 跨层句柄：`Registrar`

取代 `HostAPI` + `ExtensionContext` 两个句柄。内部仍是 `Arc<RwLock<...>>` + `BusWriter`
（沿用现有实现），对外内部可变，方法均 `&self`：

```rust
impl Registrar<'_> {
    fn tool(&self, t: impl Tool + 'static);
    fn tools(&self, ts: Vec<Box<dyn Tool>>);
    fn middleware(&self, m: impl Middleware + 'static);
    fn provide<T: ?Sized + 'static>(&self, svc: Arc<T>);     // bus 能力
    fn system_prompt(&self, layer: ContextLayer, text: impl Into<String>);
    fn command(&self, name: &str, desc: &str);

    // 只读上下文（原 ExtensionContext 字段）
    fn workspace(&self) -> &Path;
    fn bus(&self) -> &BusHandle;       // 运行期读能力（见 §3.5 规矩）
}
```

`LateContext`（原 `FinalizeContext`）额外暴露 `model: Arc<dyn LanguageModel>`、
`tools: Vec<Arc<dyn Tool>>`、`max_iterations`。

### 3.3 裸 Middleware 不用壳

```rust
AgentBuilder::new()
    .plugin(Box::new(SecurityPlugin::new(...)))      // 跨层能力 → Plugin
    .middleware(ToolTimeoutMiddleware::default());    // 单点拦截 → 直接收，无壳
```

删除这 7 个空壳类型：`LoopDetectionExtension` / `DanglingToolCallExtension` /
`ToolTimeoutExtension` / `CompactionExtension` / `CheckpointExtension` /
`HooksExtension`(评估)/ `EvaluationExtension`(条件) —— 改为在装配处直接 `.middleware()`。
（注：`HooksExtension` / `EvaluationExtension` 若除注册 middleware 外还有逻辑，保留为 Plugin；
纯注册的才删。实现计划阶段逐个核对。）

### 3.4 砍掉 Event 第 4 层

**删除**：`events.rs`（`ExtensionEvent` / `EventResult`）、`HostAPI::on` / `on_as`、
`ExtensionHost` 的 handler map + `emit()`、`bridge.rs` 的事件分发路径。

**AEP loader 迁移**（唯一用户）：第三方子进程插件改由一个内部
`AepBridgeMiddleware`（实现 `Middleware`）驱动。AEP 当前桥接 5 个订阅
（`loader.rs:256-264`），映射到 middleware 钩子：

| AEP 订阅名 | 旧 → ExtensionEvent | 新 → Middleware 钩子 |
|---|---|---|
| `before_tool_call` | BeforeToolCall（可 Block） | `before_tool_call`（返回 Err 即阻断） |
| `after_tool_call` | AfterToolCall | `after_tool_call` |
| `on_agent_start` | AgentStart | `on_agent_start` |
| `on_agent_end` | AgentEnd | `on_agent_end` |
| `on_user_message` | Input | `on_agent_start` 内从 `AgentState` 取最新用户消息文本 |

→ **不新增 middleware 钩子**。`on_user_message` 借 `on_agent_start` + state 还原文本。
附带好处：AEP dispatch 从同步（`dispatch_event_sync`）可升级为 async。

### 3.5 装配顺序规矩（provide-only）

`register()` 只**提供**能力（`provide` / `tool` / `middleware` / `system_prompt` / `command`），
**不读**别家 plugin 提供的 bus 服务。需要读别家能力的：

- 运行期通过 `Registrar::bus()` / middleware 拿到的 `BusHandle` 读；或
- 在 `discover_tools(LateContext)` 晚期读。

→ `register()` 调用顺序与结果无关，去掉了现有"全 activate → 全 configure"两趟的脆顺序依赖，
**比现状更稳**。

### 3.6 装配流程（AgentBuilder 内）

1. 收集所有 `Plugin`。
2. 逐个 `plugin.register(&registrar)`（顺序无关，§3.5）。
3. 收集裸 `.middleware()` + plugin 注册的 middleware，按 `MiddlewarePriority` 稳定排序
   组装 `MiddlewareStack`（沿用现有逻辑）。
4. 注册所有 `register()` 阶段产出的 tool 到 registry。
5. 逐个 `plugin.discover_tools(&late_ctx)`（此时 model + 完整 tool 列表已知），追加注册。

---

## 4. 重命名表

| 旧 | 新 |
|---|---|
| `Extension` (trait) | `Plugin` |
| `HostAPI` + `ExtensionContext` | `Registrar` |
| `FinalizeContext` | `LateContext` |
| `Extension::activate` + `configure` | `Plugin::register` |
| `Extension::finalize` | `Plugin::discover_tools` |
| `XxxExtension`（保留的） | `XxxPlugin` |
| `BaseAgentBuilder::extension()` | `BaseAgentBuilder::plugin()` |

---

## 5. 迁移计划（高层；细节进实现计划）

1. **新增** `Plugin` / `Registrar` / `LateContext`，`AgentBuilder` 支持 `.plugin()` + `.middleware()`。
2. **重写** ~24 个 in-tree `impl Extension` → `impl Plugin`（机械、规律）：
   - 6 个 tool-group → `register(){ r.tools(...) }`
   - 7 个 bus-publish → `register(){ r.provide(...) }`
   - 跨层（Security/Skills/Permission/Analytics/Pending/Lsp/SystemContext）→ `register()` 合并 activate+configure
   - SubAgent → `discover_tools()`；MCP → `register()` 或 `discover_tools()`（按是否需晚期）
3. **删除** 7 个纯中间件空壳，改装配处 `.middleware()`。
4. **删除** Event 层（`events.rs`、`on/on_as`、handler map、`emit`、bridge 事件路径）。
5. **AEP loader** → `AepBridgeMiddleware`（§3.4）。
6. **更新** CLI `agent_setup.rs` + Tauri `ensure_agent` 两处装配列表（暂仍各一份，
   待子项目 ① 收敛为 Preset）。
7. **测试**：`base_agent_overrides.rs` 等同名替换测试改用 `Plugin`；补 AEP 桥接迁移测试。
8. **文档**：更新 `AGENTS.md` 对应描述（顺手修此前发现的 drift：HostAPI steer/follow_up
   不存在、ContextHooks "8 钩子" 口径）。

---

## 6. 风险

| 风险 | 缓解 |
|---|---|
| 两趟 → 一趟装配的顺序依赖 | provide-only 规矩（§3.5）；实现时 grep 现有 configure 是否有"读别家 bus"的反例 |
| AEP `on_user_message` 语义降级 | `on_agent_start` + state 取文本；补桥接测试比对前后行为 |
| 重命名 + 重写 24 处的 diff 噪声 | 一次性清爽重构（已确认接受）；按类别分批 commit |
| 保留 middleware 顺序语义 | `MiddlewarePriority` 与稳定排序逻辑不动 |

---

## 7. 收益小结

- 概念 4 → 3；删 Event 门面层（`events.rs` + bridge 事件路径）。
- 删 20 个 wrapper、~173 行机械样板（其中 7 空壳 middleware 直接删类型）。
- Extension 6 方法/2 句柄 → Plugin 3 方法/1 句柄；干掉跨阶段 clone workaround。
- tool 入口 2 → 1（+ 1 个可选晚期发现）。
- 一套拦截心智、provide-only 的稳定装配顺序、产品级命名。
- 是方案 C（声明式）与子项目 ①（Preset 分档）的干净底座，二者皆可在此之上增量构建。
