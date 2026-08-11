# 01 — 建立 gpui-component UI 基线

**Status:** planned

## Goal

先建立统一的组件和视觉规范，再重构 Hub 与 ALVA App 页面，避免继续用裸 `div()` 重造标准控件。

## Scope

- 使用 `Sidebar`、`Button`、`Input`、`TabBar`、`Tag`、`Avatar`、`Collapsible`、`Sheet`、`Tooltip`、`Spinner/Skeleton`；
- 定义 Hub 的 spacing、圆角、字号、弱化文字、卡片层级和交互态；
- 标准控件使用组件库，聊天气泡和对话布局保留为产品自定义组件；
- 建立 light/dark theme 与 keyboard focus 的验收清单。
- 在 `alva-hub-gpui` 与 `alva-app-gpui` 两套 UI 中执行同一套组件替换规则；

## Acceptance

- Sidebar、状态标签、折叠区、Sheet、Tab 和 loading state 不再由裸 `div()` 模拟；
- hover、focus、disabled、loading、selected 状态统一；
- 在最小窗口尺寸下无重叠、截断或不可操作控件。
- `PARITY-CHECKLIST.md` 中两套应用的组件库缺口全部转为 migrated，或有明确后续 ticket。

**Blocked by:** 00 — 冻结旧 Tauri 对照并建立迁移清单.
