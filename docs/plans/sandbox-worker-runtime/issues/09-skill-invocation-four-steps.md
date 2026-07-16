# 09 — skill 触发四步（invocation 两档 + 目录注入 + SkillTool 接线 + REPL fallback）

**What to build:** 落地已定案的 skill 触发设计：skill 元数据增加 invocation 字段（auto 默认/explicit 两档，枚举留扩展）；agent 启动时把所有 enabled 且 auto 档 skill 的名称+描述常驻注入为 Level-1 目录，词法关键词注入中间件退役；把占位的 skill 调用工具接上真实 registry 成为统一调用入口（收编现有 search/use 双工具），点名即调、无权限校验，正文按 Explicit 策略注入，声明了工具白名单的走 Strict；REPL 未知斜杠命令 fallback 到 skill 仓库，命中则由 harness 直接注入（旁路模型）。

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] auto 档 skill 的目录常驻出现在 system prompt（recording-mock 断言请求体），explicit 档不出现但点名可调
- [ ] 中文用户消息不再依赖词法匹配——目录注入后模型自主触发（旧中间件删除或退役说明）
- [ ] 统一 skill 工具：调用即加载正文注入，未知名响亮报错并列出可用项
- [ ] REPL 输入未知斜杠命令时查 skill 仓库，命中注入、未命中保持原报错
