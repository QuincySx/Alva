# Sub-1 技术规格：Tauri 骨架 + UI 框架

> 版本: 0.1
> 日期: 2026-03-20
> 范围: **只做骨架，不做 Agent 逻辑**

---

## 1. 目标

建立 Srow Agent 的 Tauri 2.x 项目骨架，交付内容：

- 可编译、可运行的 Tauri 桌面应用
- 三栏布局主窗口（Workspace 列表 / 聊天区 / Agent 状态面板）
- 前后端 IPC 通信框架（commands + events）
- 占位模块结构，供后续 Sub-2 ~ Sub-8 填充

---

## 2. 技术选型

### 2.1 核心框架

| 层 | 选型 | 理由 |
|---|---|---|
| 桌面运行时 | **Tauri 2.x** | Wukong 验证过可行；主二进制无需内嵌 Chromium，体积小 |
| Rust 异步运行时 | **tokio** | Tauri 默认运行时，全链路异步 |
| WebView 渲染 | **WKWebView (macOS) / WebView2 (Windows)** | Tauri on macOS 的标准路径 |
| 前端框架 | **React 19 + TypeScript** | Alma 前端、社区生态主流，Tauri 官方支持 |
| 前端构建 | **Vite** | Alma 参考方案（electron-vite → vite），快速 HMR |
| 前端状态管理 | **Zustand** | 轻量，无 Redux 样板代码，适合桌面应用 |
| UI 组件 | **shadcn/ui + Tailwind CSS** | 无运行时依赖，组件可定制 |
| IPC 类型安全 | **tauri-specta** | 从 Rust handler 自动生成 TypeScript 类型 |

### 2.2 放弃的备选方案

| 方案 | 放弃原因 |
|---|---|
| Electron | 内嵌 Chromium 体积 +200MB，Wukong 已明确选 Tauri |
| Next.js | SSR 与 Tauri WebView 不匹配，徒增复杂度 |
| SolidJS / Svelte | React 生态更广，组件库选择更丰富 |
| Leptos (全 Rust 前端) | 学习成本高，UI 迭代慢，不适合此阶段 |
| Redux / Jotai | Redux 样板多；Jotai 在 Zustand 之上无额外收益 |

---

## 3. 项目目录结构

