# R04: run_start 写入失败也必须发送错误终态并收尾

**Priority:** P2
**Axis:** Spec / 新失败路径回归
**Origin:** 原卡 06；2026-09-09 review
**Blocked by:** 无
**Status:** archived-reviewed

> 2026-09-10：本卡已有实现并复审，停止按本卡重新开工。当前待办见 [R09–R13](../../kernel-boundary-review-round2/TASKS.md)；归档不代表全部验收通过。

## 问题与证据

原卡 06 将日志读写改为可失败；`crates/alva-kernel-core/src/run/mod.rs` 在发送 AgentStart 后，
对 run_start 追加执行 `.await?`，而它位于统一错误收尾的 async body 之外。
追加失败会直接跳过 AgentEnd、on_agent_end、context dispose 与最终 flush。
SDK/产品包装层只记录错误，消费者看不到结构化错误终态。

已复现：注入首条 append 失败，事件流为 `[AgentStart]`。
测试：`failed_run_start_still_emits_terminal_event`，见 [复现材料](../evidence/README.md)。

## 实现范围

- 将启动持久化错误纳入统一终止路径，保证每次已开始的运行有一次收尾。
- run_start 未写入时，不伪造已持久化的父记录；明确 run_end 的可选父关系/记录策略。
- 保存最初的写入错误，后续清理或 flush 错误不覆盖根因；不启动模型或工具。

## 验收

- [ ] 故障注入回归测试转绿：一个 AgentStart 后有且仅有一个带错误的 AgentEnd。
- [ ] 记录器验证生命周期清理和 dispose 符合启动阶段语义且至多一次；最终 flush 被尝试。
- [ ] 原始追加错误返回给调用方/进入错误终态，无成功 Completed 假象。
- [ ] 中间启动步骤失败和正常路径仍产生正确终态，不重复清理。
- [ ] kernel 生命周期相关测试、SDK streaming 及 bounded 背压测试通过。

## 通用约束

遵守根 AGENTS.md、对应目录 AGENTS.md 与 FRACTAL-DOCS.md。公开表面变化同步更新相关快照；每项逻辑修复单独提交，记录实际运行的检查和结果，不把未运行的检查标为通过。
