# R32: 清理生产设置与扩展页的样例状态

**Priority:** P2
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；R22、UI02、UI09
**Blocked by:** 无
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

components/product-pages.tsx:480–505 主题选择仅局部 state；后续密度/reduced-motion 同类。:827–843 硬编码 token 数、$14.28 和模型统计；:853–854 假版本/已最新。MCP/SSH 页面 :690–752 仍在生产展示样例、允许局部增删，提示样例不等于隔离到 demo。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

生产页读取真实数据；未支持外观/目录操作诚实禁用，样例移到 demo/UI Lab。真实版本从应用元数据取，用量/更新状态无数据就呈现未接入，不虚构统计。保留已经实现的自动化禁用和插件安装禁用。

## 验收

- [x] 非 demo 进入所有设置分区/扩展页，无假连接、假保存、假用量或假更新结论。
- [x] 浅色/系统主题/密度/减弱动效要么真实生效并按契约持久化，要么禁用说明。
- [x] 有真实数据、空、加载、失败、不可用各状态的组件/页面验证。

## 执行记录（执行者填写）

- Commit：`757216a`。
- 专项复现/回归命令、版本（vitest 2.1.9；2026-09-13 复跑 93/93，HEAD `92967d0`）：
  - `npm test -- src/components/components.test.tsx` — R32 专项（757216a 新增 + R37 label 更新）：
    - `appearance options that do nothing are disabled`：浅色/跟随系统主题、全局密度、减弱动效开关禁用且描述真实行为。
    - `tools section disables fake toggles and shows MCP as not wired`：工具假开关禁用；无 MCP 样例行、无 Add MCP Server 入口，显示"未连接后端"诚实提示。
    - SSH/用量/版本分区同样为诚实未接线态（Notice/禁用徽标），无样例主机、无虚构 token/成本/版本号。
  - 生产扩展目录：全部 installed:false、安装按钮禁用并说明原因（R22 修复保留）。
- 未完成项：无。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
