# Sub-1 技术规格：GPUI 桌面骨架

> 版本: 0.2
> 日期: 2026-03-20
> 范围: **只做骨架，不做 Agent 逻辑**
> 替代: v0.1 的 Tauri + React 方案

---

## 1. 目标

建立 Srow Agent 的纯 Rust 桌面应用骨架，使用 GPUI 框架（Zed 编辑器的 UI 框架）直接渲染，交付内容：

- 可编译、可运行的 GPUI 桌面应用
- 三栏布局主窗口（Workspace 列表 / 聊天区 / Agent 状态面板）
- 基于 GPUI `Model<T>` + `View<T>` 的组件架构
- 占位模块结构，供后续 Sub-2 ~ Sub-8 填充
- 与 Sub-2 引擎的集成契约（同进程直接调用，无 IPC 跨越）

---

## 2. 技术选型变更

### 2.1 方案对比

| 维度 | 旧方案（v0.1） | 新方案（v0.2） |
|---|---|---|
| 桌面运行时 | Tauri 2.x | 无（原生二进制） |
| UI 渲染 | WebView（WKWebView / WebView2） | GPUI（GPU 渲染，Metal / Vulkan） |
| 前端语言 | TypeScript + React 19 | Rust |
| 前端构建 | Vite + pnpm | Cargo |
| 状态管理 | Zustand | GPUI `Model<T>` + `cx.notify()` |
| UI 组件库 | shadcn/ui + Tailwind CSS | GPUI 布局原语 + 自定义 View |
| IPC 通信 | Tauri IPC（invoke / emit） | 同进程 Rust 函数调用 + GPUI 事件 |
| 类型安全层 | tauri-specta（生成 TS 类型） | 不需要（全是同一语言） |
| npm 依赖 | React / Vite 等约 30 个包 | 零 npm 依赖 |
| 二进制大小 | Rust 后端 + WebView（系统自带） | 单一 Rust 二进制 |

### 2.2 选用 GPUI 的理由

1. **零 WebView 跨界**：Sub-2 引擎是纯 Rust，GPUI 应用与引擎在同一进程内直接调用，省去整套 Tauri IPC 序列化/反序列化链路。
2. **性能一致性**：GPUI 使用 Metal（macOS）/ Vulkan（Windows/Linux）GPU 渲染，文本渲染和动画帧率稳定，不受 WebView 渲染管线影响。
3. **代码统一**：消除 Rust / TypeScript 双语言边界，类型系统统一，重构更安全。
4. **参考实现成熟**：Zed 编辑器是 GPUI 的生产级参考，其 `assistant_panel`、`project_panel`、`editor` 等组件与 Srow Agent 的 UI 需求高度对齐，可直接参考模式。

### 2.3 放弃的备选方案

| 方案 | 放弃原因 |
|---|---|
| 继续 Tauri + React | IPC 边界引入序列化开销和类型重复维护；WebView 渲染管线不可控 |
| Electron | 内嵌 Chromium 体积 +200MB，与 Sub-2 通信仍需 IPC |
| Leptos / Dioxus（Web 技术 Rust 前端） | 仍依赖 WebView 渲染，与 Tauri 问题类似 |
| Iced | 成熟度低于 GPUI，与 Zed 参考无法复用 |
| Slint | DSL 学习成本，社区小，不是 Rust-native |
| egui | 立即模式（immediate mode），状态管理与 Sub-2 引擎的事件驱动模型不匹配 |

---

## 3. GPUI 核心概念速查

GPUI 采用保留模式（retained mode）渲染，核心模式来自 Zed 源码：

