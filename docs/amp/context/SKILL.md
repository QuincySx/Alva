---
name: amp-context
description: Amp 上下文管理的完整设计 —— 为什么 300K 看起来干净。4 层策略、fileChangeTracker、/compact、handoff、context 诊断命令。需要优化自己 agent 的 token 占用或理解"自动压缩 vs handoff"决策时加载。
trigger_words:
  - context management
  - token budget
  - 上下文管理
  - 上下文压缩
  - compaction
  - /compact
  - handoff
  - auto compact
  - context window
  - prompt caching
  - cache hit rate
  - file change tracker
  - context degradation
  - context 诊断
  - thread mention
  - thread mentions
  - read_thread
  - "@T-"
  - cross-thread retrieval
---

# Amp Context Management

Amp 怎么让 300K 上下文看起来始终干净。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./strategy.md` | 4 层总策略（工具截断 / 懒加载 / 子 agent / handoff） | 想懂完整图景 |
| `./file-change-tracker.md` | 每次 edit 的 before/after 追踪 + undo | 想懂 undo_edit / tracker 数据结构 |
| `./in-thread-compact.md` | `/compact` slash command 原地压缩 | 想懂同线程压缩 |
| `./handoff.md` | LLM 自决定开新 thread + 带 recap | 想懂 handoff 工具 |
| `./thread-mentions.md` | `@T-xxx` 跨 thread 引用 + `read_thread` subagent 压缩 | 想懂跨 thread retrieval / `fork` 的另一条替代路径 |
| `./diagnostics.md` | `amp context` 诊断命令 + SHA 分片监控 | 想加类似命令到自己产品 |

## 4 层策略（速查表）

| Layer | 机制 | 省 tokens（估）|
|---|---|---|
| 1. 输入端截断 | Read 500 行 × 4K 字节/行、MCP 结果 KB 上限、AGENTS.md 32 KiB | ~20% |
| 2. 不默认加载 | Skills/MCP 懒加载、fileChangeTracker 独立存储、Prompt cache 对齐 | ~30% |
| 3. 子 agent 吸收 | Task/Oracle/finder/Librarian 独立 context，Gemini Flash 压总结 | ~40% |
| 4. Handoff | LLM 自决定开新 thread（不是压缩，是重置） | 100%（重置）|

## 关键决策（Amp 明确做了的选择）

- **没有自动 compact** —— 全手动 `/compact` + LLM 主动 `handoff`
- **子 agent 总结用便宜模型** —— Gemini 3 Flash，不是主模型
- **Read 返回 `trackFiles: [uri]`** —— 每次读都登记，为 tracker / diff 展示做准备
- **Usage 每轮 snapshot 到 assistant message** —— 不重算 tokens，读最后一条 usage 字段即可
- **Prompt caching 是一等关注对象** —— SHA 分片 + 用 cache_read_input_tokens 对比监控

## 常见问答（不用 load 子文件）

**Q：Amp 自动触发 /compact 吗？**
A：**不**。反编译里**完全没找到自动触发代码**。只能用户 slash command 或 LLM 主动 handoff。

**Q：/compact 和 handoff 什么区别？**
A：/compact 同线程原地替换 messages（用 5-section markdown summary）；handoff 开新 thread（first-person recap + top 10 files），旧 thread 保留。

**Q：Thread mention (`@T-xxx`) 和 handoff 什么区别？**
A：handoff 是"开新对话、旧 context 压成 recap 带过去"；thread mention 是"当前对话里引用另一个 thread 的某些部分"。mention 不建立 parent-child 关系，是一次性 goal-driven retrieval（`read_thread` 工具 + subagent 提取）。详见 `./thread-mentions.md`。

**Q：子 agent 的 work log 去哪了？**
A：Task 完成时，harness 用 Gemini 3 Flash 把整个 log 压成 summary，返回给父 agent。父**永远看不到**子 agent 的 tool call 细节。

**Q：cache hit rate 怎么监控？**
A：每条 assistant message 的 `usage.cacheReadInputTokens` 直接来自 Anthropic API。对比 `cacheCreationInputTokens` 判断命中率。`amp context` 命令做这事。

**Q：怎么知道哪个 block 导致 cache miss？**
A：`FmT(threadID, newHashes, ...)` 分片 SHA 对比，变化的 key 打 debug log，附原文。见 `../prompts/assembly-pipeline.md`。
