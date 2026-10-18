# R09: 恢复生产工具上下文的持久化子会话能力

**Priority:** P1
**Axis:** Spec / R05 回归
**Origin:** 2026-09-10 review，`6e5bf33...134b8b3`
**Blocked by:** 无
**Status:** archived-reviewed

> 已于2026-09-11复审；未完成行为见[当前第三轮清单](../../kernel-boundary-review-round3/TASKS.md)。不重新按本卡开工；归档不表示全部验收通过。

## 问题与证据

`agent_spawn.rs:558` 改为读取 `ctx.store_session()`，但 `tool_batch.rs:640–646` 只接入 `with_session(scoped_session)`。`RuntimeExecutionContext` 的 store_session 默认为 None，新增 with_store_session 无生产调用。
因此真实 agent_spawn 无法调用父仓库的 create_child_session，退回内存子会话，丢失持久化与父子因果链接。R05 要求收窄内核契约，同时保留产品持久化能力。
证据类型：静态生产调用链；现有 shared-lock 回归测试不覆盖仓库子会话。

## 实现范围

- 在装配/插件层把当前会话的持久化能力接到真实工具执行路径；同时覆盖递归子 Agent。
- 不把 AgentState.session 改回 AgentSession，不强迫最小 ConversationLog 消费者实现 SessionStore。
- 与 R10 协调，避免接入构造时的旧会话或新增互相漂移的会话副本。

## 验收

- [ ] 用真实 AgentBuilder/BaseAgent + agent_spawn + 可观测 SessionRepository 跑生产路径，验证 create_child_session 被调用。
- [ ] 父仓库保存 child link，子记录可重新加载，Inspector 可沿链接读取。
- [ ] swap_session 后新建子会话属于新父会话；递归子会话同样持久化。
- [ ] 最小 ConversationLog 示例及已有工具上下文测试继续通过。

## 执行记录

遵守根及目录 AGENTS.md、FRACTAL-DOCS.md；只修本卡范围。先复现再修复，每项逻辑改动独立提交。公开表面变化同步快照。完成时填写 commit、实际验证命令及结果；不得将静态推导写为测试已复现，或将定向测试写为全量通过。

- Commit：待填写
- 验证命令及结果：待填写
