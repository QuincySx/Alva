# Agent Architecture V2 — 设计规格

## 设计原则

1. **Agent 是无状态引擎** — 不持有 messages，只管"拿上下文 → 调 LLM → 执行工具 → 交还结果"
2. **AgentSession 是唯一的消息归宿** — 所有消息的存储、查询、持久化都在这里
3. **一套 Hook 系统** — Middleware 是唯一的扩展机制，没有单独的 ContextHooks
4. **State 和 Config 分离** — 可变数据和不可变逻辑互不干扰，解决 Rust 借用冲突
5. **Extensions 放可选数据** — 核心依赖是一等字段，非核心才放 Extensions

---

## 核心结构

### AgentState（可变数据 — "有什么"）

```rust
pub struct AgentState {
    /// LLM 模型
    pub model: Arc<dyn LanguageModel>,
    /// 可用工具
    pub tools: Vec<Arc<dyn Tool>>,
    /// 会话管理器 — messages 的唯一归宿
    pub session: Arc<dyn AgentSession>,
    /// 开放扩展槽 — middleware 专属的可选数据
    pub extensions: Extensions,
}
```

注意：**没有 messages 字段**。所有消息存在 session 里。

### AgentConfig（不变逻辑 — "怎么处理"）

```rust
pub struct AgentConfig {
    /// 中间件栈 — 唯一的 hook 系统
    pub middleware: MiddlewareStack,
    /// system prompt
    pub system_prompt: String,
}
```

### 为什么分成两个

```
AgentState  → &mut 传给 middleware（可变借用）
AgentConfig → &   传给 run loop（不可变借用）

两个独立的借用，不冲突。Rust 编译通过。

如果合成一个 struct：
  middleware 在 Agent 里 + middleware 需要 &mut Agent → 自引用冲突 → 编译不过
```

---

## AgentSession（消息管理器）

### 职责

```
AgentSession 回答的所有问题：
├── 消息存哪里？     → append() 存入，内部决定内存/文件/DB
├── 给 LLM 看什么？  → recent(n) 或 messages() 取出，middleware 组装 context
├── 给 UI 看什么？   → messages() 取出，UI 层渲染
├── 怎么恢复？       → restore() 从持久化加载
├── 谁的 session？   → id() 返回唯一 ID
└── 谁是我的父？     → parent_id() 返回父 session ID（子 Agent 用）
```

### Trait 定义

```rust
pub trait AgentSession: Send + Sync {
    /// Session 唯一标识
    fn id(&self) -> &str;

    /// 父 Session ID（子 Agent 才有）
    fn parent_id(&self) -> Option<&str>;

    /// 追加一条消息
    fn append(&self, message: AgentMessage);

    /// 获取所有消息（给 UI、导出）
    fn messages(&self) -> Vec<AgentMessage>;

    /// 获取最近 N 条（给 context 组装）
    fn recent(&self, n: usize) -> Vec<AgentMessage>;

    /// 持久化到存储
    async fn flush(&self);

    /// 从存储恢复
    async fn restore(&self) -> Vec<AgentMessage>;
}
```

### 默认实现

```
InMemorySession（alva-agent-core 内置）
  纯内存 RwLock<Vec<AgentMessage>>
  开箱即用，不需要配置

FileSession（APP 层，CLI 用）
  JSON 文件，.alva/sessions/{id}.json

SqliteSession（APP 层，GUI 用）
  数据库存储

RemoteSession（APP 层，云端用）
  API 调用
```

### 多 Agent 时的 Session

```
每个 Agent 有独立的 Session，互不干涉：

父 Agent ─── session-A（私有对话记录）
  ├── 子 planner ─── session-B（parent: A）
  └── 子 coder   ─── session-C（parent: A）
      └── 子 helper ─── session-D（parent: C）

Session = 私有笔记本（只有自己看）
Blackboard = 公共白板（通过 BoardMiddleware 通信）
```

---

## Middleware（唯一的 Hook 系统）

### Trait 定义（10 个接口）

