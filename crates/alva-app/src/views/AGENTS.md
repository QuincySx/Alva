# views
> GPUI 视图层：三栏布局 + 各面板组件

## 地位
在 `alva-app` crate 中承担所有 UI 渲染职责，将 `models` 层的状态映射为 GPUI 可视元素。

## 逻辑
- `RootView` 是窗口的根视图，采用 flex-row 布局：
  - 左栏 `Sidebar`：新建任务 + 导航项 + 任务历史
  - 中栏 `ChatPanel`（flex-1）：消息列表 + 输入框 + Markdown 渲染
  - 右侧 `AgentDetailPanel`：滑出面板展示 Agent 详情
  - `SessionWelcome`：空会话时显示欢迎页
- 所有面板通过 GPUI Entity 引用共享 model，model 事件驱动 UI 重绘。
- 模态对话框（Agents/Skills/Settings）通过 `open_*_dialog()` 模式打开。

## 约束
- 所有视图使用 `Theme::for_appearance()` 解析当前主题颜色。
- 面板间无直接通信，全部经由共享 model 间接协调。
- GPUI 不支持 CSS/HTML；布局使用 `div()` builder API。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| RootView | `root_view.rs` | 窗口根视图，三栏 flex 布局 |
| chat_panel | `chat_panel/` | 中央面板：消息列表 + 输入框 + Markdown 渲染 + 代码块 |
| sidebar | `sidebar/` | 左侧导航：新建任务 + 导航项 + 任务历史 + 设置 |
| settings_panel | `settings_panel/` | 设置面板 |
| AgentDetailPanel | `agent_detail_panel.rs` | 右侧滑出面板：Agent 运行详情 |
| SessionWelcome | `session_welcome.rs` | 空会话欢迎页：Logo + 标题 + 输入框 + 快捷操作卡片 |
| dialogs | `dialogs/` | 对话框：AgentsDialog、SkillsDialog、SettingsDialog |
| mod | `mod.rs` | 桶模块，re-export RootView |
