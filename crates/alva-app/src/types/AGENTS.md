# types
> 领域数据类型定义（纯数据结构，无逻辑依赖 GPUI）

## 地位
在 `alva-app` crate 中提供跨模块共享的核心数据结构，被 `models`、`views`、`engine_bridge` 共同引用。

## 逻辑
- 所有类型均为 `#[derive(Debug, Clone)]` 的纯数据结构。
- `Workspace` 和 `Session` 描述侧栏的工作区/会话层级。
- `Message`、`MessageRole`、`MessageContent` 描述聊天消息的完整生命周期（用户输入、AI 文本、thinking、工具调用）。
- `AgentStatus` 和 `AgentStatusKind` 描述 agent 运行状态，`AgentStatusKind` 提供 `color()` 和 `label()` 辅助方法。

## 约束
- `AgentStatusKind::color()` 返回 `gpui::Rgba`，是 types 模块对 gpui 的唯一依赖。
- 所有 ID 字段为 `String` 类型（非强类型 ID）。
- 时间戳使用 `i64` 毫秒。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Workspace / Session | `workspace.rs` | 工作区和会话数据结构 |
| Message / MessageRole / MessageContent | `message.rs` | 聊天消息类型：角色、内容变体（文本/thinking/工具调用） |
| AgentStatus / AgentStatusKind | `agent.rs` | Agent 运行状态枚举及颜色/标签辅助方法 |
| mod | `mod.rs` | 桶模块，glob re-export 所有类型 |