| 概念 | 作用 | 对应旧概念 |
|---|---|---|
| `App` | 应用入口，管理事件循环和窗口 | Tauri `Builder` |
| `Window` | 一个操作系统窗口，包含一个根 View | Tauri `Window` |
| `View<T>` | 有状态的 UI 组件（持有 T，实现 `Render` trait） | React Component + State |
| `Model<T>` | 纯数据模型（不渲染，可被多个 View 共享） | Zustand store |
| `cx.notify()` | 标记当前 View 为 dirty，触发下次重绘 | `setState()` |
| `cx.emit(event)` | 向订阅者广播事件 | Tauri `emit()` |
| `cx.subscribe(&model, cb)` | 订阅 Model 或 View 的事件 | Tauri `listen()` |
| `cx.update_model(&m, cb)` | 安全地修改 Model 内部状态 | Zustand `setState` |
| `div()` / `h_flex()` / `v_flex()` | 布局原语（Flexbox 语义） | CSS Flexbox |
| `SharedString` | GPUI 的共享不可变字符串（Arc<str> 语义） | `&str` / `String` |
| `Render` trait | View 必须实现，返回元素树 | React `render()` |
| `Actions` | 键盘快捷键绑定注册 | 无直接对应 |

---

## 4. 项目目录结构

```
srow-agent/
├── Cargo.toml                          # Workspace 根 manifest
├── Cargo.lock
├── .gitignore
│
├── crates/
│   ├── srow-app/                       # ← Sub-1：GPUI 应用主 crate
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs                 # 入口：gpui::App::new()，注册 View，打开窗口
│   │       ├── lib.rs                  # 库根：导出所有子模块，供集成测试引用
│   │       │
│   │       ├── app_state.rs            # AppModel：根 Model，持有各子 Model 句柄
│   │       │
│   │       ├── views/                  # View 层（UI 组件，实现 Render trait）
│   │       │   ├── mod.rs
│   │       │   ├── root_view.rs        # RootView：三栏布局容器，持有三个子 View
│   │       │   ├── side_panel/
│   │       │   │   ├── mod.rs
│   │       │   │   ├── side_panel.rs   # SidePanel：左侧面板容器
│   │       │   │   ├── workspace_list.rs  # WorkspaceList：Workspace 列表
│   │       │   │   └── session_list.rs    # SessionList：Session 列表
│   │       │   ├── chat_panel/
│   │       │   │   ├── mod.rs
│   │       │   │   ├── chat_panel.rs   # ChatPanel：消息列表 + 输入框容器
│   │       │   │   ├── message_list.rs # MessageList：消息渲染列表
│   │       │   │   ├── message_item.rs # MessageItem：单条消息（user/assistant/tool_call）
│   │       │   │   └── input_box.rs    # InputBox：多行输入框 + 发送按钮
│   │       │   └── agent_panel/
│   │       │       ├── mod.rs
│   │       │       ├── agent_panel.rs  # AgentPanel：右侧 Agent 状态面板容器
│   │       │       └── status_row.rs   # StatusRow：单个 Agent 状态行（名称 + 指示灯）
│   │       │
│   │       ├── models/                 # Model 层（纯数据，不渲染）
│   │       │   ├── mod.rs
│   │       │   ├── workspace_model.rs  # WorkspaceModel：Workspace + Session 数据
│   │       │   ├── chat_model.rs       # ChatModel：消息列表 + 草稿
│   │       │   └── agent_model.rs      # AgentModel：Agent 状态映射 session_id → status
│   │       │
│   │       ├── types/                  # 共享类型定义
│   │       │   ├── mod.rs
│   │       │   ├── workspace.rs        # Workspace, Session 结构体
│   │       │   ├── message.rs          # Message, MessageRole, MessageContent
│   │       │   └── agent.rs            # AgentStatus, AgentStatusKind
│   │       │
│   │       ├── engine_bridge/          # Sub-2 集成桥（同进程调用，无 IPC）
│   │       │   ├── mod.rs
│   │       │   └── bridge.rs           # EngineBridge：持有 AgentEngine 句柄，转发事件到 GPUI
│   │       │
│   │       └── error.rs                # SrowError 统一错误类型
│   │
│   └── srow-engine/                    # ← Sub-2：Agent 引擎（单独 crate，Sub-1 阶段为占位）
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs                  # Sub-1 阶段只导出占位类型和 EngineEvent
│
└── docs/
    └── specs/
        ├── sub-1-tauri-skeleton.md     # 本文件
        └── sub-2-agent-engine.md
```

