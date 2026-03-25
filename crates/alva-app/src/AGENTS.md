# alva-app
> GPUI 桌面应用，提供 Sidebar、ChatPanel、Agent 管理等交互界面

## 地位
用户交互层。基于 GPUI 框架构建桌面 UI，包含会话管理侧边栏、聊天面板（Markdown 渲染、代码块、工具调用展示）、Agent/Skill/Settings 对话框、主题系统等。通过 models 层与 alva-app-core 引擎通信。

## 逻辑
```
main.rs → RootView
             ├─→ Sidebar
             │     ├─→ SessionList (会话列表)
             │     └─→ ManagementButtons (Agent/Skill/Settings 快捷按钮)
             ├─→ ChatPanel
             │     ├─→ MessageList → MessageBubble / AgentBlock / ThinkingBlock / ToolCallBlock
             │     ├─→ InputBox (用户输入)
             │     ├─→ RunningAgentsZone (运行中 Agent 状态)
             │     ├─→ Markdown 渲染 + CodeBlock 代码高亮
             │     └─→ chat/ (GpuiChat + GpuiChatState 状态管理)
             ├─→ AgentDetailPanel (Agent 详情)
             ├─→ SettingsPanel (设置面板)
             └─→ Dialogs (AgentsDialog / SkillsDialog / SettingsDialog)

models/
  ├─→ WorkspaceModel (工作区状态)
  ├─→ AgentModel (Agent 数据管理)
  ├─→ ChatModel (聊天状态 & 消息管理)
  └─→ SettingsModel (用户设置)

theme.rs → 主题系统（语义色 + 暗色/亮色主题）
```

## 约束
- 所有 UI 组件基于 GPUI 框架的 Render trait
- 状态管理通过 gpui::Model + Entity 模式
- debug 构建时通过 DebugViewRegistry 注册视图供调试服务器检查
- 主题颜色使用语义化命名，不直接使用硬编码色值

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 根视图 | `views/root_view.rs` | RootView 顶层布局容器 |
| 侧边栏 | `views/sidebar/` | Sidebar、SessionList、ManagementButtons |
| 聊天面板 | `views/chat_panel/` | ChatPanel、MessageList、MessageBubble、InputBox、AgentBlock、ThinkingBlock、ToolCallBlock、RunningAgentsZone、Markdown、CodeBlock |
| Agent 详情 | `views/agent_detail_panel.rs` | AgentDetailPanel 展示 Agent 运行详情 |
| 设置面板 | `views/settings_panel/` | SettingsPanel 用户设置界面 |
| 对话框 | `views/dialogs/` | AgentsDialog、SkillsDialog、SettingsDialog |
| 聊天引擎 | `chat/` | GpuiChat 聊天控制器 + GpuiChatState 状态管理 |
| 数据模型 | `models/` | WorkspaceModel、AgentModel、ChatModel、SettingsModel |
| 类型 | `types/` | Workspace、Agent 等 UI 层类型定义 |
| 主题 | `theme.rs` | 主题系统（语义色、暗色/亮色切换） |
| 错误 | `error.rs` | UI 层错误类型 |
