# Extension System V2 — 运行时插件架构

> 参考: [Pi Interactive Subagents](https://github.com/HazAT/pi-interactive-subagents) + [Pi Coding Agent Extension System](https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent/src/core/extensions)

## 问题

当前 Extension trait 只参与 **构建阶段**：

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> { vec![] }
    async fn configure(&self, ctx: &ExtensionContext) {}
    async fn finalize(&self, ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> { vec![] }
}
```

`build()` 结束后，Extension 和 agent 之间的连接就断了。无法：
- 订阅运行时事件（agent_start、tool_call、turn_end...）
- 注入消息（steer/follow_up）
- 注册 /command 斜杠命令
- 动态启用/禁用工具
- 控制 agent 生命周期（shutdown）
- 自定义渲染

## Pi 的设计

Pi 的 extension 是一个工厂函数，接收 `ExtensionAPI` 对象：

```typescript
// 插件入口
export default (pi: ExtensionAPI) => {
    // 注册工具
    pi.registerTool({ name: "subagent", ... });

    // 注册命令
    pi.registerCommand("plan", { handler: async (args, ctx) => { ... } });

    // 订阅事件
    pi.on("agent_end", async (event, ctx) => {
        if (shouldAutoExit) ctx.shutdown();
    });

    // 运行时注入消息
    pi.on("tool_execution_end", async (event, ctx) => {
        pi.sendMessage({ content: result }, { deliverAs: "steer" });
    });

    // UI widget
    ctx.ui.setWidget("subagents", renderWidget());
};
```

**核心差异**：Pi 的插件拿到的是一个**持久的运行时句柄**，不是一次性的构建钩子。

## Pi 的冲突解决机制

| 场景 | 策略 |
|------|------|
| 多个 handler 监听同一事件 | 按注册顺序串行执行 |
| tool_call 拦截 | 任一 handler 返回 `{ block: true }` 即阻止 |
| input 处理 | 返回 `{ action: "handled" }` 短路后续 handler |
| tool_result 修改 | 链式修改 content/isError，各 handler 看到前一个的修改结果 |
| session 操作取消 | 返回 `{ cancel: true }` |
| 同名工具 | first-registration-wins |
| 同名命令 | 加后缀数字（plan → plan2） |
| handler 异常 | 不影响其他插件，通过 onError 通知 |

## 方案

### 架构分层

```
┌──────────────────────────────────────────────┐
│                  Extension                    │
│  (插件定义: 工具、中间件、事件订阅、命令)         │
└──────────┬───────────────────────────────────┘
           │ 注册到
           ▼
┌──────────────────────────────────────────────┐
│              ExtensionHost                    │
│  (运行时宿主: 事件分发、工具管理、命令路由)       │
│  - handlers: HashMap<EventType, Vec<Handler>> │
│  - tools: Vec<RegisteredTool>                 │
│  - commands: Vec<RegisteredCommand>           │
│  - agent_handle: AgentHandle                  │
└──────────┬───────────────────────────────────┘
           │ 驱动
           ▼
┌──────────────────────────────────────────────┐
│              BaseAgent / AgentLoop            │
│  (在每个生命周期节点调用 host.emit(event))      │
└──────────────────────────────────────────────┘
```

### Phase 1: ExtensionHost + 事件系统

**新增 `ExtensionHost`** — Extension 的运行时容器：

```rust
pub struct ExtensionHost {
    extensions: Vec<Box<dyn Extension>>,
    handlers: HashMap<EventType, Vec<EventHandler>>,
    commands: Vec<RegisteredCommand>,
    agent_handle: Option<AgentHandle>,
}

/// Extension 在构建期通过 HostAPI 注册运行时能力
pub struct HostAPI {
    host: Arc<Mutex<ExtensionHost>>,
}

impl HostAPI {
    /// 订阅事件
    pub fn on<E: ExtensionEvent>(&self, handler: impl Fn(E, &EventContext) -> EventResult);
    
    /// 注册 /command
    pub fn register_command(&self, name: &str, handler: CommandHandler);
    
    /// 运行时注入消息
    pub fn send_message(&self, msg: AgentMessage, deliver_as: DeliverAs);
    
    /// 终止 agent
    pub fn shutdown(&self);
}
```

**Extension trait 新增 `activate()`**：

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    
    // == 构建期（现有） ==
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> { vec![] }
    async fn configure(&self, ctx: &ExtensionContext) {}
    async fn finalize(&self, ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> { vec![] }
    
    // == 运行期（新增） ==
    /// 在 agent 构建完成后调用，接收运行时 API。
    /// 插件在此注册事件监听、命令、快捷键等运行时能力。
    fn activate(&self, _api: &HostAPI) {}
}
```

### Phase 2: 事件类型

```rust
pub enum EventType {
    // Agent 生命周期
    AgentStart,
    AgentEnd,
    TurnStart,
    TurnEnd,
    
    // LLM
    BeforeLlmCall,
    AfterLlmCall,
    MessageStart,
    MessageUpdate,
    MessageEnd,
    
    // 工具
    BeforeToolCall,   // 可 block
    AfterToolCall,    // 可修改 result
    ToolExecutionStart,
    ToolExecutionEnd,
    
    // 输入
    Input,            // 可 transform / handled
    
    // Session
    SessionStart,
    SessionShutdown,
}

pub enum EventResult {
    Continue,
    Block { reason: String },
    Transform { /* modified data */ },
    Handled,
    Cancel,
}
```

### Phase 3: 与现有 Middleware 的关系

**Middleware 和 EventHandler 不是互斥的**：

| | Middleware | Event Handler |
|---|-----------|--------------|
| 时机 | agent loop 内，同步拦截 | agent loop 内，异步通知 |
| 能力 | 可修改 messages/response/tool_call | 可观察 + 可 block/transform |
| 优先级 | 有 priority 排序 | 注册顺序 |
| 适用场景 | 安全检查、压缩、循环检测 | 插件级扩展、异步通知 |

**关系**：Middleware 是底层机制，EventHandler 是上层 API。
ExtensionHost 在 middleware 的 `before_tool_call` 等钩子内调用 `host.emit(BeforeToolCall)`。

```
Middleware Stack                    ExtensionHost
     │                                   │
     │ before_tool_call()                 │
     │───────────────────────────────────>│ emit(BeforeToolCall)
     │                                   │──> handler1() → Continue
     │                                   │──> handler2() → Block("denied")
     │<──────────────────────────────────│ EventResult::Block
     │ return Err(Blocked)               │
```

实现方式：创建一个 `ExtensionBridgeMiddleware`，优先级最高，在每个 hook 里调用 ExtensionHost.emit()。

### Phase 4: 命令系统

```rust
pub struct RegisteredCommand {
    pub name: String,
    pub description: String,
    pub source: String,  // 注册来源的 extension name
    pub handler: Box<dyn Fn(String, &CommandContext) -> Pin<Box<dyn Future<Output = ()>>>>,
}
```

命令通过 `/command args` 语法触发，由 agent 的 input 处理层路由到对应 handler。

### Phase 5: Sub-Agent 作为 Extension 示例

```rust
pub struct SubAgentExtension { max_depth: u32 }

impl Extension for SubAgentExtension {
    fn name(&self) -> &str { "sub-agents" }
    
    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(SubAgentTool::new(self.max_depth))]
    }
    
    fn activate(&self, api: &HostAPI) {
        // 注册 /subagent 命令
        api.register_command("subagent", |args, ctx| async {
            // 解析 args，spawn sub-agent
        });
        
        // 监听子 agent 完成，steer 结果回父 agent
        api.on::<ToolExecutionEnd>(|event, ctx| {
            if event.tool_name == "agent" {
                // 可以在这里做异步通知、widget 更新等
            }
            EventResult::Continue
        });
    }
}
```

## 实施路径

| 阶段 | 内容 | 改动量 |
|------|------|--------|
| **P1** | ExtensionHost 骨架 + activate() + 基础事件 | 中 |
| **P2** | ExtensionBridgeMiddleware（事件→middleware 桥接） | 小 |
| **P3** | 命令注册 + input 路由 | 中 |
| **P4** | AgentHandle（steer/shutdown/动态工具） | 中 |
| **P5** | 迁移现有 Extension 到 activate() 模式 | 小 |

P1-P2 是核心，做完后插件就有运行时能力了。P3-P4 是增强。P5 是清理。

## 不做的事

- **UI 渲染** — Pi 有 TUI，我们的 agent-core 是 headless 的。Widget/theme/editor 不在 scope 内。这些属于 app-cli 层，不属于 extension 系统。
- **Session 持久化** — Pi 用 JSONL session file。我们用 InMemorySession。持久化是独立功能。
- **OAuth Provider** — 模型 provider 管理不在 extension scope。
