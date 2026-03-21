# models
> GPUI 响应式状态模型层

## 地位
在 `srow-app` crate 中承担所有共享状态管理，是 View 和 EngineBridge 之间的数据中枢。

## 逻辑
- 所有 model 实现 `EventEmitter`，变更时 emit 事件 + `cx.notify()` 驱动订阅者重绘。
- `WorkspaceModel`：管理 sidebar_items（GlobalSession / Workspace 列表）和 selected_session_id。
- `ChatModel`：按 session_id 存储消息列表、streaming_buffer、thinking_buffer、draft。
- `AgentModel`：按 session_id 存储 AgentStatus（Idle/Running/Error/WaitingHitl/Offline）。
- `SettingsModel`：包装 `AppSettings`，提供 load/save 到 `~/.srow/settings.json`。
- Model 在 `main.rs` 中由 `cx.new()` 创建为 GPUI Entity，Entity 引用分发给各 View 和 Bridge。

## 约束
- WorkspaceModel 初始数据为硬编码 mock（mock_sidebar_items）。
- ChatModel 和 AgentModel 为纯内存，无持久化。
- SettingsModel 持久化到 JSON 文件，无加密。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| WorkspaceModel | `workspace_model.rs` | 侧栏数据：SidebarItem 列表、session 选中/workspace 展开 |
| ChatModel | `chat_model.rs` | 聊天消息：按 session 存储消息、streaming/thinking buffer |
| AgentModel | `agent_model.rs` | Agent 运行状态：按 session 存储 AgentStatus |
| SettingsModel | `settings_model.rs` | 应用配置：LLM、Proxy、Theme，JSON 持久化 |
| mod | `mod.rs` | 桶模块，re-export 所有 model |
