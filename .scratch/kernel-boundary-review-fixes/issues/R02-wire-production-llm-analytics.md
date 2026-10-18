# R02: 把 LLM 埋点接回生产 AnalyticsPlugin

**Priority:** P1
**Axis:** Spec / 行为回归
**Origin:** 原卡 15；2026-09-09 review
**Blocked by:** 无
**Status:** archived-reviewed

> 2026-09-10：本卡已有实现并复审，停止按本卡重新开工。当前待办见 [R09–R13](../../kernel-boundary-review-round2/TASKS.md)；归档不代表全部验收通过。

## 问题与证据

原卡 15 要求 LLM 埋点迁至分析中间件，且 provider / 模型 / token / 缓存 / 耗时字段保持一致。
`crates/alva-app-core/src/extension/analytics.rs` 的新 `wrap_llm_call` 写在 AnalyticsMiddleware 上，
但 AnalyticsPlugin::register 只注册 BeforeToolCall / AfterToolCall 两个 phase handler，未安装这个 LLM 钩子。
现有新增测试直接安装 middleware，未覆盖正式插件装配。

已复现：SDK AgentBuilder 注册 AnalyticsPlugin 并完成一轮，分析文件为空。
测试：`production_analytics_plugin_records_llm_call`，见 [复现材料](../evidence/README.md)。

## 实现范围

- 修正 AnalyticsPlugin 的真实接线，使一轮 LLM 调用经过新钩子。
- 保留工具阶段埋点，避免为接入 LLM 而同时注册两份工具埋点。
- 不恢复内核对 AnalyticsSink 的直接依赖，不新建阶段级 loop seam。

## 验收

- [ ] 生产插件装配回归测试转绿；通过插件运行一轮，恰好收到一条 LlmCall。
- [ ] 通过正式插件装配断言 session/provider/model、token、缓存字段与既有口径一致；耗时和时间戳按合理范围验证。
- [ ] 一次工具调用恰好产生一对 start/end 埋点，不因双重注册重复。
- [ ] 无分析插件时运行正常；LLM 错误路径保持迁移前的记录语义。
- [ ] 现有 analytics 相关测试和内核依赖边界检查通过。

## 通用约束

遵守根 AGENTS.md、对应目录 AGENTS.md 与 FRACTAL-DOCS.md。公开表面变化同步更新相关快照；每项逻辑修复单独提交，记录实际运行的检查和结果，不把未运行的检查标为通过。