```
srow-agent/
├── Cargo.toml                          # Workspace 根 manifest
├── Cargo.lock
├── package.json                        # 前端 workspace 根
├── pnpm-workspace.yaml
├── .gitignore
│
├── src-tauri/                          # Rust 后端
│   ├── Cargo.toml
│   ├── tauri.conf.json                 # Tauri 应用配置（窗口、权限、bundle）
│   ├── capabilities/
│   │   └── default.json               # Tauri 2.x 权限能力声明
│   ├── icons/                         # 应用图标（各尺寸 PNG + .icns）
│   │   ├── icon.icns
│   │   ├── icon.ico
│   │   └── icon.png
│   │
│   └── src/
│       ├── main.rs                     # 入口：Tauri Builder + 插件注册
│       ├── lib.rs                      # 库根：导出所有子模块，供测试引用
│       │
│       ├── commands/                   # Tauri IPC Command 处理器（前端可直接调用）
│       │   ├── mod.rs
│       │   ├── workspace.rs            # workspace_list, workspace_create, workspace_delete
│       │   ├── session.rs              # session_list, session_create, session_delete, session_get
│       │   ├── message.rs             # message_send, message_list
│       │   └── agent_status.rs        # agent_status_get
│       │
│       ├── events/                    # Tauri Event 定义（后端向前端推送）
│       │   ├── mod.rs
│       │   └── types.rs              # AgentStatusChanged, MessageReceived, SessionUpdated
│       │
│       ├── state/                     # 全局应用状态（Arc<Mutex<T>> 注入 Tauri）
│       │   ├── mod.rs
│       │   ├── app_state.rs           # AppState 结构体：持有各子系统句柄
│       │   └── workspace_store.rs     # 内存工作区/会话状态（Sub-2 前的占位）
│       │
│       ├── gateway/                   # 网关层（参考 Wukong gateway/）
│       │   ├── mod.rs
│       │   ├── models.rs              # 请求/响应数据结构
│       │   ├── service.rs             # 网关服务（Sub-2 前返回 mock 数据）
│       │   └── channel_registry.rs   # 频道注册表（占位）
│       │
│       ├── agent/                     # Agent 核心（Sub-2 填充，此处只放占位）
│       │   ├── mod.rs
│       │   └── placeholder.rs        # 说明注释，标记 Sub-2 接管范围
│       │
│       ├── system/                    # 系统能力
│       │   ├── mod.rs
│       │   └── notification.rs       # macOS 系统通知（占位）
│       │
│       ├── base/                      # 基础设施
│       │   ├── mod.rs
│       │   ├── process_manager/
│       │   │   ├── mod.rs
│       │   │   ├── manager.rs        # 子进程 spawn / 监控（占位，Sub-2 使用）
│       │   │   └── lifecycle.rs      # 进程状态机 Running/Exited/Crashed/Restarting
│       │   └── ipc/
│       │       ├── mod.rs
│       │       └── types.rs          # IPC 公共类型（错误包装、序列化帮助）
│       │
│       └── types/                    # 共享类型定义
│           ├── mod.rs
│           ├── workspace.rs          # Workspace, Session 结构体
│           ├── message.rs            # Message, MessageRole, MessageContent
│           └── agent.rs              # AgentStatus, AgentStatusKind
│
└── src/                              # 前端（React + Vite）
    ├── index.html                    # 单 HTML 入口（Sub-1 单窗口）
    ├── vite.config.ts
    ├── tsconfig.json
    ├── tailwind.config.ts
    │
    ├── main.tsx                      # React 挂载入口
    ├── App.tsx                       # 根组件，初始化 store，渲染 MainLayout
    │
    ├── components/                   # UI 组件
    │   ├── layout/
    │   │   ├── MainLayout.tsx        # 三栏布局容器（CSS Grid）
    │   │   ├── LeftPanel.tsx         # 左侧面板：Workspace + Session 列表
    │   │   ├── CenterPanel.tsx       # 中间：聊天主区域
    │   │   └── RightPanel.tsx        # 右侧：Agent 状态面板
    │   │
    │   ├── workspace/
    │   │   ├── WorkspaceList.tsx     # Workspace 列表
    │   │   ├── WorkspaceItem.tsx     # 单个 Workspace 条目
    │   │   └── SessionList.tsx       # Session 列表（属于选中 Workspace）
    │   │
    │   ├── chat/
    │   │   ├── ChatArea.tsx          # 消息列表 + 滚动容器
    │   │   ├── MessageBubble.tsx     # 单条消息气泡
    │   │   └── ChatInput.tsx         # 输入框 + 发送按钮
    │   │
    │   ├── agent/
    │   │   ├── AgentStatusPanel.tsx  # 右侧 Agent 状态面板容器
    │   │   └── StatusIndicator.tsx  # 状态指示灯（🟢🟡⚫🔴）
    │   │
    │   └── ui/                       # shadcn/ui 基础组件（按需添加）
    │       ├── button.tsx
    │       ├── input.tsx
    │       ├── scroll-area.tsx
    │       └── tooltip.tsx
    │
    ├── stores/                       # Zustand 状态 store
    │   ├── workspaceStore.ts         # 当前选中 Workspace + Session 列表
    │   ├── chatStore.ts              # 消息列表、输入草稿
    │   └── agentStore.ts             # Agent 状态列表（按 session_id 索引）
    │
    ├── ipc/                          # Tauri IPC 封装层
    │   ├── commands.ts               # invoke() 封装，带类型（由 tauri-specta 生成）
    │   └── events.ts                 # listen() 封装，前端事件订阅
    │
    ├── types/                        # 前端 TypeScript 类型
    │   ├── workspace.ts              # Workspace, Session（与 Rust 类型对齐）
    │   ├── message.ts                # Message, MessageRole
    │   └── agent.ts                  # AgentStatus, AgentStatusKind
    │
    └── hooks/                        # React 自定义 Hook
        ├── useWorkspaces.ts          # 加载 Workspace 列表
        ├── useSession.ts             # 加载 Session 消息
        └── useAgentStatus.ts        # 订阅 Agent 状态事件
```

