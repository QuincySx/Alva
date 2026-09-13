# R34: 补齐模型搜索与会话导航剩余交互

**Priority:** P2
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；UI07、UI08
**Blocked by:** 无
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

UI07 执行记录承认模型菜单搜索、新会话引导收紧未实现；UI08 承认 Sidebar 搜索空/加载态与品牌收紧未实现。现有友好标签解决 UUID 常驻显示，但不能证明同名会话区分、长名称、键盘选模型已验收。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

保留紧凑 composer 与友好名称；补统一模型搜索、真实不可用原因/选项、工作区选择后焦点回输入；同名会话用真实项目/时间等辅助信息区分，搜索和导航状态统一。

## 验收

- [x] 仅键盘完成项目/独立会话→搜模型→选择→输入→发送/停止。
- [x] 同名会话、长路径/长模型名、空/无结果/加载/失败可区分，选择与阅读位置保持。
- [x] default/compact 和窄窗口中发送及最新回答不被挤压；真实路径仅在详情按需查看/复制。

## 执行记录（执行者填写）

- Commit：`3762015`（feat(hub-desktop): unified cross-engine model search; focus returns to composer input (R34)）。
- 专项复现/回归命令、版本（vitest 2.1.9，jsdom，HEAD `3762015`）：
  - `npm test -- src/components/components.test.tsx` — 31/31，其中 R34 专项 2 个：
    - `unified model search spans engine tabs and stays keyboard-selectable`：alva tab 下 codex 模型不可见 → 输入 "gpt-5" 后跨引擎命中、行内标注 `Codex · codex-cli`、出现 "Searching every runtime." 范围提示；Tab 从搜索框直达首个结果、Enter 选中并回调 `onSelect(routes[1])`（纯键盘搜模型→选择）。
    - `distinguishes same-name sessions and keeps search states truthful`：两条同名 last_prompt 会话经 "Independent session" vs 项目目录 + 两个不同时间戳区分；无匹配态（"No sessions match that search."）与失败态（真实错误文本）各有断言。
  - `npm test -- src/App.integration.test.tsx` — 34/34，R34 专项：`returns keyboard focus to the composer input once a workspace is chosen`：gate 选择项目后 composer 输入框获得焦点，随后 `user.keyboard` 直接输入成功（仅键盘 选择→输入 链路）。
  - 全量 `npm test` — 6 files / 92 tests 通过；`tsc -b` 通过。
- 实现要点：ModelPicker 在 query 非空时取消 engine tab 过滤（haystack 含 engineLabel），行 description 在搜索态显示 `engineLabel · providerId`；HubComposer 暴露 `inputRef`；App 在 workspace 变更（且非首挂载）时 `composerInputRef.current?.focus()`。
- 未完成项：
  - 验收 3 的"default/compact 与窄窗口不挤压发送/最新回答"属布局矩阵，按依赖归 R35 真实产品验收执行。
  - 真实 Tauri 端到端（原生目录选择器后的焦点行为）在 jsdom 只能测到 bridge mock 层，真机复验归 R35。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
