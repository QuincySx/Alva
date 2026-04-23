---
name: amp-orchestration
description: Amp 的多 agent 编排设计 —— Agg Man (Aggregation Manager) 双 persona、execution threads 指挥链 ($iT/Yg/Qg)、canonical workflow 把 merge/deploy 等高风险动作固化成预定义 prompt。做 agent orchestration 或防 high-stakes action 翻车时加载。
trigger_words:
  - Aggman
  - aggregation manager
  - orchestrator
  - execution thread
  - merge workflow
  - canonical workflow
  - code_review workflow
  - callback
  - Yg tool
  - multi-agent
  - agent orchestration
  - trigger words
  - anti-trigger
  - ship it
---

# Amp Orchestration

Amp 的 "agent 指挥 agent" 架构。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./execution-threads.md` | `$iT` / `Yg` / `Qg` 指挥链 + 安全规则 + 相关 Aggman 工具 | 想懂 orchestrator 怎么指挥 executor |
| `./canonical-workflows.md` | `workflow: "merge_changes"` 固化 prompt 模式 + 触发/反触发词 | 想把 high-stakes action 固化 |

## 核心洞察（不用 load 子文件）

### 两套 persona 区分

| | Executor Amp | Orchestrator Amp (Agg Man) |
|---|---|---|
| 身份 | "coding agent" | "workflow management" |
| 工作方式 | 真改代码 | 指挥别人改代码 |
| 系统 prompt | fwR/kwR/$wR/... | 独立 Aggman preamble |
| 关键工具 | Bash/Read/edit_file/Task/Oracle | `$iT`/`Yg`/`Qg`/`Pv`/`bd` |
| 输出 | 代码改动 | Slack / thread mention |

### 回调模式（不轮询）

```
Orchestrator 调 Yg(targetThread, workflow: "...")
  └→ 回应用户并 stop（硬规则：NEVER poll with Pv）
  
Execution thread 跑完后主动调 Qg(原 orchestrator, result)
  └→ Orchestrator 新 turn 收到回调，报告给用户 / Slack
```

Prompt 明确禁止 poll，要求下游主动 callback。

### Canonical workflow（固化高风险动作）

用户说 "merge it" 时：

1. Orchestrator 调 `Yg({threadID, workflow: "merge_changes"})` —— 不带 content
2. Server 把预存的 canonical prompt (`aqT`) **verbatim** 发给 execution thread
3. Execution thread 按 verbatim prompt 执行

好处：
- **确定性**：同一 workflow 永远一样
- **防 prompt injection**：用户消息里的"顺便 rm -rf"会被完全忽略
- **可版本化**：改 canonical prompt 在 server 端，不用改 agent
- **可审计**：log 里一眼能看见 workflow name

### 触发词白 + 黑双写

```
✅ 明确触发: "merge", "merge it", "merge changes", "ship it", "let's ship it"
❌ 不触发:   "make that change", "do it", "go ahead", "sounds good"
```

Prompt 里**明确列出反例**，比只列正例准确得多。

## Aggman 工具集（速查）

| 变量 | 推断工具名 | 用途 |
|---|---|---|
| `$iT` | `create_execution_thread` | 创建 clean-slate 执行线程 |
| `Yg` | `send_to_execution_thread` | 发消息，带 workflow 参数 |
| `Qg` | `callback` | execution thread 回调 orchestrator |
| `Pv` | `read_thread` | 查 thread 状态（**不能 poll**） |
| `bd` | `search_threads` | DSL 搜索 |
| `XW` / `jiT` | slack 读/写 | Slack 集成 |
| `tAR` | GitHub | 外部 CI / commits |

## 已知 canonical workflows

- `workflow: "merge_changes"` → prompt 变量 `${aqT}`
- `workflow: "code_review"` → prompt 变量 `${uwR}`
- 未来可能：`"deploy"` / `"rollback"` / `"release"`

## 对 Alva 的启发

你们 `Blackboard` + `BoardMessage` + `SpawnScope` 方向对。确认：

- **Orchestrator 和 Executor 用不同 prompt**（`agent_kind` 切换）？
- **`MessageKind::Callback`** 是不是一等类型？
- **Workflow 固化**做成 `WorkflowSkill` 类型（详见 `../alva-learnings/workflow-skill.md`）
- **Trigger / anti-trigger 词表**写进 system prompt
