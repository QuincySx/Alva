# dialogs
> 模态弹窗视图集合，承载 Agents 管理、Skills 管理和应用设置的弹出交互界面。

## 地位
位于 `alva-app/views` 下的子模块，属于纯 UI 展示层。依赖 GPUI 和 gpui-component 的 Dialog/Button/Input 等组件，以及 `crate::models`（SettingsModel）和 `crate::theme`。被上层 Sidebar 或其他触发入口通过 `open_*_dialog()` 函数调用。

## 逻辑
1. 每个 dialog 文件提供一个 `open_*_dialog()` 公开函数，负责创建 Dialog 实例并注入到 GPUI 窗口。
2. `AgentsDialogContent` 和 `SkillsDialogContent` 共享相似的 list/edit 双模式交互模式——列表模式展示条目，点击后进入编辑模式。
3. `settings_dialog` 将 `SettingsPanel` 嵌入 Dialog 容器，委托具体设置逻辑给 settings_panel 模块。
4. `mod.rs` 作为 barrel 模块统一 re-export 三个 `open_*_dialog` 函数。

## 约束
- Dialog 内容组件（`AgentsDialogContent`、`SkillsDialogContent`）不对外暴露，只暴露 `open_*_dialog()` 函数。
- 弹窗不应持有长生命周期状态；关闭后资源应随 GPUI Entity 生命周期自动释放。
- 样式统一通过 `crate::theme::Theme` 获取，不要硬编码颜色值。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Agents 弹窗 | agents_dialog.rs | Agent 列表/编辑管理弹窗，提供 `open_agents_dialog()` |
| Skills 弹窗 | skills_dialog.rs | Skill 列表/编辑管理弹窗，提供 `open_skills_dialog()` |
| Settings 弹窗 | settings_dialog.rs | 将 SettingsPanel 嵌入 Dialog，提供 `open_settings_dialog()` |
| Barrel 导出 | mod.rs | 聚合并 re-export 三个 `open_*_dialog` 入口函数 |
