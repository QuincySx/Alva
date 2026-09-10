# UI05: 重做 Inspector 首屏重点与分层下钻

**Priority:** P1
**Axis:** 用户最新 UI 重整需求 / 产品审查
**Origin:** 2026-09-11 用户“你好”真实会话及全 UI 反馈；HEAD `f9d0e57`
**Blocked by:** UI01、UI02、UI03
**Status:** ready-for-agent

## 问题与证据

真实“你好”只有1次LLM调用、无工具，却在Inspector首部常驻36条Middleware调用，最新轮次默认展开后再平铺before/wrap/after hooks。engine-inspector.tsx:365–368、398–407；原生截图/tree可直接复现。
首屏强调native ID/记录数，LLM与工具结果被淹没；工具错误需继续找，用户无法迅速看出重点。当前与产品Attempt摘要还显示冲突的用量（原生6589/125，另一块0 token/空耗时）。

## 实现范围

默认概览只显示当前/选中Attempt身份、状态、当前动作、总耗时、有效用量、关键步骤和最近异常。步骤以紧凑时间顺序列表呈现LLM/Tool，明确成功/失败/取消及简短输入/结果摘要。
Middleware默认折叠为计数/异常提示或高级诊断入口；选中某步后在二级页面查看完整hook链。生命周期hook与轮次hook有明确上下文，不重复平铺。
保留执行时间线入口、raw records与完整诊断能力；标明inclusive duration，避免将洋葱每层同一耗时相加误读。缺失数据用未知/不可用，不伪装0。

## 验收

- [ ] “你好”默认无需滚动即可看到一次LLM调用、成功、真实耗时/用量；50个成功hook不增加概览高度。
- [ ] Middleware零默认展开，失败hook有可发现入口，点击后可查名称/阶段/耗时/错误。
- [ ] 运行/失败/取消时无需打开JSON即可判断正在做什么、哪一步出错、下一步点哪里。
- [ ] 同屏用量/耗时来源一致；未知值不显示假的0或空单位。
- [ ] 两级子Agent及历史Attempt有可读面包屑，返回恢复先前选择与滚动。

## 执行记录

- 设计/实现 commit：616c2d9
- 验证：`npx tsc --noEmit` 通过；`npx vitest run` 70 tests 通过；`npm run build` 通过。根级 Middleware 默认折叠为计数+失败入口；轮次 hook 平铺改为单行计数→二级 turnHooks 页；LLM/Tool 二级页补 HookChain；失败/取消徽标可见；0/0 用量显示 "—"。已知部分：面包屑返回的滚动位置恢复未实现（结构上保留 path/back stack）。

只在 alva-hub-desktop 的 Tauri/React UI 上实施，不改旧 alva-app-tauri。遵守根/目录 AGENTS.md、FRACTAL-DOCS.md 及 UI glossary/wireframes/motion；与用户最新要求冲突的旧描述按 UI01 更新。每个逻辑改动独立提交，不为过截图直接放宽阈值，不把demo视图冒充真实产品。此轮只review与建卡，未执行本卡实现。
