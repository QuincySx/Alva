# Context 管理目录

> Amp 的上下文管理是**系统性工程**，不是单个聪明算法。这个目录讲清楚所有机制。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`strategy.md`](./strategy.md) | 四层总策略（工具截断 + tracker + /compact + handoff） |
| [`file-change-tracker.md`](./file-change-tracker.md) | 每次 edit 的细粒度追踪 + undo |
| [`in-thread-compact.md`](./in-thread-compact.md) | `/compact` slash command |
| [`handoff.md`](./handoff.md) | LLM 自决定开新线程 |
| [`diagnostics.md`](./diagnostics.md) | `amp context` 诊断命令 + prompt caching 监控 |

## 核心洞察

**Amp 不做"自动压缩"**。它用 4 层叠加把上下文控制在可控范围：

1. **工具输入端截断** —— Read 默认 500 行、MCP 结果有 KB 上限
2. **fileChangeTracker** —— 每次 edit 记录 before/after，做 undo + 聚合统计
3. **`/compact`** —— 手动 slash command 压缩当前 thread
4. **handoff** —— LLM 自主开新 thread（不是压缩，是**重置**）

每一层都单独节省不了太多，但**叠加起来让"300K 上下文很干净"成为可能**。

## 快速查询

**"为什么读了很多文件上下文还干净？"**
→ [`strategy.md`](./strategy.md) 的"4 层防线"

**"Amp 怎么记得哪些文件改了？"**
→ [`file-change-tracker.md`](./file-change-tracker.md)

**"/compact 是 Amp 自动触发的吗？"**
→ 不是，是用户 slash command。详见 [`in-thread-compact.md`](./in-thread-compact.md)

**"Amp 在上下文快满时会自动压缩吗？"**
→ 不会。它依赖 LLM 自己调 `handoff` 工具开新 thread。详见 [`handoff.md`](./handoff.md)

**"我怎么知道上下文被什么占满了？"**
→ `amp context` 命令。详见 [`diagnostics.md`](./diagnostics.md)
