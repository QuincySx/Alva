# R18: 将具体会话存储接线移回装配层

**Priority:** P2
**Axis:** Standards / 架构规则
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** 无
**Status:** ready-for-agent

## 问题与证据

tool_batch.rs:648–651 在内核按 StoreSessionCell 具体类型查询 Extensions，注释用途明确是 agent_spawn 的持久化子会话。kernel-abi/tool/execution.rs:96 新增具体 cell 的访问接口；run_child 再持有并传递同一具体 store 能力。
CONTEXT.md 规定 Kernel 不认识会话如何存，Session Store 不是内核依赖，并强调默认实现不能消除具体工具耦合。这次功能修复重新将上层存储/子 Agent 决策带回内核；最小消费者测试能通过不代表边界符合设计。
证据：Standards 静态复审。当前 public-surface 检查通过，仅代表快照一致，不能证明新增表面满足架构。

## 实现范围

为真实消费者选择上层插件/装配持有的窄能力桥，具体 cell 与 spawn 持久化语义不应由内核识别。先记录最小方案与归属再实施，不为了绕过类型检查创建新的 bus God Object 或隐藏下转型。与 R14/R15 保留产品功能。

## 验收

- [ ] kernel tool 执行路径不显式识别 StoreSessionCell、仓库和 agent_spawn 存储语义。
- [ ] 纯 ConversationLog 入口与现有通过的产品行为不退化。本卡验收窄能力桥及归属；递归持久化、原子换会话的完整验收分别由后续 R14、R15 负责，并在依赖链完成后联验，不作为本卡完成前置条件。
- [ ] 公开接口变化同 commit 更新快照，Registrar/Bus/SDK 依赖边界仍满足规则。
- [ ] 新职责的源码头与目录文档即时更新。

## 执行记录

- Commit：dbb743e
- 内容：存储接线收回装配层：kernel 只认 ConversationLog + PersistentSessions 窄能力桥，
具体 cell/store 语义移回 agent-core/app-core。
- 验证（2026-09-12 复跑）：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` 32 个测试目标全部 ok，655 passed / 0 failed；`scripts/ci-check-deps.sh` 通过（Rule 17/18/19 边界完好）。
- 实际验证命令及结果：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` → 32 个测试目标 ok，655 passed / 0 failed（2026-09-12 复跑）；`scripts/ci-check-deps.sh` exit 0。

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