**说明：**
- `srow-app` 是 Sub-1 交付的主 crate，依赖 `srow-engine`
- `srow-engine` 在 Sub-1 阶段只提供类型定义和占位实现，Sub-2 填充真实逻辑
- 不存在 `src/`（前端）目录、`package.json`、`pnpm-workspace.yaml` 等前端工件

---

## 5. GPUI 组件树

### 5.1 View / Model 层级

```
App (gpui::App)
└── Window
    └── RootView                        # View<RootView>：三栏布局根节点
        ├── Model<WorkspaceModel>       # 共享 Model：Workspace + Session 数据
        ├── Model<ChatModel>            # 共享 Model：消息列表 + 草稿
        ├── Model<AgentModel>           # 共享 Model：Agent 状态
        │
        ├── SidePanel                   # View<SidePanel>：左侧面板（宽 220px）
        │   ├── WorkspaceList           # View<WorkspaceList>
        │   │   └── WorkspaceItem × N  # 内联渲染（无独立 View，仅 render 方法返回元素）
        │   └── SessionList             # View<SessionList>
        │       └── SessionItem × N    # 内联渲染
        │
        ├── ChatPanel                   # View<ChatPanel>：中间主区域（flex: 1）
        │   ├── MessageList             # View<MessageList>
        │   │   └── MessageItem × N    # View<MessageItem>（需要独立状态：展开/折叠）
        │   └── InputBox               # View<InputBox>：底部输入框
        │
        └── AgentPanel                  # View<AgentPanel>：右侧面板（宽 280px）
            └── StatusRow × N          # 内联渲染（每个活跃 Session 一行）
```

### 5.2 各组件职责

#### RootView

- 持有三个子 View 的句柄（`View<SidePanel>`、`View<ChatPanel>`、`View<AgentPanel>`）
- 持有三个共享 Model 的句柄
- 负责整体三栏布局（`h_flex()`，左右固定宽度，中间 `flex_1()`）
- 在初始化时订阅 `WorkspaceModel` 事件，驱动 ChatPanel 刷新

#### SidePanel / WorkspaceList / SessionList

- 参考 Zed 的 `project_panel`（文件树 View）
- `WorkspaceList` 渲染 `WorkspaceModel.workspaces`，点击 item 调用 `cx.update_model` 更新 `selected_workspace_id`
- `SessionList` 渲染当前 Workspace 的 Sessions，点击 item 更新 `selected_session_id`
- 两者均通过 `cx.subscribe(&workspace_model, ...)` 监听变化，调用 `cx.notify()` 触发重绘

#### ChatPanel / MessageList / InputBox

- 参考 Zed 的 `assistant_panel`（AI 聊天面板）
- `MessageList` 渲染 `ChatModel.messages[selected_session_id]`，使用 `v_flex()` 垂直堆叠
- `MessageItem` 根据 `MessageRole` 渲染不同气泡样式；`MessageContent::ToolCall` 显示工具调用卡片
- `InputBox` 参考 Zed 的 `editor::Editor`，支持多行输入、`Enter` 换行、`Cmd+Enter` 发送
- Sub-1 阶段：发送后写入 `ChatModel`，1 秒后注入 mock 回复；Sub-2 接管后改为调用 `EngineBridge`

#### AgentPanel / StatusRow

- `AgentModel` 持有 `HashMap<session_id, AgentStatus>`
- `AgentPanel` 订阅 `AgentModel`，遍历所有活跃 session 渲染 `StatusRow`
- `StatusRow` 显示 session 名称和颜色编码的状态指示灯

---

## 6. 组件间通信方式

GPUI 中有三种通信路径，Srow Agent 均会用到：

### 6.1 Model 共享（最常用）

多个 View 持有同一 `Model<T>` 的句柄，通过 `cx.read_model` 读取，通过 `cx.update_model` 修改，修改后调用 `cx.notify()` 或使用 `cx.emit()` 通知订阅者。

```rust
// WorkspaceList 点击 Workspace 时：
cx.update_model(&self.workspace_model, |model, cx| {
    model.selected_workspace_id = Some(workspace_id.clone());
    cx.notify();
});
```

### 6.2 View 内部事件（同 View 内回调）