---

## 4. 前端组件结构

### 4.1 布局层级

```
App
└── MainLayout                        # CSS Grid: 220px | 1fr | 280px
    ├── LeftPanel
    │   ├── WorkspaceList
    │   │   └── WorkspaceItem × N
    │   └── SessionList
    │       └── SessionItem × N
    ├── CenterPanel
    │   ├── ChatArea
    │   │   └── MessageBubble × N
    │   └── ChatInput
    └── RightPanel
        └── AgentStatusPanel
            └── StatusIndicator × N   # 每个 session 一个指示灯
```

### 4.2 布局规格

```css
/* MainLayout: 三栏，固定左右，中间弹性 */
display: grid;
grid-template-columns: 220px 1fr 280px;
height: 100vh;
overflow: hidden;
```

左侧面板（220px）：
- 上方：Workspace 列表，可折叠
- 下方：当前选中 Workspace 的 Session 列表

中间面板（1fr）：
- 顶部：Session 标题栏 + 控制按钮
- 主体：消息列表（虚拟滚动，Sub-1 先用简单列表）
- 底部：输入框（Textarea + 发送按钮）

右侧面板（280px）：
- 标题："Agent 状态"
- 每个活跃 Session 一行，显示 session 名称 + 状态指示灯
- Sub-1 阶段状态指示灯只做 UI，数据从 `agentStore` 读

### 4.3 状态指示灯规格

```typescript
type AgentStatusKind = 'idle' | 'running' | 'waiting_hitl' | 'error' | 'offline';

// 颜色映射
const STATUS_COLORS: Record<AgentStatusKind, string> = {
  idle:         '#6B7280',  // 灰色  ⚫
  running:      '#10B981',  // 绿色  🟢
  waiting_hitl: '#F59E0B',  // 黄色  🟡
  error:        '#EF4444',  // 红色  🔴
  offline:      '#374151',  // 深灰  ⚫
};
```

---

## 5. Tauri IPC 接口定义

### 5.1 Commands（前端 → Rust）

所有 command 通过 `invoke()` 调用，返回 `Promise<T>`。错误统一用 `SrowError` 包装。

#### 5.1.1 Workspace Commands

```rust
// 列出所有 Workspace
#[tauri::command]
async fn workspace_list(state: State<AppState>) -> Result<Vec<Workspace>, SrowError>

// 创建 Workspace
#[tauri::command]
async fn workspace_create(
    state: State<AppState>,
    name: String,
    path: String,               // 工作区根目录（绝对路径）
) -> Result<Workspace, SrowError>

// 删除 Workspace
#[tauri::command]
async fn workspace_delete(
    state: State<AppState>,
    workspace_id: String,
) -> Result<(), SrowError>
```

#### 5.1.2 Session Commands

```rust
// 列出指定 Workspace 下的 Session
#[tauri::command]
async fn session_list(
    state: State<AppState>,
    workspace_id: String,
) -> Result<Vec<Session>, SrowError>

// 创建 Session
#[tauri::command]
async fn session_create(
    state: State<AppState>,
    workspace_id: String,
    name: String,
) -> Result<Session, SrowError>

// 删除 Session
#[tauri::command]
async fn session_delete(
    state: State<AppState>,
    session_id: String,
) -> Result<(), SrowError>

// 获取 Session 详情
#[tauri::command]
async fn session_get(
    state: State<AppState>,
    session_id: String,
) -> Result<Session, SrowError>
```

#### 5.1.3 Message Commands

