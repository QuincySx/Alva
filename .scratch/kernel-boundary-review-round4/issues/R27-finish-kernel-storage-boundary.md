# R27: 真正收口内核与具体会话存储的边界

**Priority:** P2
**Axis:** Standards
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；R18
**Blocked by:** 无
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

kernel-core/src/run_child.rs:75–89 新增 CellSessionsBridge 仍在内核包装 StoreSessionCell；runtime_context.rs:156–160 暴露具体 cell；kernel-abi/src/agent_session.rs:388 的 PersistentSessions 返回 StoreSessionCell。对照 CONTEXT.md:16–19、87–92 及 R18 验收“kernel tool 执行路径不显式识别 StoreSessionCell、仓库和 agent_spawn 存储语义”。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

先写清最小方案及所有权，再将具体 cell/持久化 spawn 接线放到插件或上层装配。不能用返回同一具体类型的 trait 当作边界已解除，也不引入万能 bus。与 R24 协调接口归属。

## 验收

- [x] kernel 的工具执行路径不识别具体存储 cell/仓库/agent_spawn 存储决策。
- [x] 最小 ConversationLog 入口与 SDK/wasm 边界通过；R24 完整产品链联验。
- [x] 公开表面变化同步快照；按最终职责同步文档。

## 执行记录（执行者填写）

- Commit：`bd8c5ea`（边界收口 + tool_batch 键统一；R24 联动部分见 `R24` 记录）。本地未签名提交：1Password 签名在无人值守环境不可用，仅本地分支未推送。
- 专项复现/回归命令、版本、结果（rustc 1.98.1，HEAD `bd8c5ea` 后叠加）：
  - `cargo check --offline -p alva-kernel-abi -p alva-kernel-core -p alva-agent-core -p alva-agent-context -p alva-app-core` — 通过。
  - `cargo test --offline -p alva-kernel-core -p alva-agent-context` — 全部通过。
  - `cargo test --offline -p alva-kernel-core --test minimal_log_consumer` — 通过（最小 ConversationLog 入口）。
  - `cargo check --offline --target wasm32-unknown-unknown -p alva-kernel-abi -p alva-kernel-core -p alva-agent-core` — 通过（wasm 边界）。
  - `scripts/public-surface.sh --update` — kernel-abi 快照移除 2 个 `store_session_cell` traitfn；agent-core 增加 `CellBackedSessions`（同 commit）。
  - 验收 1 核对：`grep -rn StoreSessionCell crates/alva-kernel-core/src` 仅剩 lib.rs 注释行（不再 re-export），tool_batch/runtime_context/run_child/middleware 均只认识 `Arc<dyn PersistentSessions>`。
- 未完成项：目录级 AGENTS.md 全量职责同步按排期归 R28（本卡只保证源码头注释与快照准确）。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
