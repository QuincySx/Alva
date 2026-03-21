# agent_panel
> 右侧 Agent 状态/设置/预览 三标签面板

## 地位
在 `views` 中作为右侧信息栏，展示当前 Agent 运行状态、内嵌 SettingsPanel 以及预留的 Preview 面板。

## 逻辑
- `AgentPanel` 持有 `AgentModel`、`WorkspaceModel` 和 `SettingsPanel` Entity。
- 顶部 Tab 栏切换 Status / Settings / Preview 视图。
- Status 视图读取 `AgentModel` 中当前选中 session 的状态，并列出所有 session 的状态指示器。
- Settings 视图直接嵌入 `SettingsPanel` Entity。
- 订阅 `AgentModel` 和 `WorkspaceModel` 事件以触发重绘。

## 约束
- 未实现 Preview 功能（占位文字）。
- 依赖 `settings_panel` 子模块，形成跨面板引用。
- Tab 切换为本地状态，不持久化。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| AgentPanel | `agent_panel.rs` | 三标签面板：Status 展示 agent 运行状态，Settings 内嵌设置面板，Preview 占位 |
| mod | `mod.rs` | 桶模块，re-export AgentPanel |