InputBox 发送按钮点击时，通过 closure 回调 ChatPanel：

```rust
// ChatPanel 在构建 InputBox 时注册回调
InputBox::new(cx, {
    let chat_model = self.chat_model.clone();
    move |text, cx| {
        cx.update_model(&chat_model, |model, cx| {
            model.push_user_message(session_id, text);
            cx.notify();
        });
    }
})
```

### 6.3 Model Event 订阅（跨 View 通知）

Sub-2 引擎事件通过 `AgentModel` 的 `cx.emit()` 广播，多个 View 订阅响应：

```rust
// AgentPanel 订阅 AgentModel 的状态变化事件
cx.subscribe(&agent_model, |this, _model, event: &AgentModelEvent, cx| {
    match event {
        AgentModelEvent::StatusChanged { session_id, status } => {
            cx.notify(); // 触发 AgentPanel 重绘
        }
    }
}).detach();
```

---

## 7. 与 Sub-2 引擎的集成方式

### 7.1 集成原则

Sub-1 和 Sub-2 在**同一进程**内运行，没有 IPC 边界：

```
旧方案（Tauri）：
  前端 JS → invoke() → JSON 序列化 → Tauri IPC → Rust 后端

新方案（GPUI）：
  View → EngineBridge::start_session() → AgentEngine::run() [同进程]
```

### 7.2 EngineBridge 职责

`engine_bridge/bridge.rs` 是 UI 层与引擎层的适配器，职责：

1. 持有 `AgentEngine` 句柄（Sub-1 阶段为 mock，Sub-2 替换为真实引擎）
2. 接收 `EngineEvent` 流（`mpsc::Receiver<EngineEvent>`），将事件转换为 GPUI Model 更新
3. 通过 `cx.update_model` 驱动 `ChatModel` 和 `AgentModel` 更新，触发 UI 重绘

### 7.3 EngineEvent 接入

Sub-2 定义的事件类型（来自 `srow-engine/src/application/engine.rs`）：

```rust
pub enum EngineEvent {
    TextDelta { session_id: String, text: String },
    ToolCallStarted { session_id: String, tool_name: String, tool_call_id: String },
    ToolCallCompleted { session_id: String, tool_call_id: String, output: String, is_error: bool },
    WaitingForHuman { session_id: String, question: String, ask_id: String },
    Completed { session_id: String },
    Error { session_id: String, error: String },
    TokenUsage { session_id: String, input: u32, output: u32, total: u32 },
}
```

`EngineBridge` 在后台 tokio 任务中消费 `event_rx`，通过 GPUI 的 `AsyncAppContext` 将更新调度回主线程：

```rust
// engine_bridge/bridge.rs（示意）
pub struct EngineBridge {
    engine: AgentEngine,
    event_tx: mpsc::Sender<EngineEvent>,
    chat_model: Model<ChatModel>,
    agent_model: Model<AgentModel>,
}

impl EngineBridge {
    pub fn start(
        &self,
        session_id: String,
        prompt: String,
        cx: &mut AppContext,
    ) {
        let (event_tx, mut event_rx) = mpsc::channel(256);
        let chat_model = self.chat_model.clone();
        let agent_model = self.agent_model.clone();

        // 在后台 tokio 任务中运行引擎循环
        let mut engine = /* AgentEngine::new(...) */;
        let async_cx = cx.to_async();

        tokio::spawn(async move {
            engine.run(&session_id, LLMMessage::user(prompt)).await.ok();
        });

        // 在后台 tokio 任务中转发事件到 GPUI
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let chat_model = chat_model.clone();
                let agent_model = agent_model.clone();
                async_cx.update(|cx| {
                    match event {
                        EngineEvent::TextDelta { session_id, text } => {
                            cx.update_model(&chat_model, |model, cx| {
                                model.append_text_delta(&session_id, &text);
                                cx.notify();
                            });
                        }
                        EngineEvent::ToolCallStarted { session_id, tool_name, .. } => {
                            cx.update_model(&chat_model, |model, cx| {
                                model.push_tool_call_start(&session_id, &tool_name);
                                cx.notify();
                            });
                        }
                        EngineEvent::Completed { session_id } => {
                            cx.update_model(&agent_model, |model, cx| {
                                model.set_status(&session_id, AgentStatusKind::Idle);
                                cx.notify();
                            });
                        }
                        EngineEvent::Error { session_id, error } => {
                            cx.update_model(&agent_model, |model, cx| {
                                model.set_status(&session_id, AgentStatusKind::Error);
                                cx.notify();
                            });
                        }
                        _ => {}
                    }
                }).ok();
            }
        });
    }
}
```

