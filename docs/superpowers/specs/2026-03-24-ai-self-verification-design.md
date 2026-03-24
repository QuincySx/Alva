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
│   └── assertions.rs       # 领域断言宏
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
| P0 | srow-core base_agent | Facade 层的 agent 组装和运行 |
| P1 | srow-app GpuiChat | UI 状态机：发送/接收/错误/取消 |
| P1 | srow-core skills | Skill 加载、注入、中间件 |
| P2 | srow-app 其他组件 | Sidebar、Settings 等 |

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
       └─ HTTP → srow-debug (in app process, port 9229)
              │
              └─ mpsc channel → GPUI main thread
```

srow-debug 扩展核心能力（HTTP API），上面包一层薄 MCP adapter 让 AI 直接调工具。

### 1. srow-debug 新增 HTTP Endpoints

#### Action Dispatch — 控制 app

```
POST /api/action
Body: { "target": "chat_panel", "method": "send_message", "args": {"text": "hello"} }
Response: { "ok": true }
```

实现机制：
- app 启动时把关键 Entity 的 `WeakEntity<T>` 注册到 `ActionRegistry`（Send/Sync safe）
- `ActionRegistry` 维护 `HashMap<String, RegisteredEntity>`，每个 entry 包含 WeakEntity 和支持的方法列表
- debug server 收到请求 → 通过 mpsc channel 发给 GPUI 主线程的 foreground task
- foreground task 用 `entity.update(cx, |component, cx| ...)` 执行操作
- 复用 GpuiChat 里已有的 channel + cx.spawn 跨线程模式

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
- 注册时连同 WeakEntity 一起注册 state snapshot 闭包
- 比现有 `inspect/tree`（只有静态元数据）多了实时业务状态

#### Navigation — 导航

不单独实现。导航就是 action dispatch 的特例 — 调用 RootView 的状态切换方法：

```
POST /api/action
Body: { "target": "root_view", "method": "navigate", "args": {"panel": "settings"} }
```

#### Screenshot — 截图

```
POST /api/screenshot
Response: { "path": "/tmp/srow-debug-screenshot-1711234567.png" }
```

GPUI 没有截图 API，走 macOS 外部方案：
- debug server 获取自身 app 的 window ID
- 调用 `screencapture -l <windowID> -o <path>` 截图
- 返回图片路径，AI 用多模态能力查看

#### Shutdown — 优雅关闭

```
POST /api/shutdown
Response: { "ok": true }
```

通过 channel 通知 GPUI 主线程调 `cx.quit()`。

### 2. srow-devtools-mcp — 薄 MCP 适配层

新增 crate，把 srow-debug HTTP API 暴露为 MCP 工具：

```
crates/srow-devtools-mcp/
├── src/
│   ├── lib.rs
│   ├── server.rs       # MCP server 实现
│   └── tools.rs        # Tool 定义，转发到 HTTP API
```

MCP server 就是 HTTP client，转发到 `127.0.0.1:9229`。

工具列表：

| MCP Tool | 对应 HTTP | 用途 |
|----------|----------|------|
| `srow_action` | `POST /api/action` | 对组件执行操作 |
| `srow_inspect` | `GET /api/inspect/state` | 读取组件实时状态 |
| `srow_screenshot` | `POST /api/screenshot` | 窗口截图 |
| `srow_shutdown` | `POST /api/shutdown` | 优雅关闭 app |
| `srow_views` | `GET /api/inspect/tree` | 列出所有已注册 view 和可用 action |

### 3. 完整自验证闭环

```
AI 写代码
  │
  ├─ cargo test                        ← Phase A: 业务逻辑
  │
  ├─ cargo run -p srow-app &           ← 启动 app
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
| `srow-devtools-mcp` | 产品层 | MCP adapter，转发到 srow-debug |

---

## 约束

- `alva-test` 只在 `[dev-dependencies]`，不进生产二进制
- srow-debug 新 endpoint 全部 `#[cfg(debug_assertions)]`，release 无痕
- MCP server 只在开发时启动
- 截图走 macOS `screencapture`，不修改 GPUI 内部
- ActionRegistry 使用 WeakEntity（Send/Sync safe），不持有 GPUI Context
