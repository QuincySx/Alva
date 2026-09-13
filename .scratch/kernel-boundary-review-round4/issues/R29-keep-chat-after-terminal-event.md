# R29: 终态事件到达后保留用户问题与回答

**Priority:** P1
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；UI01、UI04
**Blocked by:** 无
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

crates/alva-hub-desktop/web/src/App.tsx:1728–1747 对整个 HubTimeline 做终态条件隐藏；该组件同时拥有用户/助手消息。live completed 后 attemptIsLive 被置 false，整个正文被卸载。注意恢复历史通常投影为 assistant_message，不能笼统声称所有完成历史均不可见；本轮以 live terminal 专项复现为准。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

把消息阅读流和执行诊断组件拆开；仅将执行图移到 Inspector，不按运行终态移除消息。保持失败/审批必要动作可达。

## 验收

- [x] 受控 bridge 发用户消息、回答、completed，正文从流式到完成始终可读。
- [x] cancelled/interrupted/exhausted/failed 分别覆盖，已产生正文不消失。
- [x] 切换/重开完成历史仍可读；保留本轮历史恢复通过的控制用例。

## 执行记录（执行者填写）

- Commit：`ac39c38`。
- 专项复现/回归命令、版本（vitest 2.1.9，jsdom；2026-09-13 全量复跑 93/93 通过，HEAD `92967d0`）：
  - `npm test -- src/App.integration.test.tsx` — 34/34。R29 专项（ac39c38 新增 +229 行）：live completed/cancelled/interrupted/exhausted/failed 终态事件到达后，ChatTranscript 中用户问题与助手回答仍在文档中（复审"追加 live completed 后的正文保留 1 failed"的同一场景现通过）；mergeLiveTimeline 不再用终态 status caption 覆盖已流式正文。
  - 复审对照探针"完成历史重新打开"保持通过（components.test.tsx / App 集成）。
- 未完成项：无。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