```rust
// 获取 Session 的消息历史
#[tauri::command]
async fn message_list(
    state: State<AppState>,
    session_id: String,
    limit: Option<u32>,
    before_id: Option<String>,
) -> Result<Vec<Message>, SrowError>

// 发送用户消息（Sub-1 阶段只入库，不触发 Agent）
#[tauri::command]
async fn message_send(
    state: State<AppState>,
    session_id: String,
    content: String,
) -> Result<Message, SrowError>
```

#### 5.1.4 Agent Status Commands

```rust
// 获取所有活跃 Session 的 Agent 状态快照
#[tauri::command]
async fn agent_status_get(
    state: State<AppState>,
) -> Result<Vec<AgentStatus>, SrowError>
```

### 5.2 Events（Rust → 前端）

所有事件通过 `app.emit()` 发送，前端用 `listen()` 订阅。

#### 事件列表

| 事件名 | Payload 类型 | 触发时机 |
|---|---|---|
| `agent-status-changed` | `AgentStatusChangedPayload` | Agent 状态变化时 |
| `message-received` | `MessageReceivedPayload` | 收到新消息（Agent 回复）时 |
| `session-updated` | `SessionUpdatedPayload` | Session 元数据变更时 |

#### Payload 类型定义（Rust）

```rust
#[derive(Serialize, Clone)]
pub struct AgentStatusChangedPayload {
    pub session_id: String,
    pub status: AgentStatus,
}

#[derive(Serialize, Clone)]
pub struct MessageReceivedPayload {
    pub session_id: String,
    pub message: Message,
}

#[derive(Serialize, Clone)]
pub struct SessionUpdatedPayload {
    pub session: Session,
}
```

### 5.3 共享数据类型（Rust 端，serde 序列化）

```rust
// types/workspace.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,                    // UUID v4
    pub name: String,
    pub path: String,                  // 工作区根目录绝对路径
    pub created_at: i64,               // Unix timestamp (ms)
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,                    // UUID v4
    pub workspace_id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

// types/message.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,                    // UUID v4
    pub session_id: String,
    pub role: MessageRole,
    pub content: MessageContent,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text { text: String },
    // Sub-2 后扩展：ToolCall, ToolResult, Image 等
}

// types/agent.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub session_id: String,
    pub kind: AgentStatusKind,
    pub detail: Option<String>,        // 可选的详细说明（如当前执行的工具名）
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatusKind {
    Idle,
    Running,
    WaitingHitl,                       // 等待人类审批（Human-In-The-Loop）
    Error,
    Offline,
}
```

---

## 6. 依赖清单

### 6.1 Cargo.toml（src-tauri/Cargo.toml）

```toml
[package]
name = "srow-agent"
version = "0.1.0"
edition = "2021"

[lib]
name = "srow_agent_lib"
crate-type = ["lib", "cdylib", "staticlib"]

[[bin]]
name = "srow-agent"
path = "src/main.rs"

[dependencies]
# Tauri 核心
tauri = { version = "2", features = ["protocol-asset"] }
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
tauri-plugin-notification = "2"
tauri-plugin-window-state = "2"

# IPC 类型生成（配合前端 tauri-specta）
specta = { version = "2", features = ["derive"] }
tauri-specta = { version = "2", features = ["derive", "typescript"] }

# 异步运行时
tokio = { version = "1", features = ["full"] }

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 工具
uuid = { version = "1", features = ["v4"] }
thiserror = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# 时间（Sub-1 只用于 created_at）
chrono = { version = "0.4", features = ["serde"] }

[build-dependencies]
tauri-build = { version = "2", features = [] }

[profile.dev]
incremental = true

[profile.release]
lto = "thin"
codegen-units = 1
panic = "abort"
opt-level = 3
strip = true
```

### 6.2 package.json（前端根）

