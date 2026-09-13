# R37: 将新增产品文案归入 typed i18n

**Priority:** P3
**Axis:** Standards
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；UI01、UI09
**Blocked by:** R30、R31、R32、R33、R34
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

components/product-pages.tsx:83、87、109、172、233、241 等新增 locale === zh-CN 分支。web/src/AGENTS.md 明确“领域组件使用 typed message key，不在组件内按语言写条件分支”。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

将本轮及返工新增文案集中进 typed i18n 字典，组件只消费 key；不扩成全仓无关文案重写。同步控件可访问名称，避免中英文说明能力范围不一致。

## 验收

- [x] 新增/修改领域组件不再散落语言条件分支。
- [x] 中英文文案等义，未知/禁用/权限说明一致；tsc与相关控件测试通过。
- [x] 长文案布局验收并入R35，避免回归可访问名称。

## 执行记录（执行者填写）

- Commit：`92967d0`（refactor(hub-desktop): product pages consume typed i18n keys, no inline locale branches (R37)）。
- 专项复现/回归命令、版本（vitest 2.1.9，HEAD `92967d0`）：
  - `npm test -- src/i18n.test.tsx` — 3/3，新增守卫：`keeps reworked domain components on typed keys, not inline locale branches`（经 Vite `?raw` 读 product-pages.tsx 源码，断言无 `locale === "zh-CN"` 内联分支）。
  - `npm test` 全量 — 6 files / 93 tests 通过；`tsc -b` 通过。
  - R31/R32 既有断言随可访问名称同步更新：Permission Mode 组合框、Worker/Resource/Git 开关的 accessible name 现与 typed 行标题一致（aria-label 不再是脱离界面的英文硬编码）。
- 覆盖范围：product-pages.tsx 全部内联语言分支（含 R20/R21/R22/R31/R32 新增与该文件历史遗留约 90 处）+ App.tsx 本轮新增 1 处（`nav.closeNavigation`）。provider-settings.tsx / hub-timeline.tsx / engine-readiness-panel.tsx 的历史分支早于本轮范围（f9d0e57 前已存在，diff 计数 0），按卡内"不扩成全仓无关文案重写"保留。
- 中英文等义修正：原只有中文的快捷键 5 条描述补英文；"4 个 Worker (标准推荐)/(标准)"、"节能模式 (Low Power)/(节能)" 等注记统一。
- 未完成项：
  - 06-settings 视觉基线含可见文案变化（如 Git 开关 label 由英文常驻变本地化），基线更新与逐图差异记录按依赖归 R36。
  - 长文案布局验收按卡内说明并入 R35。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
