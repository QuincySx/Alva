# R01: 子 agent 恢复共享资源锁约束

**Priority:** P1
**Axis:** Spec / 行为回归
**Origin:** 原卡 08；2026-09-09 review
**Blocked by:** 无
**Status:** archived-reviewed

> 2026-09-10：本卡已有实现并复审，停止按本卡重新开工。当前待办见 [R09–R13](../../kernel-boundary-review-round2/TASKS.md)；归档不代表全部验收通过。

## 问题与证据

原卡 08 要求「并发工具执行、锁等待、锁超时的既有行为与测试断言不变」。
内核的取锁逻辑删除后，`ToolLockRegistryPlugin` 只给父 agent 注册了 `ToolLockMiddleware`。
`AgentSpawnTool` 手动构造的 child middleware 只有安全/稳定性等中间件，没有锁中间件。
子 agent 虽然继承同一个 bus 与锁表，却不再获取锁。

已复现：另一执行者持有 `SerialGlobal` 写锁时，子工具仍执行。
测试：`spawned_child_obeys_shared_tool_lock`，见 [复现材料](../evidence/README.md)。

## 实现范围

- 检查 `crates/alva-app-core/src/extension/agent_spawn.rs` 的 child middleware 装配，以及其他共享锁表的 child 入口。
- 子 agent 使用同一份注册表，在装配层补齐锁中间件；保留 workspace 路径解析及超时配置。
- 涉及 `crates/alva-agent-core/src/tool_lock.rs`、`crates/alva-app-core/src/extension/tool_lock_registry.rs` 时保持插件所有权。
- 不把锁表或取锁实现搬回 kernel，不让 Coordinator 自身持有会阻塞嵌套调用的锁。

## 验收

- [ ] 上述生产 BaseAgent + SubAgentPlugin 的失败测试转绿。
- [ ] 锁被持有时，冲突的子工具不执行；超过期限返回工具错误。
- [ ] 释放锁后子工具可执行，父子共享同一注册表；覆盖同资源写冲突与无冲突路径。
- [ ] Coordinator 仍能驱动嵌套工具，无新增死锁；取消和锁超时保持有界。
- [ ] `alva-agent-core` 既有 `tool_lock_middleware` 测试及 app-core 子 agent 安全测试通过。

## 通用约束

遵守根 AGENTS.md、对应目录 AGENTS.md 与 FRACTAL-DOCS.md。公开表面变化同步更新相关快照；每项逻辑修复单独提交，记录实际运行的检查和结果，不把未运行的检查标为通过。
