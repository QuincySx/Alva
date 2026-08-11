# 04 — 把运行操作内联进聊天流

**Status:** planned

## Goal

审批、fallback、engine switch、readiness 和错误处理都在对话上下文内完成。

## Scope

- permission card：Allow、Deny、作用域和决策后静态状态；
- fallback/switch card：原因、目标、Stop、Switch back、View handoff；
- readiness/repair card：不可用原因和修复入口；
- 详情通过 `Sheet` 打开 attempts、handoff、diagnostics 和历史审批；
- 保持所有操作路由到正确 EngineBranch/WorkerSession。

## Acceptance

- 主流程不依赖独立 approval/fallback/readiness 面板；
- 所有按钮操作后，原卡片原地更新且重启后状态一致；
- handoff 正文继续经过敏感信息过滤；
- fallback exhausted 展示完整且有界的失败链。

**Blocked by:** 03 — 实现 Chat-First 主界面.