### 7.4 Sub-1 占位实现

Sub-1 阶段 `srow-engine` crate 只导出必要类型和一个 mock 实现：

```rust
// srow-engine/src/lib.rs（Sub-1 占位）
pub use types::{EngineEvent, AgentConfig};

pub struct AgentEngine; // 占位

impl AgentEngine {
    pub async fn run_mock(
        session_id: &str,
        prompt: &str,
        event_tx: tokio::sync::mpsc::Sender<EngineEvent>,
    ) {
        // 模拟流式回复：逐字发送 TextDelta，最后发 Completed
        let reply = format!("收到：{}", prompt);
        for ch in reply.chars() {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let _ = event_tx.send(EngineEvent::TextDelta {
                session_id: session_id.to_string(),
                text: ch.to_string(),
            }).await;
        }
        let _ = event_tx.send(EngineEvent::Completed {
            session_id: session_id.to_string(),
        }).await;
    }
}
```

Sub-2 完成后，`EngineBridge` 只需将 `AgentEngine::run_mock` 替换为 `AgentEngine::run`，UI 层无需修改。

---

## 8. 数据类型定义

```rust
// crates/srow-app/src/types/workspace.rs

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: String,          // UUID v4
    pub name: String,
    pub path: String,        // 工作区根目录绝对路径
    pub created_at: i64,     // Unix timestamp (ms)
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,          // UUID v4
    pub workspace_id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}
```

```rust
// crates/srow-app/src/types/message.rs

#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub content: MessageContent,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text { text: String },
    ToolCallStart { tool_name: String, call_id: String },
    ToolCallEnd { call_id: String, output: String, is_error: bool },
    // Sub-2 后扩展
}
```

```rust
// crates/srow-app/src/types/agent.rs

#[derive(Debug, Clone)]
pub struct AgentStatus {
    pub session_id: String,
    pub kind: AgentStatusKind,
    pub detail: Option<String>,   // 当前执行的工具名等
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatusKind {
    Idle,
    Running,
    WaitingHitl,
    Error,
    Offline,
}

impl AgentStatusKind {
    /// 对应 UI 状态指示灯颜色（RGBA hex）
    pub fn color(&self) -> gpui::Rgba {
        match self {
            Self::Idle         => gpui::rgba(0x6B7280FF),  // 灰色
            Self::Running      => gpui::rgba(0x10B981FF),  // 绿色
            Self::WaitingHitl  => gpui::rgba(0xF59E0BFF),  // 黄色
            Self::Error        => gpui::rgba(0xEF4444FF),  // 红色
            Self::Offline      => gpui::rgba(0x374151FF),  // 深灰
        }
    }
}
```

---

## 9. Model 设计

### 9.1 WorkspaceModel

```rust
// crates/srow-app/src/models/workspace_model.rs

pub struct WorkspaceModel {
    pub workspaces: Vec<Workspace>,
    pub selected_workspace_id: Option<String>,
    pub sessions: Vec<Session>,          // 当前 Workspace 的 Session 列表
    pub selected_session_id: Option<String>,
}

pub enum WorkspaceModelEvent {
    WorkspaceSelected { workspace_id: String },
    SessionSelected { session_id: String },
}

impl EventEmitter<WorkspaceModelEvent> for WorkspaceModel {}

impl WorkspaceModel {
    pub fn select_workspace(&mut self, id: String, cx: &mut ModelContext<Self>) {
        self.selected_workspace_id = Some(id.clone());
        // Sub-1 阶段：sessions 从内存 mock 数据过滤
        self.sessions = self.workspaces.iter()
            .find(|w| w.id == id)
            .map(|_| mock_sessions_for(&id))
            .unwrap_or_default();
        cx.emit(WorkspaceModelEvent::WorkspaceSelected { workspace_id: id });
        cx.notify();
    }

    pub fn select_session(&mut self, id: String, cx: &mut ModelContext<Self>) {
        self.selected_session_id = Some(id.clone());
        cx.emit(WorkspaceModelEvent::SessionSelected { session_id: id });
        cx.notify();
    }
}
```