```rust
#[async_trait]
pub trait Middleware: Send + Sync {
    // ── Agent 生命周期 ──
    async fn on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> { Ok(()) }
    async fn on_agent_end(&self, state: &mut AgentState, error: Option<&str>) -> Result<(), MiddlewareError> { Ok(()) }

    // ── LLM 调用 ──
    async fn before_llm_call(&self, state: &mut AgentState, messages: &mut Vec<Message>) -> Result<(), MiddlewareError> { Ok(()) }
    async fn after_llm_call(&self, state: &mut AgentState, response: &mut Message) -> Result<(), MiddlewareError> { Ok(()) }
    async fn wrap_llm_call(&self, state: &AgentState, messages: Vec<Message>, next: &dyn LlmCallFn) -> Result<Message, MiddlewareError> {
        next.call(messages).await.map_err(|e| MiddlewareError::Other(e.to_string()))
    }

    // ── Tool 调用 ──
    async fn before_tool_call(&self, state: &mut AgentState, tool_call: &ToolCall) -> Result<(), MiddlewareError> { Ok(()) }
    async fn after_tool_call(&self, state: &mut AgentState, tool_call: &ToolCall, result: &mut ToolResult) -> Result<(), MiddlewareError> { Ok(()) }
    async fn wrap_tool_call(&self, state: &AgentState, tool_call: &ToolCall, next: &dyn ToolCallFn) -> Result<ToolResult, MiddlewareError> {
        next.call(tool_call).await.map_err(|e| MiddlewareError::Other(e.to_string()))
    }

    // ── 元信息 ──
    fn priority(&self) -> i32 { 3000 }
    fn name(&self) -> &str { std::any::type_name::<Self>() }
}
```

### 执行规则

```
before 系列 → 按 priority 从小到大（1000 → 2000 → 3000）
after 系列  → 按 priority 从大到小（3000 → 2000 → 1000）洋葱模型
wrap 系列   → 嵌套包裹（最外层最先拿到请求，最后拿到响应）

Blocked error → 短路，后面的不执行
```

### 标准优先级

```
1000  SECURITY     — 权限、沙箱
1001  DANGLING     — 修复中断的 tool call
2000  GUARDRAIL    — 安全护栏
2001  LOOP_DETECT  — 循环检测
3000  SESSION      — 消息同步（从 session 拿、往 session 存）
3001  COMPRESS     — 上下文压缩（滑窗、摘要）
3002  MEMORY       — 长期记忆注入
3003  BOARD        — 团队通信注入
4000  ROUTING      — 模型选择、A/B 测试
5000  OBSERVATION  — 日志、metrics、tracing
6000  RETRY        — 重试、降级、fallback
```

---

## Run Loop（自由函数）

```rust
pub async fn run_agent(
    state: &mut AgentState,
    config: &AgentConfig,
    cancel: CancellationToken,
    input: Vec<AgentMessage>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentError>
```

### 执行流程

```
1. middleware: on_agent_start(state)

2. 把 input 消息存入 session:
   for msg in input { state.session.append(msg); }

3. 循环（直到没有 tool calls 或被取消）:

   a. 从 session 获取消息:
      messages = state.session.recent(N)  // N 由 CompressMiddleware 决定

   b. middleware: before_llm_call(state, &mut messages)
      — SessionMiddleware 可以在这里补充/过滤消息
      — CompressMiddleware 可以在这里做压缩
      — BoardMiddleware 可以在这里注入团队消息
      — MemoryMiddleware 可以在这里注入记忆

   c. 组装 LLM 输入:
      llm_messages = [system_prompt] + messages + [tool definitions]

   d. middleware: wrap_llm_call(state, llm_messages, actual_llm_call)
      — 内部调用 model.complete() 或 model.stream()
      — RetryMiddleware 可以在这里做重试

   e. 拿到 LLM 响应（assistant_message）

   f. middleware: after_llm_call(state, &mut assistant_message)
      — LoopDetectionMiddleware 在这里做 hash 比对
      — 如果硬停：剥掉 tool_calls

   g. 存入 session:
      state.session.append(assistant_message)

   h. 发送事件:
      event_tx.send(AgentEvent::MessageEnd { message })

   i. 如果有 tool_calls:
      for each tool_call:
        - middleware: wrap_tool_call(state, tool_call, actual_tool_exec)
          — SecurityMiddleware 在这里做权限检查
        - 拿到 tool_result
        - middleware: after_tool_call(state, tool_call, &mut result)
        - state.session.append(tool_result)
        - event_tx.send(AgentEvent::ToolExecutionEnd { ... })
      回到 3a

   j. 如果没有 tool_calls → 跳出循环

4. middleware: on_agent_end(state, error)

5. 返回
```