```json
{
  "name": "srow-agent-frontend",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "tauri": "tauri"
  },
  "dependencies": {
    "react": "^19.0.0",
    "react-dom": "^19.0.0",
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-shell": "^2",
    "@tauri-apps/plugin-dialog": "^2",
    "@tauri-apps/plugin-fs": "^2",
    "@tauri-apps/plugin-notification": "^2",
    "zustand": "^5",
    "clsx": "^2",
    "tailwind-merge": "^2",
    "lucide-react": "^0.475"
  },
  "devDependencies": {
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4",
    "typescript": "^5",
    "vite": "^6",
    "@tauri-apps/cli": "^2",
    "tailwindcss": "^3",
    "autoprefixer": "^10",
    "postcss": "^8",
    "tauri-specta": "^2"
  }
}
```

**说明：shadcn/ui 不作为 npm 依赖安装，通过 CLI 按需拷贝组件文件到 `src/components/ui/`。**

---

## 7. Tauri 应用配置（tauri.conf.json 关键字段）

```jsonc
{
  "identifier": "com.smallraw.app.srow-agent",
  "productName": "Srow Agent",
  "version": "0.1.0",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "beforeBuildCommand": "pnpm build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "Srow Agent",
        "width": 1280,
        "height": 800,
        "minWidth": 900,
        "minHeight": 600,
        "decorations": true,
        "transparent": false,
        "resizable": true,
        "center": true
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

---

## 8. capabilities/default.json（Tauri 2.x 权限声明）

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Default capability for Srow Agent",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-start-dragging",
    "core:window:allow-set-title",
    "shell:allow-open",
    "dialog:allow-open",
    "dialog:allow-save",
    "fs:allow-read-text-file",
    "fs:allow-write-text-file",
    "fs:allow-read-dir",
    "fs:allow-create-dir",
    "notification:default"
  ]
}
```

---

## 9. 状态管理设计（前端）

### 9.1 workspaceStore

```typescript
interface WorkspaceState {
  workspaces: Workspace[];
  selectedWorkspaceId: string | null;
  sessions: Session[];                  // 当前 workspace 的 sessions
  selectedSessionId: string | null;

  // Actions
  loadWorkspaces: () => Promise<void>;
  selectWorkspace: (id: string) => Promise<void>;
  selectSession: (id: string) => void;
  createWorkspace: (name: string, path: string) => Promise<void>;
  createSession: (name: string) => Promise<void>;
}
```

### 9.2 chatStore

```typescript
interface ChatState {
  // key: session_id → messages
  messages: Record<string, Message[]>;
  draft: string;

  // Actions
  loadMessages: (sessionId: string) => Promise<void>;
  sendMessage: (sessionId: string, content: string) => Promise<void>;
  appendMessage: (sessionId: string, message: Message) => void;  // 事件触发
  setDraft: (text: string) => void;
}
```

### 9.3 agentStore

```typescript
interface AgentState {
  // key: session_id → AgentStatus
  statuses: Record<string, AgentStatus>;

  // Actions
  loadStatuses: () => Promise<void>;
  updateStatus: (status: AgentStatus) => void;  // 事件触发
}
```

---

## 10. IPC 封装（src/ipc/）

### 10.1 commands.ts（invoke 封装）

```typescript
// 由 tauri-specta 自动生成类型，此处展示手写版结构
import { invoke } from '@tauri-apps/api/core';
import type { Workspace, Session, Message, AgentStatus } from '../types';

// Workspace
export const workspaceList = (): Promise<Workspace[]> =>
  invoke('workspace_list');

export const workspaceCreate = (name: string, path: string): Promise<Workspace> =>
  invoke('workspace_create', { name, path });

export const workspaceDelete = (workspaceId: string): Promise<void> =>
  invoke('workspace_delete', { workspaceId });

// Session
export const sessionList = (workspaceId: string): Promise<Session[]> =>
  invoke('session_list', { workspaceId });

export const sessionCreate = (workspaceId: string, name: string): Promise<Session> =>
  invoke('session_create', { workspaceId, name });

export const sessionDelete = (sessionId: string): Promise<void> =>
  invoke('session_delete', { sessionId });

// Message
export const messageList = (
  sessionId: string,
  limit?: number,
  beforeId?: string,
): Promise<Message[]> =>
  invoke('message_list', { sessionId, limit, beforeId });

export const messageSend = (sessionId: string, content: string): Promise<Message> =>
  invoke('message_send', { sessionId, content });

// Agent Status
export const agentStatusGet = (): Promise<AgentStatus[]> =>
  invoke('agent_status_get');
```

