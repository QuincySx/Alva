# R10: 切换会话后压缩必须写入当前会话

**Priority:** P1
**Axis:** Spec / R05 回归；Standards 设计启发
**Origin:** 2026-09-10 review，`6e5bf33...134b8b3`
**Blocked by:** 无
**Status:** archived-reviewed

> 已于2026-09-11复审；未完成行为见[当前第三轮清单](../../kernel-boundary-review-round3/TASKS.md)。不重新按本卡开工；归档不表示全部验收通过。

## 问题与证据

CompactionMiddleware::configure 在 `agent-context/src/middleware.rs:212–213` 用 OnceLock 缓存装配时的完整会话；压缩在 :398–413 向该会话 append marker 并 compact_messages。
BaseAgent::swap_session（`base_agent/agent.rs:255–275`）只更新 state.session 和自己的 full_session，未更新 middleware 缓存与 Agent.full_session。CLI 存在启动恢复、新建/切换会话调用。
结果是读取新会话生成摘要，却覆盖旧会话消息；当前会话仍未压缩。这违反 R05 保留真实 store 消费能力的要求。
证据类型：静态调用链。Standards 另标记 possible Shotgun Surgery：同一“当前会话”分散持有并需多处同步，这是设计判断，不是独立硬性规范。

## 实现范围

- 为会话切换与当前持久化能力建立一致的所有权与更新路径，涵盖压缩、工具上下文及公开 session 访问。
- 保留原有 flush 失败不切换的语义；避免与运行中的 turn 产生半切换状态。
- 不用取消压缩、写回旧会话或重新扩大内核输入契约绕过问题。

## 验收

- [ ] 构造会话 A，切到有足够消息触发压缩的 B，执行真实压缩路径：marker/compact 只写 B，A 完全不变。
- [ ] 再切 C、切回 B 均使用当前会话；当前日志与完整能力指向同一逻辑会话。
- [ ] flush 失败保留旧会话；最小 ConversationLog 运行不要求 store。
- [ ] 覆盖 CLI 启动恢复及 new/resume 使用的切换路径。

## 执行记录

遵守根及目录 AGENTS.md、FRACTAL-DOCS.md；只修本卡范围。先复现再修复，每项逻辑改动独立提交。公开表面变化同步快照。完成时填写 commit、实际验证命令及结果；不得将静态推导写为测试已复现，或将定向测试写为全量通过。

- Commit：待填写
- 验证命令及结果：待填写
