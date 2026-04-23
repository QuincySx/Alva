# System Prompts 目录

Amp 里所有能还原出来的 prompt 都在这个子目录。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`executor-modes.md`](./executor-modes.md) | 7 个 executor prompt（fwR/kwR/$wR/EwR/MwR/DwR/wwR），即 CLI/TUI 下 "You are Amp, a coding agent" 那套 |
| [`orchestrator-aggman.md`](./orchestrator-aggman.md) | Agg Man 模式的 orchestrator prompt（ampcode.com Web UI） |
| [`subagents.md`](./subagents.md) | Oracle / Librarian / Code Reviewer / Diff Explainer / File Analyzer / Walkthrough |
| [`compaction-recap.md`](./compaction-recap.md) | `/compact` 内部压缩 + `handoff` 跨线程 recap 两套模板 |
| [`assembly-pipeline.md`](./assembly-pipeline.md) | `YwR()` 函数：system prompt 如何动态装配 + SHA-256 分片指纹 |
| [`placeholder-dictionary.md`](./placeholder-dictionary.md) | 二进制里 `${Y8}`/`${P8}`/... 占位符的真实工具名解码表 |

## 总览：模式选择矩阵

```
                Executor          Orchestrator       Subagent (嵌套)
                ─────────         ─────────────     ────────────────
前端            CLI/IDE           ampcode.com web    调 Task/Oracle 时
typing用户      "make me X"       "merge it"          (不见用户)

主 prompt       fwR/kwR/$wR       长版 prompt         独立 prompt
                /EwR/MwR/DwR       (orchestrator      (oracle/librarian/
                /wwR (7 选 1)       的 preamble)        reviewer/...)
                按 mode 路由

工具集          Bash/Read/Grep    $iT/Yg/Qg/Pv/bd    受限（只给必要的）
                edit/create       Slack/GitHub       常加 stop tool
                Task/Oracle/...

关键规则        "do the work"     "不要 poll 进度"    "只返回最后一条"
                                  "fork → handoff"   "通过 Gemini 总结"
```

## 阅读顺序建议

1. 先看 [`executor-modes.md`](./executor-modes.md) —— 这是 Amp 日常行为的核心。
2. 然后 [`placeholder-dictionary.md`](./placeholder-dictionary.md) —— 有这张表才能读懂其他 prompt 里的 `${XX}` 引用。
3. [`assembly-pipeline.md`](./assembly-pipeline.md) —— 理解 prompt 不是静态字符串，是动态拼的。
4. [`subagents.md`](./subagents.md) —— 看 Amp 怎么委托任务给 "下属"。
5. [`compaction-recap.md`](./compaction-recap.md) —— 上下文危机处理。
6. [`orchestrator-aggman.md`](./orchestrator-aggman.md) —— 高阶，Amp 的"多 agent 编排"幕后。
