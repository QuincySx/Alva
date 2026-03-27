# sidebar
> 左侧边栏视图，承载新建任务、功能导航、任务历史列表和设置入口。

## 地位
位于 `alva-app/views` 下的子模块，属于应用主布局的一级视图组件。依赖 `crate::models`（WorkspaceModel、ChatModel、AgentModel、SettingsModel）提供数据，依赖 `crate::theme` 获取样式，依赖 gpui-component 的 Button/Icon 等基础组件。被 `root_view` 或上层布局直接嵌入。

## 逻辑
1. `Sidebar` 是顶层 Entity，组合 new-task 按钮、导航项区域、任务历史列表和底部设置按钮。通过订阅多个 Model 的变化事件驱动 UI 更新。
2. `nav_items` 渲染固定的功能导航项（Search、Schedule、Skills、MCP），每项由 Lucide 图标 + 文字标签组成。
3. `TaskList` 是独立 Entity，订阅 `WorkspaceModel` 变化，渲染历史任务条目（名称、时长、状态），支持点击切换当前会话。

## 约束
- `Sidebar` 对外只暴露结构体本身，内部子组件（`TaskList`、`nav_items`）不对外公开。
- 导航项定义在 `nav_items.rs` 中集中管理，新增导航项只需修改此文件。
- 任务列表数据来源于 `WorkspaceModel`，`TaskList` 自身不持有业务数据，只做展示。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Sidebar 主组件 | sidebar.rs | 左侧边栏顶层 Entity，组合导航、任务列表和操作按钮 |
| 导航项 | nav_items.rs | 渲染 Search/Schedule/Skills/MCP 等固定导航条目 |
| 任务历史列表 | task_list.rs | 渲染历史任务条目，支持点击切换会话 |
| Barrel 导出 | mod.rs | 聚合子模块，re-export `Sidebar` |
