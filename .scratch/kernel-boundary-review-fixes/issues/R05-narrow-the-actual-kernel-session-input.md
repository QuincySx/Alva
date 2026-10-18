# R05: 内核入口真正只要求 ConversationLog

**Priority:** P1
**Axis:** Spec / 核心验收缺口
**Origin:** 原卡 06；2026-09-09 review
**Blocked by:** R01, R02, R03, R04（先固定行为回归，再调整共享契约）
**Status:** archived-reviewed

> 2026-09-10：本卡已有实现并复审，停止按本卡重新开工。当前待办见 [R09–R13](../../kernel-boundary-review-round2/TASKS.md)；归档不代表全部验收通过。

## 问题与证据

原卡 06 明确要求「内核循环只依赖对话记录，不再要求查询 / 回滚 / 快照 / 订阅 / 压缩」。
但 `crates/alva-kernel-core/src/state.rs` 仍声明 `pub session: Arc<dyn AgentSession>`；
AgentSession 是 ConversationLog + SessionStore 的组合。run_agent 需要 AgentState，
因此仅实现 ConversationLog 的消费者仍不能运行内核。
`run/mod.rs` 内的局部 upcast 只改变局部变量类型，不改变调用方必须实现的契约。

这是静态 API 验收缺口，需用新的最小消费者测试固定，不能以修改注释作为完成。

## 实现范围与边界

- 梳理 AgentState、run_agent/AgentLoop、ChildAgentParams、middleware 对 session 的真实需求，使 kernel 入口只接受 ConversationLog。
- 依赖 SessionStore 的查询、压缩、持久化装配和 Inspector 能力由上层显式提供，保留完整产品功能。
- 对跨层接线给出清晰的所有权说明；不得通过给 ConversationLog 加上 query/rollback/compact 等方法来绕过目标。
- 不要求最小消费者实现返回空值的假 SessionStore，不让 kernel 依赖 agent-abi/app/host。
- 不增加 Registrar 读取方法，不抽新的阶段级 loop seam；若实际接口/职责改变，同步相关文档及公开符号快照。

## 验收

- [ ] 新测试定义只实现 ConversationLog 的内存日志，可直接构造内核运行态并完成纯文本一轮。
- [ ] 该最小日志无 AgentSession/SessionStore 实现，也无需依赖 SDK 装配或产品 crate。
- [ ] kernel 入口不再强制查询、回滚、快照、订阅、压缩能力；产品能力通过上层接线仍可用。
- [ ] typed tool intent/outcome 的原子性、错误终态、flush 与取消行为不退化。
- [ ] 卡 R01–R04 的回归测试仍通过；CLI/Hub 会话、压缩、审批、子 agent 和 Inspector 的相关测试通过。
- [ ] 公开表面快照、依赖边界与要求的 wasm 检查通过；本轮最终执行完整测试套件并如实记录结果。

## 通用约束

遵守根 AGENTS.md、对应目录 AGENTS.md 与 FRACTAL-DOCS.md。公开表面变化同步更新相关快照；每项逻辑修复单独提交，记录实际运行的检查和结果，不把未运行的检查标为通过。
