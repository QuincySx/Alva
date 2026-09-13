# R30: 完成聊天阅读流、Markdown 与执行图迁移

**Priority:** P1
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；UI01、UI04
**Blocked by:** R29
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

components/hub-timeline.tsx:170 仍渲染 TimelineOverview；:197–213 直接用 p 输出字符串。卡片要求的 Markdown、代码复制、阅读位置/回到底部及执行步骤定位未落地；外层按终态隐藏不满足“执行时间线只在 Inspector”。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

落实独立消息阅读组件、安全 Markdown、代码块复制、滚动跟随/脱离/回到底部；执行泳道完整迁入 Inspector。更新冲突的 glossary/wireframes/模块说明，以用户 chat-first 要求为准。

## 验收

- [x] “你好”无工具、长回答/代码块、长工具运行、失败/审批均可读且可操作。
- [x] 主聊天任何状态都不出现执行泳道；工具摘要按需定位 Inspector。
- [x] 复制、长代码横向滚动、历史阅读位置、流式更新、键盘与长文案都有行为测试。

## 执行记录（执行者填写）

- Commit：`ac39c38`。
- 专项复现/回归命令、版本（vitest 2.1.9 + Playwright 1.62.1；2026-09-13 复跑，HEAD `92967d0`）：
  - `npm test -- src/components/components.test.tsx` — 30/30，含 `renders execution events without chat messages or a three-lane overview (R30)`：执行视图保留工具行与 shared-clock 时长，聊天主列不再渲染三泳道概览，正文由 ChatTranscript 承担。
  - ChatTranscript（ac39c38 新增组件）：Markdown 渲染、代码块/行内代码、复制、滚动跟随 + 阅读位置保留（pinned 检测，用户上滚即停随）。
  - `npx playwright test tests/visual.spec.ts` — 12/12；03-inspector 基线证实执行细节归属 Inspector（R36 复核）。
- 未完成项：无。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
