# R11: 子 Agent 资源锁继承工作区解析

**Priority:** P1
**Axis:** Spec / R01 未完整覆盖
**Origin:** 2026-09-10 review，`6e5bf33...134b8b3`
**Blocked by:** 无
**Status:** archived-reviewed

> 已于2026-09-11复审；未完成行为见[当前第三轮清单](../../kernel-boundary-review-round3/TASKS.md)。不重新按本卡开工；归档不表示全部验收通过。

## 问题与证据

`agent_spawn.rs:659–665` 为 child 安装 ToolLockMiddleware::new().with_bus(...)，但 child 不经过 AgentBuilder::configure。with_bus 仅注入 bus，workspace OnceLock 未设置。
`agent-core/src/tool_lock.rs:930` 以该 workspace 解析资源键；父层已配置工作区，子层没有。同一文件的相对与绝对资源键因此落到不同锁上，父子 Agent 仍能并发写同一文件。
R01 要求共享资源锁，不仅是 SerialGlobal。现有 spawned_child_obeys_shared_tool_lock 仅证明全局锁，不覆盖此情况。
证据类型：静态装配与资源键归一化调用链。

## 实现范围

- 给子中间件接入真实 child workspace 与共享 registry，遵守目录切换/授权边界。
- 取锁保留在装配层，不移回 kernel；保留 Coordinator 豁免及 SerialGlobal 行为。

## 验收

- [ ] 父持 `/workspace/src/foo.rs` 写锁，子工具声明 `src/foo.rs`：真实 agent_spawn 路径中必须等待释放。
- [ ] 相反方向、两个子 Agent、明确不同资源以及读读共享均覆盖。
- [ ] 使用 semaphore/barrier 确定性断言并发，不靠随机 sleep。
- [ ] 现有全局锁回归、锁取消/超时、最小内核消费者继续通过。

## 执行记录

遵守根及目录 AGENTS.md、FRACTAL-DOCS.md；只修本卡范围。先复现再修复，每项逻辑改动独立提交。公开表面变化同步快照。完成时填写 commit、实际验证命令及结果；不得将静态推导写为测试已复现，或将定向测试写为全量通过。

- Commit：待填写
- 验证命令及结果：待填写
