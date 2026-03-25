# AI 自验证闭环设计

> 让 AI 在开发过程中能自主验证、发现问题、修复、再验证，形成完整的自驱闭环。

## 目标

AI 写完代码后能自己跑通验证：

```
写代码 → 验证 → 发现问题 → 修复 → 再验证 → 直到对
```

分两阶段交付：
- **Phase A**：业务逻辑自验证 — headless 测试 + `cargo test`
- **Phase B**：UI 自动化 — 启动真实 app、控制操作、检查状态、截图判断

---

## Phase A：业务逻辑自验证基建

### 工作流

TDD 风格：写测试 → 跑失败 → 写实现 → 跑通过 → 重构。

### 1. `alva-test` crate — 共享测试工具箱

新增 crate，只作为 `[dev-dependencies]` 被引用，不进生产代码。

```
crates/alva-test/
├── src/
│   ├── lib.rs
│   ├── mock_provider.rs    # MockLanguageModel
│   ├── mock_tool.rs        # MockTool
│   ├── fixtures.rs         # 工厂函数
│   ├── assertions.rs       # 领域断言宏
│   └── gpui_helpers.rs     # GPUI 测试辅助：setup TestAppContext globals
```

**MockLanguageModel** 能力：
- 预设响应序列（第 N 次调用返回什么）
- 模拟流式 token（逐 token yield）
- 模拟错误（超时、rate limit）
- 记录收到的 messages 和 tools，用于断言

**MockTool** 能力：
- 可编程返回值
- 记录调用参数历史
- 模拟延迟和失败

**GPUI 测试辅助**（L2/L3 测试需要）：
- `setup_test_app(cx)` — 设置 TestAppContext 所需的全局状态（SharedRuntime 等）
- `make_chat_panel(cx, mock_model)` — 工厂函数，创建注入 mock 依赖的 Entity
- 各组件的工厂函数按需添加，避免每个测试文件重复搭环境

### 2. 三层测试结构

遵循测试金字塔，密度从底到顶递减。

**L1 纯逻辑单元测试** — 每个非 trivial 函数都测

位置：各 crate 内部 `#[cfg(test)] mod tests`

```rust
#[test]
fn search_filters_messages_case_insensitive() {
    let msgs = vec![make_message("Hello"), make_message("world")];
    let results = search(&msgs, "hello");
    assert_eq!(results.len(), 1);
}
```

**L2 组件状态测试** — 每个组件的每个功能都测

位置：各 crate 内部，使用 `#[gpui::test]` 或 `#[tokio::test]`

```rust
#[gpui::test]
fn test_chat_panel_search(cx: &mut TestAppContext) {
    // 创建 ChatPanel entity with mock dependencies
    // 调用 panel.search("hello")
    // 断言 panel.filtered_messages() 长度和内容
}
```

通过 headless GPUI window 创建真实 Entity，注入 mock 依赖，调方法，断言状态。不测渲染，只测状态流转。

**L3 关键路径集成测试** — 只测核心跨组件交互

位置：`tests/` 目录

```rust
#[gpui::test]
fn test_session_switch_resets_search(cx: &mut TestAppContext) {
    // 创建 Sidebar + ChatPanel
    // ChatPanel 设置搜索状态
    // Sidebar 切换 session
    // 断言 ChatPanel 搜索状态已重置
}
```

防止"各组件单独测试通过但接在一起出问题"的集成 bug。

### 3. 测试覆盖优先级

| 优先级 | 目标 | 说明 |
|--------|------|------|
| P0 | alva-core agent loop | 核心引擎：消息处理、工具调用、middleware |
| P0 | alva-app-core base_agent | Facade 层的 agent 组装和运行 |
| P1 | alva-app GpuiChat | UI 状态机：发送/接收/错误/取消 |
| P1 | alva-app-core skills | Skill 加载、注入、中间件 |
| P2 | alva-app 其他组件 | Sidebar、Settings 等 |

### 4. 测试规范

- 新功能必须先写测试（TDD）
- L1/L2 测试与源文件同目录，L3 在 `tests/` 下
- Mock 统一从 `alva-test` 导入，不各 crate 自己造
- 测试命名：`test_<组件>_<行为>_<场景>`

---

## Phase B：UI 自动化

### 架构总览

```
Claude CLI
  │
  ├─ cargo test                    ← Phase A
  │
  └─ MCP: srow-devtools           ← Phase B
       │
       └─ HTTP → alva-app-debug (in app process, port 9229)
              │
              └─ mpsc channel → GPUI main thread
```

alva-app-debug 扩展核心能力（HTTP API），上面包一层薄 MCP adapter 让 AI 直接调工具。

### 1. alva-app-debug 新增 HTTP Endpoints

#### Action Dispatch — 控制 app

