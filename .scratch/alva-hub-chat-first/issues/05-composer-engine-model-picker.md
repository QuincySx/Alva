# 05 — Composer Engine/Model 选择器

**Status:** planned

## Goal

在发送消息的位置完成 engine、model 和运行配置选择，不再依赖顶部运维面板。

## Scope

- 使用 `TabBar` 展示 Alva / Claude / Codex；
- 每个 engine 下提供可搜索 model 列表、最近使用和当前选择；
- composer 显示 `engine · model · permission profile`；
- 运行中切换按现有 queued-switch 语义处理；
- 不兼容或未认证模型显示原因，不只做灰色禁用。

## Acceptance

- 新 session 和已有 session 都能正确恢复选择；
- 选择结果进入 EngineTarget/attempt snapshot；
- keyboard 可打开、搜索、选择和关闭 picker；
- picker 只展示模型目录数据，不自行猜测兼容性。

**Blocked by:** 01 — 建立 gpui-component UI 基线; 03 — 实现 Chat-First 主界面.
