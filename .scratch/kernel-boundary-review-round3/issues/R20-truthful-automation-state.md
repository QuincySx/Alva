# R20: 自动化不能假成功或切页即丢失

**Priority:** P1
**Axis:** Hub 实测 / 产品完整性
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** 无
**Status:** ready-for-agent

## 问题与证据

原生 Tauri + 当前前端实测：自动化 → 新建标题 Review Sep11 persistence probe 和无副作用提示词 → 创建，显示“已配置 1 个”且启用；切到插件再回自动化，变成“已配置 0 个”。截图/tree 见 evidence/native/native-auto-created 与 native-auto-returned。
product-pages.tsx:86 初始化空 useState，:110–130 只更新组件内数组，无后端保存或 scheduler 接线。文案却承诺按时间运行，并显示启用，用户会误以为任务已安排。
本轮检查使用已有桌面构建加载当前 Vite 前端；此行为完全位于当前前端，最新 Rust 构建验证状态另列。

## 实现范围

先让生产入口诚实：在真实后端尚未支持时隐藏/禁用创建与启用，并明确未开放；若选择接入，则必须真正持久化且调度可观测。不要为过验收仅加 localStorage 却继续宣称会运行。本卡不要求扩张完整自动化产品。

## 验收

- [ ] 用户不能获得虚假的已创建/已启用反馈。
- [ ] 真实支持路径中，切页、刷新、重启保留任务和启用状态，计划到期产生可追踪运行结果。
- [ ] 未实现路径明确不可用，无误导性成功状态；demo fixture 只能在 demo/UI Lab 出现。
- [ ] 原生或真实后端 UI 回归覆盖切页/重启，不只组件内数组断言。

## 执行记录

- Commit：ed02c57 + cf18940
- 内容：自动化页诚实化：创建按钮禁用并注明"调度尚未开放"、启用开关与创建对话框移除、
模板标记预览、不再出现"已配置 N 个/已启用"假成功。
- 验证（2026-09-12 复跑）：`npx vitest run` 70/70 通过（含 R20 自动化、R21 权限设置相关组件测试）；`npm run build` 通过；`npx playwright test` 12/12 通过（04-automation / 05-plugins-skills / 06-settings 基线按诚实化后界面重建）。
- 实际验证命令及结果：`npx vitest run` 70/70 通过（含自动化/权限/插件组件测试）、`npm run build` 通过、`npx playwright test` 12/12 通过（04-automation/05-plugins-skills/06-settings 基线按诚实化界面重建，2026-09-12 复跑）。

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