```
POST /api/action
Body: { "target": "chat_panel", "method": "send_message", "args": {"text": "hello"} }
Response: { "ok": true, "result": ... }

Error responses:
{ "ok": false, "error": "target_not_found", "message": "Entity 'chat_panel' not registered or has been dropped" }
{ "ok": false, "error": "method_not_found", "message": "Method 'foo' not found on 'chat_panel'" }
{ "ok": false, "error": "invalid_args", "message": "Failed to deserialize args: ..." }
{ "ok": false, "error": "execution_failed", "message": "..." }
```

##### ActionRegistry 设计

核心问题：`WeakEntity<T>` 是泛型的，不同组件类型不能存在同一个 HashMap 里。解法是**类型擦除** — 注册时将 WeakEntity 和方法分发逻辑一起捕获进闭包。

```rust
/// 类型擦除的 action 闭包：接收 method + args，在 GPUI 线程内执行
type ActionFn = Box<dyn Fn(&str, Value, &mut App) -> Result<Value, String> + Send + Sync>;

/// 类型擦除的 state 闭包：在 GPUI 线程内读取状态
type StateFn = Box<dyn Fn(&mut App) -> Option<Value> + Send + Sync>;

struct RegisteredView {
    action_fn: ActionFn,
    state_fn: StateFn,
    methods: Vec<String>,  // 可用方法列表，用于 srow_views() 发现
}

struct ActionRegistry {
    views: RwLock<HashMap<String, RegisteredView>>,
}
```

组件注册示例：

```rust
// ChatPanel 注册时，将 WeakEntity<GpuiChat> 捕获进闭包
let weak = cx.entity().downgrade();  // WeakEntity<GpuiChat>
registry.register("chat_panel", RegisteredView {
    action_fn: Box::new(move |method, args, cx| {
        weak.update(cx, |chat, cx| {
            match method {
                "send_message" => {
                    let text: String = serde_json::from_value(args["text"].clone())?;
                    chat.send_message(&text, cx);
                    Ok(Value::Null)
                }
                _ => Err(format!("unknown method: {method}"))
            }
        }).ok_or_else(|| "entity dropped".to_string())?
    }),
    state_fn: Box::new(move |cx| {
        weak_clone.read_with(cx, |chat, _| chat.debug_state()).ok()
    }),
    methods: vec!["send_message".into(), "clear".into()],
});
```

##### 请求生命周期

```
HTTP thread (tiny_http):
  1. 解析 POST /api/action JSON body
  2. 发送 (target, method, args, oneshot_tx) 到 mpsc channel
  3. 阻塞等待 oneshot_rx 响应（带超时）
  4. 序列化为 HTTP JSON response

GPUI foreground task (长驻，app 启动时 cx.spawn 创建):
  1. 循环 recv from mpsc channel
  2. 查找 registry.views[target]
  3. 调用 action_fn(method, args, cx)
  4. 通过 oneshot_tx 返回 Result
```

注意：这里的 GPUI drain task 是 **app 启动时创建的长驻 foreground task**（通过 `cx.spawn` 一次性创建），不是每次 HTTP 请求创建新的 spawn。它持续从 mpsc channel 读取请求并执行。

新 view 只要注册到 ActionRegistry，AI 就能自动发现和操作，不需要硬编码。

#### State Dump — 检查状态

```
GET /api/inspect/state?view=chat_panel
Response: {
  "view": "chat_panel",
  "state": {
    "messages_count": 5,
    "is_loading": false,
    "search_query": "",
    "current_session_id": "xxx"
  }
}
```

实现机制：
- 每个组件实现 `DebugState` trait：`fn debug_state(&self) -> serde_json::Value`
- 定义在 `alva-app-debug` crate 中，替代现有的 `DebugInspect` trait（合并功能）
- state_fn 已在 ActionRegistry 注册时一并提供（见上方 RegisteredView 定义）
- 请求走同样的 mpsc → GPUI thread → oneshot 路径（因为读状态也需要 GPUI Context）
- 比现有 `inspect/tree`（只有静态元数据）多了实时业务状态

`DebugState` trait：

```rust
/// 定义在 alva-app-debug crate
pub trait DebugState {
    fn debug_state(&self) -> serde_json::Value;
}
```

组件实现约定：导出所有对 AI 验证有用的字段。遗漏字段会导致验证盲区 — 新增功能的 PR checklist 应包含"DebugState 是否更新"。

#### Navigation — 导航

不单独实现。导航就是 action dispatch 的特例 — 调用 RootView 的状态切换方法：

```
POST /api/action
Body: { "target": "root_view", "method": "navigate", "args": {"panel": "settings"} }
```

#### Screenshot — 截图

```
POST /api/screenshot
Response: { "path": "/tmp/alva-app-debug-screenshot-1711234567.png" }
```

GPUI 没有截图 API，走 macOS 外部方案：
- debug server 通过 `core-graphics` crate 调用 `CGWindowListCopyWindowInfo` 获取自身进程的 window ID
- 调用 `screencapture -l <windowID> -o <path>` 截图
- 返回图片路径，AI 用多模态能力查看

前置条件：macOS 需授予 Screen Recording 权限（System Preferences > Privacy > Screen Recording），否则截图为空白。首次使用时应提示用户授权。