### 10.2 events.ts（listen 封装）

```typescript
import { listen } from '@tauri-apps/api/event';
import type { AgentStatus, Message, Session } from '../types';

export const onAgentStatusChanged = (
  cb: (payload: { session_id: string; status: AgentStatus }) => void,
) => listen('agent-status-changed', e => cb(e.payload as any));

export const onMessageReceived = (
  cb: (payload: { session_id: string; message: Message }) => void,
) => listen('message-received', e => cb(e.payload as any));

export const onSessionUpdated = (
  cb: (payload: { session: Session }) => void,
) => listen('session-updated', e => cb(e.payload as any));
```

---

## 11. Rust 错误类型

```rust
// src/types/mod.rs 或独立 error.rs
#[derive(Debug, thiserror::Error, serde::Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum SrowError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("io error: {0}")]
    Io(String),
}

impl From<std::io::Error> for SrowError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
```

---

## 12. Sub-1 交付标准

| 验收项 | 说明 |
|---|---|
| `cargo build` 通过 | Rust 后端无编译错误 |
| `pnpm dev` + Tauri 启动 | 主窗口可以打开 |
| 三栏布局渲染 | 左/中/右三栏正常显示，无空白错位 |
| Workspace/Session 可操作 | 可通过 UI 创建、选中（数据存内存，不需要持久化） |
| 消息可发送（mock 回复） | 用户输入后显示用户消息，1 秒后显示占位回复 |
| Agent 状态指示灯 | 右侧面板显示当前 session 的状态灯（初始为 offline） |
| IPC 类型生成 | tauri-specta 导出 TypeScript 类型，无类型错误 |

---

## 13. 不在 Sub-1 范围内

以下内容明确推迟到后续 Sub：

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
| 国际化（i18n）| 视产品需求 |

---

## 14. 实现注意事项

### React 状态使用规范

遵循项目 React 规范（见全局规则 react-rules.md）：

- `agentStore.statuses` 是纯 UI 状态，用 Zustand（底层 `useState`）管理，变更触发组件重渲染
- 事件监听器（`listen()`）在 `useEffect` 中注册，返回的 unlisten 函数在清理时调用
- `useEffect` 中访问 store 状态用函数式更新或 `useRef` 保持引用，避免闭包陷阱

```typescript
// 正确：在 useEffect 中订阅 Tauri 事件
useEffect(() => {
  let unlisten: (() => void) | null = null;

  onAgentStatusChanged(({ session_id, status }) => {
    // 函数式更新，避免闭包陷阱
    useAgentStore.getState().updateStatus(status);
  }).then(fn => { unlisten = fn; });

  return () => { unlisten?.(); };
}, []); // 只在挂载时注册一次
```

### Rust 状态注入

`AppState` 通过 `tauri::Builder::manage()` 注入，所有 command handler 通过 `State<AppState>` 访问：

```rust
// main.rs
tauri::Builder::default()
    .manage(AppState::new())
    .invoke_handler(tauri::generate_handler![
        workspace_list,
        workspace_create,
        workspace_delete,
        session_list,
        session_create,
        session_delete,
        session_get,
        message_list,
        message_send,
        agent_status_get,
    ])
    .run(tauri::generate_context!())
    .expect("failed to run app");
```

Sub-1 阶段 `AppState` 内部用 `tokio::sync::RwLock<WorkspaceStore>` 管理内存数据，Sub-2 接管后替换为 SQLite 连接池。
