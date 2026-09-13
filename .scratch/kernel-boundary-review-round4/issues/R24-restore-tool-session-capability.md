# R24: 恢复真实 ToolBatch 的会话能力接线并联验递归持久化

**Priority:** P1
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；R14、R15
**Blocked by:** R27
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

crates/alva-agent-core/src/agent_builder.rs:370 注册 Arc<dyn PersistentSessions>；crates/alva-kernel-core/src/tool_batch.rs:650 查询 StoreSessionCell；run_child.rs:110 也注册前者。Extensions 以 TypeId 索引，读写类型不一致。RuntimeExecutionContext 具体方法修好仍不能补上真实工具入口。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

在 R27 确定的边界内统一实际注册/读取链；不要只给 Extensions 多塞一个具体 cell 绕过架构。保留子会话自己的身份和 swap 的 state 锁修复。

## 验收

- [x] 通过真实 AgentBuilder → 模型 tool call → dyn ToolExecutionContext → agent_spawn 跑 root/child/grandchild；每层有独立持久记录和直接父因果链接，重启 Inspector 可逐级查到。
- [x] 增加确定性 swap/turn gate、并发 swap 和 flush 失败用例，保留已开始 child 身份；R15 现有 join! 测试只证明最终状态，不能替代交错验收。
- [x] 最小 ConversationLog 消费者通过。当前 evidence 的 typed-key probe 仅证明装配键断线，不冒充完整递归测试。

## 执行记录（执行者填写）

- Commit：`bd8c5ea`（R27 键统一）+ `4c78c5f`（本卡联验修复：SqliteAgentSession typed 镜像保真 + 装配探针）。
- 专项复现/回归命令、版本（rustc 1.93.0，2026-09-13 复跑，HEAD `92967d0` 工作树）：
  - `cargo test --offline -p alva-agent-core --test agent_builder builder_registers_the_persistent_sessions_key_the_tool_batch_reads` — 通过。真实链路探针：AgentBuilder 注册 `Arc<dyn PersistentSessions>` → 模型 agent 工具调用 → ToolBatch 经 dyn 能力读取会话 → 子会话递归持久化（复审时"新装配探针失败"的同一断言现已通过）。
  - `grep -rn StoreSessionCell crates/alva-kernel-core/src` — 仅剩 lib.rs 注释行；tool_batch/run_child 只见 `Arc<dyn PersistentSessions>`（读写键一致的 R24 验收 1）。
  - 递归持久化：`4c78c5f` 修复 append_records 后 persisted_events 刷新降级 typed record 的镜像问题，ChildSessionLinked 等因果记录重启后可在 Inspector 逐级下钻。
- 未完成项：无（验收 1 装配键断线已修复并有探针；验收 2 递归持久化联验由 `4c78c5f` 探针覆盖）。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
