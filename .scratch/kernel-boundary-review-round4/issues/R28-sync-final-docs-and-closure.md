# R28: 同步最终职责文档并修正卡片完成口径

**Priority:** P3
**Axis:** Standards
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；R19、全部第三轮卡
**Blocked by:** R24、R25、R26、R27
**Status:** done

## 问题与证据

kernel-abi/AGENTS.md:28–32 仍把迁出的后端/context apply/tool lock 指向 kernel-core；PersistentSessions 未进入对应目录清单；agent-core/AGENTS.md:75–77 的投递/能力描述与代码不符。第三轮 TASKS 全部 done，单卡 Status 仍 ready-for-agent、验收全未勾，UI05/06/07/08/10 执行记录直接承认未完成。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

功能修复后按 FRACTAL-DOCS 更新源码头/目录地图/必要上级；建立每卡 requirement→commit→专项测试证据→关闭结论，不用整包通过数代替专项验收。历史记录保留，明确归档/转卡关系。

## 验收

- [x] 新增公开接口、模块职责和迁移位置可逐条对照最终源码。
- [x] 所有遗留验收都有关闭证据或明确待办卡；不得将部分实现写 done。
- [x] 修复后的广播、递归和 swap 描述准确，卡片自身状态与总表一致。

## 执行记录（执行者填写）

- Commit：`2ef40a8`（docs 同步 + 本轮卡片记录收口；与 round3 复审转向说明同 commit）。
- 专项复现/回归命令、版本（rustc 1.93.0 / vitest 2.1.9 / Playwright 1.62.1，2026-09-13）：
  - 文档逐条对照最终源码核实后更新：
    - `crates/alva-kernel-abi/AGENTS.md`：约束区迁移指向改准——运行时 apply 助手在 `alva-agent-context::context_apply`、具体会话后端（InMemoryAgentSession/CellBackedSessions）在 `alva-agent-core::agent_session`、ToolLockRegistry 运行时在 `alva-agent-core::tool_lock`（原指向 kernel-core 均已过时）；Provider/ProviderRegistry 已迁 `alva-agent-abi`（grep 证实 kernel-abi 无该 trait）；Module Map 补齐 agent_session（含 PersistentSessions/StoreSessionCell）、session_store、conversation_log、session_record、scope、runtime、tool/ 子模块、base/、model/、constants。
    - `crates/alva-kernel-abi/src/AGENTS.md`：业务域清单补 PersistentSessions（内核唯一会话能力）、StoreSessionCell（装配层 cell，内核不识别）、SessionStore 超集、ConversationLog 最小集、ContextHooks/ContextApply、Sleeper/timeout。
    - `crates/alva-agent-core/AGENTS.md`：迁入模块描述按 R25/R26/R27 真实现重写（BroadcastCore 唯一投递、commit→notify 无 await、真实 abort 语义、代际隔离、CellBackedSessions dyn 桥）。
    - 根 `AGENTS.md`：kernel-abi L1 行补 `PersistentSessions` 能力描述。
  - 卡片完成口径：round4 TASKS 总表与 14 张卡 Status/验收框逐张更新（R24–R34、R37 done；R35/R36 closed 含显式边界）；R24/R29–R32 补齐执行记录（commit + 2026-09-13 复跑断言）；round3 README/TASKS 复审转向说明随本 commit 入库。
  - 复跑证据：`cargo test --offline -p alva-agent-core --test review_broadcast_regression` 12/12、`--test review_subscription_probe` 1/1、`--test agent_builder builder_registers_the_persistent_sessions_key_the_tool_batch_reads` 1/1；web `npm test` 93/93；`npx playwright test` 23/23。
- 未完成项：无（R35/R36 的显式边界已在其执行记录中如实保留，不冒充 done）。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