### 9.2 ChatModel

```rust
// crates/srow-app/src/models/chat_model.rs

pub struct ChatModel {
    // key: session_id → messages
    pub messages: std::collections::HashMap<String, Vec<Message>>,
    pub drafts: std::collections::HashMap<String, String>,
    // 当前正在流式输出的 session → 尾部未完成消息 buffer
    pub streaming_buffers: std::collections::HashMap<String, String>,
}

pub enum ChatModelEvent {
    MessageAppended { session_id: String },
    StreamDelta { session_id: String },
}

impl EventEmitter<ChatModelEvent> for ChatModel {}

impl ChatModel {
    pub fn push_user_message(&mut self, session_id: &str, text: String, cx: &mut ModelContext<Self>) {
        let msg = Message { /* ... */ role: MessageRole::User, content: MessageContent::Text { text } };
        self.messages.entry(session_id.to_string()).or_default().push(msg);
        cx.emit(ChatModelEvent::MessageAppended { session_id: session_id.to_string() });
        cx.notify();
    }

    pub fn append_text_delta(&mut self, session_id: &str, delta: &str, cx: &mut ModelContext<Self>) {
        self.streaming_buffers.entry(session_id.to_string()).or_default().push_str(delta);
        cx.emit(ChatModelEvent::StreamDelta { session_id: session_id.to_string() });
        cx.notify();
    }
}
```

### 9.3 AgentModel

```rust
// crates/srow-app/src/models/agent_model.rs

pub struct AgentModel {
    pub statuses: std::collections::HashMap<String, AgentStatus>,
}

pub enum AgentModelEvent {
    StatusChanged { session_id: String },
}

impl EventEmitter<AgentModelEvent> for AgentModel {}

impl AgentModel {
    pub fn set_status(&mut self, session_id: &str, kind: AgentStatusKind, cx: &mut ModelContext<Self>) {
        let status = AgentStatus {
            session_id: session_id.to_string(),
            kind,
            detail: None,
            updated_at: chrono::Utc::now().timestamp_millis(),
        };
        self.statuses.insert(session_id.to_string(), status);
        cx.emit(AgentModelEvent::StatusChanged { session_id: session_id.to_string() });
        cx.notify();
    }
}
```

---

## 10. 布局规格

### 10.1 三栏布局

```
┌─────────────────────────────────────────────────────────┐
│  SidePanel (220px)  │  ChatPanel (flex:1)  │  AgentPanel (280px) │
│                     │                      │                     │
│  Workspace A path A │  MessageList         │  AgentStatusList    │
│  ├─ Session A          │  ├─ UserMessage      │  ├─ 🟢 决策 Agent   │
│  ├─ Session B          │  ├─ AssistantMessage │  ├─ 🟢 浏览器       │
│  └─ Session C          │  ├─ ToolCallMessage  │  └─ ⚫ 编码         │
│                     │  └─ ...              │                     │
│  Workspace B path B │                      │                     │
│  ├─ Session 1          │  InputBox (底部)      │                     │
│  └─ Session 2          │                      │                     │
└─────────────────────────────────────────────────────────┘
```

### 10.2 RootView Render 示意

```rust
impl Render for RootView {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .child(
                // 左侧面板，固定宽度
                div()
                    .w(px(220.))
                    .h_full()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .child(self.side_panel.clone()),
            )
            .child(
                // 中间主区域，弹性填充
                div()
                    .flex_1()
                    .h_full()
                    .child(self.chat_panel.clone()),
            )
            .child(
                // 右侧面板，固定宽度
                div()
                    .w(px(280.))
                    .h_full()
                    .border_l_1()
                    .border_color(cx.theme().border)
                    .child(self.agent_panel.clone()),
            )
    }
}
```