注意：截图功能仅限开发机本地使用，无法在 headless CI 环境运行。

#### Shutdown — 优雅关闭

```
POST /api/shutdown
Response: { "ok": true }
```

通过 channel 通知 GPUI 主线程调 `cx.quit()`。

### 2. alva-app-devtools-mcp — 薄 MCP 适配层

新增 crate，把 alva-app-debug HTTP API 暴露为 MCP 工具：

```
crates/alva-app-devtools-mcp/
├── src/
│   ├── lib.rs
│   ├── server.rs       # MCP server 实现
│   └── tools.rs        # Tool 定义，转发到 HTTP API
```

传输方式：stdio（stdin/stdout JSON-RPC），这是 Claude MCP 的标准传输。MCP server 进程内部是 HTTP client，转发到 `127.0.0.1:9229`。

使用 `rmcp` crate（Rust MCP SDK）实现 MCP server 协议。

工具列表：

| MCP Tool | 对应 HTTP | 用途 |
|----------|----------|------|
| `srow_action` | `POST /api/action` | 对组件执行操作 |
| `srow_inspect` | `GET /api/inspect/state` | 读取组件实时状态 |
| `srow_screenshot` | `POST /api/screenshot` | 窗口截图 |
| `srow_shutdown` | `POST /api/shutdown` | 优雅关闭 app |
| `srow_views` | `GET /api/inspect/views` | 列出所有已注册 view 及其可用 method（新 endpoint，非 inspect/tree） |

### 3. 完整自验证闭环

```
AI 写代码
  │
  ├─ cargo test                        ← Phase A: 业务逻辑
  │
  ├─ cargo run -p alva-app &           ← 启动 app
  │
  ├─ srow_views()                      ← 发现可用 view 和 action
  ├─ srow_inspect("chat_panel")        ← 状态对不对
  ├─ srow_action(target, method, args) ← 模拟操作
  ├─ srow_inspect(...)                 ← 操作后状态对不对
  ├─ srow_screenshot()                 ← 看起来对不对（多模态视觉判断）
  │
  ├─ 发现问题 → 修代码 → 重新验证
  │
  └─ srow_shutdown()                   ← 关掉 app
```

---

## GPUI headless 测试边界

明确 Phase A 和 Phase B 的能力分界：

| 能力 | Phase A (headless) | Phase B (UI 自动化) |
|------|-------------------|-------------------|
| 组件逻辑测试 | 直接调方法，断言状态 | — |
| 组件间集成测试 | 创建多组件，断言数据流转 | — |
| 模拟用户操作（点击/输入） | 不能 | action dispatch |
| 检查实时 UI 状态 | 不能 | state dump |
| 视觉验证 | 不能 | screenshot + 多模态 |
| 导航页面 | 不能（可程序化调方法） | action dispatch |

Phase A 测"调方法后状态对不对"，Phase B 测"真实 app 运行时行为和视觉对不对"。

---

## 新增 Crate 清单

| Crate | 层级 | 用途 |
|-------|------|------|
| `alva-test` | 框架层（dev-only） | 共享 mock 和测试工具 |
| `alva-app-devtools-mcp` | 产品层 | MCP adapter，转发到 alva-app-debug |

---

## App 重启周期

AI 修改代码后需要重新验证，涉及 app 重启：

1. `srow_shutdown()` 优雅关闭 — GPUI `cx.quit()` 会释放资源和端口
2. 如果 shutdown 超时（3s），fallback 到 kill PID
3. 重新 `cargo run -p alva-app &` 启动
4. 轮询 `GET /api/health` 直到返回 200（最多等 10s），确认 debug server 就绪
5. 继续 MCP 工具调用

MCP server 应优雅处理连接中断（app 进程退出时 HTTP 连接断开），返回明确错误而非 hang。

---

## 阶段完成标准

**Phase A 完成标准：**
- alva-test crate 就绪，MockLanguageModel + MockTool 可用
- alva-core agent loop 有 L1+L2 测试覆盖
- alva-app-core base_agent 有 L2 测试覆盖
- AI 能通过 `cargo test` 自验证新功能

**Phase B 完成标准：**
- ActionRegistry + GPUI drain task 工作
- 至少 3 个组件（RootView、GpuiChat、Sidebar）注册了 action 和 state
- srow_action / srow_inspect / srow_screenshot MCP 工具可用
- AI 能完成一次完整的"启动 app → 操作 → 检查 → 截图 → 关闭"循环

---

## 约束

- `alva-test` 只在 `[dev-dependencies]`，不进生产二进制
- alva-app-debug 新 endpoint 全部 `#[cfg(debug_assertions)]`，release 无痕
- MCP server 只在开发时启动
- 截图走 macOS `screencapture`，不修改 GPUI 内部
- ActionRegistry 使用类型擦除闭包（Send/Sync safe），不持有 GPUI Context
- tiny_http 单线程处理，同时只有一个 action 请求在执行（对 AI 单线程调用够用）
- 截图功能仅限开发机本地，不可在 headless CI 运行
