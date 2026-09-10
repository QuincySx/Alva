# R23: 核对并修正 Hub 视觉基线漂移

**Priority:** P3
**Axis:** Verification / Hub 浏览器回归
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** UI04–UI10
**Status:** ready-for-agent

## 问题与证据

当前 `npm run test:visual -- --update-snapshots=none`：7 passed / 5 failed。五项均为截图断言，差异集中于顶栏状态标签：旧基线“操作已完成”，当前“已完成”，影响旁边控件水平位置；主结构无大面积差异。
已实际查看 chat expected/actual/diff；其他四项输出也为同一顶栏区域 411/509 像素差异。详见 evidence/visual 与 alva-sep11-visual-tests.txt。不是将五个截图失败分别宣称为五个产品 bug。
截图断言失败还会提前终止 Inspector 测试后续 Back 检查，故此次不能声明整条回退流程已通过。

## 实现范围

用户追加全面UI重整，故本卡作为新设计后的最终基线闸门；不要先把将被重做的旧界面批量更新为expected。新增“你好”无工具、长工具运行、失败/审批、Middleware折叠与子Agent回退的真实内容矩阵。

核对状态文案改变是否符合 typed i18n 与产品规范。若正确则审阅后仅更新受影响基线；若错误则修复文案。让关键 Inspector 交互断言不依赖截图先通过才能执行。禁止整体放宽像素阈值或未经查看全部更新快照。

## 验收

- [ ] 记录每个基线差异的原因，审批后的 expected 与实际一致。
- [ ] 12 项视觉/交互测试通过，Inspector 子层打开及逐层 Back 均真实执行。
- [ ] 中英文、当前桌面 viewport 下状态文字及控件不截断。
- [ ] 不把截图基线通过当作真实 store/engine 端到端通过。

## 执行记录

- Commit：9123e81
- 验证：`npm run test:visual` 先复现 11 项截图漂移（交互测试通过）→ 逐张审阅 actual（漂移均为本轮已审定的 UI02-UI10 重整：字体 token、时间线收编、Middleware 折叠、紧凑 composer、友好标签、设置页清理）→ `npm run test:visual:update` 重建受影响基线 → `npx playwright test` 12/12 通过。Inspector 下钻/回退断言重排至截图之前，真实执行通过。状态文案"已完成"核对为 typed i18n 既定规范化（status.completed），保留。截图基线通过不等同真实 store/engine 端到端通过——该说明已记录。

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
