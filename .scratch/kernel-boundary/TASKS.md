# 内核边界重构 —— 工单索引

> 背景、判据与「明确不做」清单见 [README.md](./README.md)。工单正文在 [issues/](./issues/)。
> 动手前先读仓库根 `CONTEXT.md`——工单一律使用那套词汇。
> **ARCHIVED：历史卡 01–19 已归档，不再是可执行清单。** 已有实现提交不代表全部验收通过。
> 2026-09-10 复审修复请只执行 [当前清单 R14–R23 + UI01–UI10](../kernel-boundary-review-round3/TASKS.md)。
> 下方依赖图和开工建议仅为历史记录，不再用于当前排期。

## 前置闸门

| # | 标题 | Blocked by |
|---|---|---|
| [01](./issues/01-public-surface-snapshot-gate.md) | 公开符号快照闸 + Registrar 准入规则 | — |

> **01 之后每个 commit 都必须带符号快照 diff。** 快照文件是这条巨型分支的进度条。

## Tool 契约（三张可并行）

| # | 标题 | Blocked by |
|---|---|---|
| [02](./issues/02-purge-dead-surface.md) | 清除内核契约层的死表面 | 01 |
| [03](./issues/03-tool-scheduling-descriptor.md) | 调度语义收成一个描述符 | 01 |
| [04](./issues/04-tool-describes-harness-decides.md) | 工具自述行为，harness 决定权限 | 01 |

> 04 是本次唯一有真实行为改动的一刀，其余两张是删除与合并。

## 会话契约（expand → contract）

| # | 标题 | Blocked by |
|---|---|---|
| [05](./issues/05-split-conversation-log-and-session-store.md) | 拆出对话记录与会话存储（expand） | 01 |
| [06](./issues/06-kernel-requires-only-conversation-log.md) | 内核只认对话记录 | 05 |

## 内核瘦身（07/08/11 可并行）

| # | 标题 | Blocked by |
|---|---|---|
| [07](./issues/07-no-dangling-tool-calls-on-middleware-error.md) | 工具批出错不再留下悬空调用 | 01 |
| [08](./issues/08-move-tool-lock-out-of-kernel.md) | 资源锁搬出内核 | 01 |
| [09](./issues/09-move-in-memory-sessions-out-of-kernel.md) | 内存会话实现搬出内核 | 06 |
| [10](./issues/10-move-context-apply-and-fix-compaction.md) | 上下文执行搬出内核，压缩不再删日志 | 06 |
| [11](./issues/11-narrow-loop-seam-and-real-streaming.md) | 循环 seam 收窄，run 真流式 | 01 |

## 拆 agent 契约层（唯一的 wide refactor）

| # | 标题 | Blocked by |
|---|---|---|
| [12](./issues/12-create-agent-abi-expand.md) | 新建 agent 契约层（expand） | 09, 10 |
| [13](./issues/13-migrate-sdk-side-imports.md) | 迁移批 A —— SDK 侧 | 12 |
| [14](./issues/14-migrate-product-side-imports.md) | 迁移批 B —— 产品侧 | 12 |
| [15](./issues/15-move-analytics-out-of-kernel.md) | 埋点移出内核 | 12 |
| [16](./issues/16-contract-delete-reexports-and-lock-boundary.md) | 收口（contract） | 13, 14, 15 |

> 12 是 expand（零破坏），16 是 contract（删旧路径 + 焊死边界）。
> 中间三张各自独立绿，因为旧路径还在。**15 的删除与补充必须同一次提交。**

## Hub（与内核全部无依赖）

| # | 标题 | Blocked by |
|---|---|---|
| [17](./issues/17-fifth-engine-can-be-constructed.md) | 第五个引擎能被构造出来 | — |
| [18](./issues/18-separate-orchestration-decision-from-bookkeeping.md) | 编排的决策与记账分家 | — |

## 收尾

| # | 标题 | Blocked by |
|---|---|---|
| [19](./issues/19-land-the-decisions-in-adrs.md) | 决策落档（ADR-0008/0009/0010 + 0007 修订） | 02, 03, 04, 06, 11, 16, 17 |

## 依赖图

```text
01 ─┬─ 02 ─────────────────────────────────────────────┐
    ├─ 03 ─────────────────────────────────────────────┤
    ├─ 04 ─────────────────────────────────────────────┤
    ├─ 07                                              │
    ├─ 08                                              │
    ├─ 11 ─────────────────────────────────────────────┤
    └─ 05 ── 06 ─┬─ 09 ─┐                              │
                 ├─ 10 ─┴─ 12 ─┬─ 13 ─┐                │
                 │             ├─ 14 ─┼─ 16 ───────────┤
                 │             └─ 15 ─┘                │
                 └──────────────────────────────────── ┤
                                                       │
17 ────────────────────────────────────────────────────┤
18 (独立，不阻塞任何票)                                 │
                                                      19
```

## 开工建议顺序

无 blocker 的三张可同时开：**01**（闸门，越早越好）、**17**、**18**。
01 完成后 02/03/04/05/07/08/11 全部解锁，是本次并行度最高的一段。
12 是全局收敛点，它之前的所有内核改动都会缩小它要搬的量。