### 10.3 状态指示灯规格

| 状态 | 颜色 | 视觉 |
|---|---|---|
| `Idle` | `#6B7280`（灰） | ⚫ |
| `Running` | `#10B981`（绿） | 🟢 |
| `WaitingHitl` | `#F59E0B`（黄） | 🟡 |
| `Error` | `#EF4444`（红） | 🔴 |
| `Offline` | `#374151`（深灰） | ⚫ |

---

## 11. 应用入口（main.rs）

```rust
// crates/srow-app/src/main.rs

fn main() {
    gpui::App::new().run(|cx: &mut AppContext| {
        // 初始化全局 Model
        let workspace_model = cx.new_model(|_| WorkspaceModel::default());
        let chat_model      = cx.new_model(|_| ChatModel::default());
        let agent_model     = cx.new_model(|_| AgentModel::default());

        // 打开主窗口
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(1280.), px(800.)),
                })),
                titlebar: Some(TitlebarOptions {
                    title: Some("Srow Agent".into()),
                    appears_transparent: false,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |cx| {
                cx.new_view(|cx| {
                    RootView::new(workspace_model, chat_model, agent_model, cx)
                })
            },
        ).unwrap();
    });
}
```

---

## 12. 依赖清单

### 12.1 Workspace Cargo.toml（根）

```toml
[workspace]
members = [
    "crates/srow-app",
    "crates/srow-engine",
]
resolver = "2"

[profile.dev]
incremental = true

[profile.release]
lto = "thin"
codegen-units = 1
panic = "abort"
opt-level = 3
strip = true
```

### 12.2 srow-app/Cargo.toml

```toml
[package]
name = "srow-app"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "srow-agent"
path = "src/main.rs"

[lib]
name = "srow_app"
path = "src/lib.rs"

[dependencies]
# UI 框架（来自 Zed 的 GPUI，通过 git 依赖引用）
gpui = { git = "https://github.com/zed-industries/zed", package = "gpui" }

# 引擎 crate（同 workspace，Sub-1 阶段为占位）
srow-engine = { path = "../srow-engine" }

# 异步运行时（GPUI 内部使用 smol，但 engine bridge 需要 tokio）
tokio = { version = "1", features = ["full"] }

# 序列化（配置文件、状态持久化）
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# ID / 时间
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }

# 错误处理
thiserror = "1"
anyhow = "1"

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
tempfile = "3"
```

### 12.3 srow-engine/Cargo.toml（Sub-1 占位版）

```toml
[package]
name = "srow-engine"
version = "0.1.0"
edition = "2021"
description = "Srow Agent core engine — placeholder for Sub-1, full implementation in Sub-2"

[lib]
name = "srow_engine"
path = "src/lib.rs"

[dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1"

# Sub-2 正式实现后追加：
# rig-core = { version = "0.9", features = ["all"] }
# tokio-rusqlite = "0.5"
# rusqlite = { version = "0.31", features = ["bundled"] }
# async-trait = "0.1"
# futures = "0.3"
```

**注意**：GPUI 目前以 git 依赖方式引用 Zed 主仓库，版本随 Zed 发布节奏更新。正式发布时需固定 `rev` 到稳定的 Zed tag。

---

## 13. 错误类型

```rust
// crates/srow-app/src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum SrowError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("engine error: {0}")]
    Engine(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("internal error: {0}")]
    Internal(String),
}
```

---

## 14. 与 Sub-2 的接口契约

Sub-1 与 Sub-2 通过以下契约解耦，Sub-2 完成后只需替换 `EngineBridge` 内部实现：

### 14.1 引擎启动接口