---

## Extensions（开放数据槽）

```
放什么（middleware 专属的可选数据）：
├── ScopeInfo { depth, parent_id, role }
├── BoardHandle(Arc<Blackboard>)
├── TokenBudget { max, used }
├── LoopState { hashes, warn_count }
└── 未来任何新数据

不放什么（已是一等字段）：
❌ session → state.session
❌ model → state.model
❌ tools → state.tools
```

---

## 子 Agent 派生

```
父 Agent 有 "task" tool。LLM 调用时：

1. 创建子 AgentSession:
   child_session = InMemorySession::new_with_parent(state.session.id())

2. 创建子 AgentState:
   child_state = AgentState {
       model: state.model.clone(),        // 共享 model
       tools: inherited_tools,             // 可选继承
       session: Arc::new(child_session),
       extensions: Extensions::new(),      // 独立的
   }
   child_state.extensions.insert(ScopeInfo {
       depth: parent_depth + 1,
       parent_id: state.session.id(),
       role: "planner",
   })

3. 创建子 AgentConfig:
   child_config = AgentConfig {
       middleware: build_child_middleware(), // 可复用/可定制
       system_prompt: "You are a planner...",
   }

4. 运行:
   run_agent(&mut child_state, &child_config, child_cancel, task_message, child_event_tx)

5. CancellationToken 继承:
   child_cancel = cancel.child_token()  // 父取消 → 子也取消

6. 深度限制:
   ToolGuard（Arc 共享）检查 depth < max_depth

7. 结果:
   作为 ToolResult 返回给父 Agent
```

---

## 事件系统

```rust
pub enum AgentEvent {
    AgentStart,
    AgentEnd { error: Option<String> },

    TurnStart,
    TurnEnd,

    MessageStart { message: AgentMessage },
    MessageUpdate { message: AgentMessage, delta: StreamEvent },  // 流式
    MessageEnd { message: AgentMessage },

    ToolExecutionStart { tool_call: ToolCall },
    ToolExecutionUpdate { tool_call_id: String, update: Value },
    ToolExecutionEnd { tool_call: ToolCall, result: ToolResult },
}
```

调用方通过 `mpsc::UnboundedReceiver<AgentEvent>` 消费事件，实现 CLI 打印或 UI 渲染。

---

## 删除的概念

```
旧概念                → 新架构中的替代
──────                  ──────────────
ContextHooks trait     → Middleware（before_llm_call 做 assemble/inject）
ContextHandle trait    → AgentSession + CompressMiddleware
ContextSystem          → 不再需要
ContextStore           → AgentSession 内部实现
SpawnScope             → Extensions(ScopeInfo) + ScopeMiddleware
MiddlewareContext      → &mut AgentState 直接传
AgentHooks 闭包        → Middleware trait
四层上下文模型          → APP 层 middleware 自行实现（可选）
SessionTracker         → AgentSession.parent_id() 自带树形关系
```

---

## Crate 结构

```
alva-types（定义层 — 0 依赖）
├── AgentMessage, Message, ContentBlock
├── LanguageModel trait, ModelConfig
├── Tool trait, ToolCall, ToolResult, ToolRegistry
├── AgentSession trait              ← 新增
├── Middleware trait, MiddlewareStack
├── Extensions
├── AgentEvent, StreamEvent
├── CancellationToken
├── ToolGuard
└── ScopeInfo, ScopeError（scope types）

alva-agent-core（引擎层）
├── AgentState, AgentConfig
├── run_agent()                     ← 自由函数
├── InMemorySession                 ← 默认实现
├── 内置 Middleware:
│   ├── DanglingToolCallMiddleware
│   └── LoopDetectionMiddleware
└── MiddlewarePriority 常量

alva-agent-scope（协作层 — 可选）
├── Blackboard, BoardMiddleware
├── ScopeMiddleware
└── 相关 Extension 类型

alva-app-core（应用层）
├── BaseAgent builder（组装 State + Config）
├── APP 层 Middleware:
│   ├── SessionMiddleware（管 session 同步策略）
│   ├── CompressMiddleware（上下文压缩）
│   ├── SecurityMiddleware（权限）
│   ├── MemoryMiddleware（长期记忆）
│   └── ObservabilityMiddleware（监控）
├── FileSession / SqliteSession 实现
├── CLI
└── Skills, MCP 等
```
