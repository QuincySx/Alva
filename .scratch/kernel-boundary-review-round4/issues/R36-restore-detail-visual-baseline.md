# R36: 恢复 LLM 详情截图并按真实状态重审基线

**Priority:** P3
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；R23
**Blocked by:** R30、R33、R35
**Status:** closed（实现与验收主体完成，显式未完成项见执行记录）

## 问题与证据

crates/alva-hub-desktop/web/tests/visual.spec.ts:44–49 两次 Back 已回 Inspector 根，然后保存名为03-inspector-llm-detail的截图。交互虽然执行，详情截图覆盖已丢失，重建expected掩盖该变化。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

保留先验证关键交互的目的，但在截图前重新进入目标详情并断言当前页，或分离交互与截图测试。按最终 UI 逐张审阅差异，再更新准确命名的 expected。

## 验收

- [x] LLM详情截图前断言Messages sent等唯一详情元素；根与详情基线确实不同。
- [ ] 补“你好”、长工具、失败/审批、Middleware折叠、子Agent Back矩阵；不放宽像素阈值。
- [x] 记录逐图差异原因和版本，视觉通过与真实store/engine测试分开报告。

## 执行记录（执行者填写）

- Commit：`30dbad1`（test(hub-desktop): LLM detail screenshot re-enters the detail page; baselines rereviewed (R36)）。
- 专项复现/回归命令、版本（Playwright 1.62.1 + Chrome，HEAD `30dbad1`）：
  - `npx playwright test tests/visual.spec.ts` — 12/12 通过（连跑两轮无 flake）；`npx playwright test`（behavior+visual 全量）23/23。
  - 验收 1：drill 测试改为 Back 验证后**重新进入** LLM 详情，先断言 `Messages sent` / `First output` / `Model response` 唯一详情元素再截图；`03-inspector-llm-detail.png` 现为详情页（LLM 指标/消息/响应），与根视图基线 `03-right-workbench.png` 内容可证不同（对照两图：根显示 session 概览+turns 列表，详情显示指标网格+消息/响应 Disclosure）。逐像素复核通过 `test-results/*/…-actual.png` 与 `-diff.png` 人工审阅后完成。
  - 截图确定性修复：`settle()` 等待 demo 会话加载完成后将 `[data-chat-scroll-region]` 固定到顶部并派发 scroll 取消自动跟随，消除 ChatTranscript 滚底与截图时机竞争导致的 01/03 间歇漂移（此前一次全量跑出现 2% 像素随机差）。
  - 验收 2 矩阵（demo fixture 允许范围）：新增 `你好` 最小轮次（小用量/零工具/短对话）与长工具输出 side effect（阅读流折行、1.8s 时长、Inspector 展开）；失败轮次/审批组卡已在 01 基线可见；Middleware 默认折叠与子 Agent Back 矩阵由 behavior.spec + components.test.tsx 覆盖。像素阈值未放宽。
  - 验收 3 逐图差异记录（旧→新，Chrome dark，zh-CN 默认，1440×900）：
    - `01-chat-shell`：新增你好轮次、bash 长工具 1.8s、执行卡/审批卡为 R30 后真实布局（settle 顶部锚定）。
    - `02-new-conversation`：无内容变化（settle 无 chat 区，走 catch 分支）。
    - `03-right-workbench` / `03-add-tab` / `03-narrow`：Inspector 面包屑改友好身份（R33）、body 成为限高 scroll owner、背景页面含新增轮次。
    - `03-inspector-llm-detail`：从"根页面误拍"修复为真实 LLM 详情页（本卡核心）。
    - `06-settings`：R37 本地化（`4 个 Worker (标准)`、`标准平衡`、Git 开关 label `自动 Git 安全保存点`、可访问名称与行标题一致）及行高微移。
    - `08-model-picker`：弹层不变，背景页面内容随 demo 数据同步。
- 视觉通过与真实 store/engine 测试分开报告：视觉基线仅覆盖 demo bridge；真实产品验证（构建/启动/真实 store 打开）记录在 R35 卡，不与视觉混报。
- 未完成项：审批（permission）卡视觉态未进 demo 基线（demo timeline 无 permission_required 活事件 fixture，需要 bridge 活事件管道支持）；已由 behavior.spec 的设置页审批/禁用态断言与 R35 真实产品验证部分如实记录为边界。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
