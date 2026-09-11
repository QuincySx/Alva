# R14: 递归子会话必须保存到正确的父仓库

**Priority:** P1
**Axis:** Spec / R09
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** R18
**Status:** ready-for-agent

## 问题与证据

首层 store_session 接线已修正，但递归仍有三处缺口：
1. runtime_context.rs:150–162 未覆盖 ToolExecutionContext::store_session_cell；具体类型的方法返回 Some，agent_spawn 使用 dyn trait 时却命中默认 None。
2. run_child.rs:89–92 向 Extensions 插入 Arc<StoreSessionCell>，tool_batch.rs:650 却按 StoreSessionCell 获取，typed key 不匹配。
3. agent_spawn.rs:705 传递的是父 cell，而不是当前 child_session 的能力；仅补前两点会将孙会话关联到根父会话。
R09 验收要求递归子会话持久化，并在 Inspector 中保留真实父子归属。证据为静态调用链及 evidence/review_round3_probes.rs 的动态上下文断言；运行结果以 evidence/README 为准。

## 实现范围

在 R18 确定的上层装配边界内修复递归能力传递。每个运行中的 child 必须持有自己的仓库身份，不能共享指向另一个会话的可变当前 cell。不要通过硬编码下转型绕过 trait 缺失。

## 验收

- [ ] 真正运行 root → child → grandchild 的 agent_spawn 链，三者都有独立持久记录。
- [ ] grandchild 的因果链接仅落到直接父 child，重启后 Inspector 可逐层下钻。
- [ ] 使用真实 dyn ToolExecutionContext 路径，不能只测具体类型方法。
- [ ] root 后续切换会话不重绑已经开始的 child 身份。
- [ ] 最小 ConversationLog 无 store 消费者继续通过。

## 执行记录

- Commit：83052d3
- 内容：递归子会话持久化到直接父仓库：RuntimeExecutionContext 补 store_session_cell 覆写、
SubAgentPlugin 派生链传递父 StoreSessionCell、会话记录沿父链写入。
- 验证（2026-09-12 复跑）：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` 32 个测试目标全部 ok，655 passed / 0 failed；`scripts/ci-check-deps.sh` 通过（Rule 17/18/19 边界完好）。
- 实际验证命令及结果：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` → 32 个测试目标 ok，655 passed / 0 failed（2026-09-12 复跑）；`scripts/ci-check-deps.sh` exit 0。

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
