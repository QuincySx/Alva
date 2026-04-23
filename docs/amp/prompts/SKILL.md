---
name: amp-prompts
description: Amp 所有 system prompts 原文和装配逻辑。覆盖 7 个 executor 模式、Agg Man orchestrator、5 种 subagent、compaction/handoff 两套 recap 模板、装配 pipeline 和变量解码。需要读 Amp prompt 原文或理解 prompt 怎么拼出来时加载。
trigger_words:
  - prompt
  - system prompt
  - fwR
  - kwR
  - oracle prompt
  - librarian
  - executor mode
  - hardcore
  - rush mode
  - pair programming
  - compaction prompt
  - handoff recap
  - prompt 装配
  - prompt caching
  - SHA 分片
---

# Amp Prompts

所有从二进制提取的 prompt 原文 + 装配逻辑。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./executor-modes.md` | 7 个 executor prompt (fwR/kwR/$wR/EwR/MwR/DwR/wwR) 原文 + 主题对照 | 想看 Amp 日常 CLI 行为的 prompt |
| `./orchestrator-aggman.md` | Agg Man orchestrator prompt 全文 + 硬规则 | 想看 Amp 在 ampcode.com web UI 下的行为 |
| `./subagents.md` | Oracle / Librarian / Code Reviewer / Diff Explainer / File Analyzer / Walkthrough 三阶段 | 想看子 agent prompt |
| `./compaction-recap.md` | /compact 5-section 模板 + handoff first-person recap | 想看 Amp 怎么让模型做摘要 |
| `./assembly-pipeline.md` | YwR() 装配函数 + SHA-256 分片指纹 (zmT/FmT) | 想看 prompt 动态装配 + caching 监控 |
| `./placeholder-dictionary.md` | `${Y8}` / `${P8}` 等符号 → 真实工具名映射 | 读其他文件时遇到 `${xx}` 占位符 |

## 快速决策树

**遇到 `${XX}` 不知道指什么？** → `placeholder-dictionary.md`

**想看某个 prompt 原文？**
- Pair programming (default) → `executor-modes.md` 的 fwR
- 硬核 guardrails 模式 → `executor-modes.md` 的 $wR
- XML-structured → `executor-modes.md` 的 MwR
- Rush / 1-3 词回答 → `executor-modes.md` 的 wwR
- Orchestrator (Agg Man) → `orchestrator-aggman.md`
- Oracle 高推理顾问 → `subagents.md` 的 Oracle 节
- 跨仓库代码理解 → `subagents.md` 的 Librarian 节
- 代码审查 → `subagents.md` 的 Code Reviewer 节
- 单文件分析 (Gemini Flash) → `subagents.md` 的 File Analyzer 节
- /compact 压缩 → `compaction-recap.md` 路径 A
- Handoff 跨线程 → `compaction-recap.md` 路径 B

**想看 prompt 装配逻辑？** → `assembly-pipeline.md`（YwR 函数 + 每块 SHA 分片）

## 核心洞察（不用 load 子文件就能用）

1. **两套不同人设**：CLI 模式是 executor；Web UI 是 orchestrator。system prompt 完全不同，工具集也不同。
2. **7 种 executor prompt 按 agent mode 路由**：smart→fwR, deep→$wR, rush→wwR, speed→DwR 等。
3. **AGENTS.md 32 KiB 硬预算**：超了 warn + 截断。
4. **SHA 分片指纹**：每个 prompt block 独立 SHA，FmT 对比变化用于 debug prompt caching 失效。
5. **子 agent "Only your last message is returned"**：父 agent 只看最终总结，不看过程。
