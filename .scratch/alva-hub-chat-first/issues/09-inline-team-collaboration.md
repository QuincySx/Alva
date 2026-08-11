# 09 — TeamRun / Worker 协作内联

**Status:** planned

## Goal

让 Lead 与 Workers 的协作过程留在同一个聊天上下文中，而不是跳转到独立团队面板。

## Scope

- WorkerTask 派发、排队、运行、结果和失败使用 inline card；
- 展示 worker engine/model、resource claim、worktree 和 lifecycle badge；
- 支持追加消息、取消、hibernate、resume、archive、adopt/discard；
- 聚合审批必须保留 worker 与资源范围；
- 详情继续通过 Sheet 展开，不占据主聊天流。

## Acceptance

- 多 worker 并行时聊天流顺序稳定、来源明确；
- 单 worker fallback 不影响 TeamRun 其他成员；
- worktree adoption 和 resource conflict 状态重启后可恢复；
- 原有 team runtime 测试继续通过。

**Blocked by:** 04 — 把运行操作内联进聊天流.