```rust
// Sub-1 期望 Sub-2 提供的接口（来自 sub-2-agent-engine.md）
use srow_engine::{AgentEngine, EngineBuilder, EngineEvent, AgentConfig};
use srow_engine::{AnthropicProvider, SessionService};

// Sub-2 交付后，EngineBridge 的启动代码：
let (event_tx, event_rx) = mpsc::channel(256);
let (cancel_tx, cancel_rx) = watch::channel(false);

let engine = EngineBuilder::new(agent_config)
    .with_llm(AnthropicProvider::new(&api_key, "claude-opus-4-5"))
    .with_default_sqlite_storage().await?
    .build(event_tx, cancel_rx)?;

let session = SessionService::new(engine.storage())
    .create(&engine.config()).await?;

tokio::spawn(async move {
    engine.run(&session.id, LLMMessage::user(prompt)).await.ok();
});
```

### 14.2 事件类型对齐

`EngineBridge` 消费的 `EngineEvent` 与 Sub-2 定义完全对齐（见第 7.3 节），Sub-1 阶段的 mock 实现覆盖相同的 enum variants。

---

## 15. 实现注意事项

### GPUI 线程模型

- GPUI 的 UI 更新必须在主线程（`AppContext` 线程）执行
- `tokio::spawn` 的后台任务使用 `cx.to_async()` 获取 `AsyncAppContext`，通过 `async_cx.update(|cx| ...)` 调度回主线程
- 不要在后台线程直接调用 `cx.notify()` 或修改 Model，必须经过 `AsyncAppContext::update`

### GPUI 的 Render 不可有副作用

- `Render::render()` 必须是纯函数：只读 `self` 和 `cx`，不做 I/O，不修改状态
- 状态修改只能在事件回调（`on_click`、`cx.subscribe` 回调等）中进行
- 参考 Zed 的 `project_panel::ProjectPanel::render` 学习正确模式

### View 生命周期

- View 被 `Window` 持有，Window 关闭时 View 及其订阅自动析构
- `cx.subscribe().detach()` 会使订阅跟随 View 生命周期自动取消（推荐）
- 不要在 View 外部持有 `ViewContext` 的引用

---

## 16. Sub-1 交付标准

| 验收项 | 说明 |
|---|---|
| `cargo build` 通过 | workspace 所有 crate 无编译错误 |
| `cargo run -p srow-app` 启动 | 主窗口可以打开，无 panic |
| 三栏布局渲染正确 | 左(220px) / 中(弹性) / 右(280px) 三栏正常显示，分隔线清晰 |
| Workspace / Session 可交互 | 可通过 UI 切换 Workspace 和 Session，列表正确刷新 |
| 消息可发送（mock 流式回复） | 用户输入后显示用户消息，流式逐字显示 mock 回复 |
| Agent 状态指示灯 | 右侧面板显示当前 Session 的状态指示灯（初始为 offline，发送消息后变 running，完成后变 idle） |
| EngineBridge 接口可替换 | `EngineBridge` 中有注释标记的接口替换点，Sub-2 只需替换 mock 实现 |

---

## 17. 不在 Sub-1 范围内

| 内容 | 对应 Sub |
|---|---|
| SQLite 持久化 | Sub-2 |
| 实际 Agent 调用（rig）| Sub-2 |
| ACP 协议接入 Claude Code 等 | Sub-3 |
| Skill / MCP 系统 | Sub-4 |
| 多 Agent 编排 | Sub-5 |
| 浏览器自动化 | Sub-6 |
| 沙箱 + HITL | Sub-7 |
| 内嵌 Bun/Python/Chromium | Sub-8 |
| 多窗口（设置、任务面板等） | Sub-5 或后续 |
| 消息虚拟滚动优化 | 视性能需求 |
| 主题切换（暗色/亮色） | 视产品需求 |
| 国际化（i18n） | 视产品需求 |
| 应用签名 / 公证（Notarize） | 发布阶段 |

---

## 附录：GPUI 参考资料

- **GPUI 官网**：https://www.gpui.rs/
- **Zed 源码**（GPUI 生产级参考）：https://github.com/zed-industries/zed
  - `crates/project_panel/` — 参考左侧树形列表实现
  - `crates/assistant/` — 参考聊天面板 + 流式文本渲染
  - `crates/editor/` — 参考 InputBox 多行文本输入
- **GPUI 示例**：`zed/crates/gpui/examples/` — hello_world、input、window_shadow 等
