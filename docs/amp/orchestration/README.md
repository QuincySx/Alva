# Orchestration 目录

> Amp 的 "agent 指挥 agent" 模式：Aggman + Canonical Workflows。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`execution-threads.md`](./execution-threads.md) | `$iT` / `Yg` / `Qg` 指挥链 |
| [`canonical-workflows.md`](./canonical-workflows.md) | merge_changes / code_review 固化 prompt 模式 |

## 核心概念

**Agg Man (Aggregation Manager)** 是 Amp 在 ampcode.com 上的"编排模式"。它的工作不是写代码，是**调度写代码的 agent**。

```
用户 "merge it"
   │
   ▼
Agg Man (Orchestrator)
   │ 调 Yg(targetThread, workflow: "merge_changes")
   ▼
Server 把 canonical merge prompt 发给 execution thread
   │
   ▼
Execution Thread (另一个 Amp，在 DTW / local 上)
   │ 按 canonical prompt 执行
   │ 完事后调 Qg(原 orchestrator thread, result)
   ▼
Orchestrator 收到回调
   │
   ▼
往 Slack / 用户 UI 报告结果
```

## 两个独立贡献

### 1. 两个 persona

Orchestrator 和 Executor 用**不同 system prompt** 和**不同工具集**。orchestrator 的 prompt 硬禁 "you should not do the work yourself" —— 防止 orchestrator 抢活干。

详见 [`../prompts/orchestrator-aggman.md`](../prompts/orchestrator-aggman.md)。

### 2. 固化 workflow

高风险动作（merge / ship / deploy）**不让 LLM 即兴写 prompt**。orchestrator 只决定**什么时候触发**，具体的 prompt 是 server 端预定义的 `workflow: "name"` 参数。

详见 [`canonical-workflows.md`](./canonical-workflows.md)。

## 对 Alva 的 Blackboard / SpawnScope 的启示

你们 `alva-agent-context::scope` 有 Blackboard + SpawnScope + SessionTracker。方向对，但要对照以下细节：

- **Orchestrator 和 Executor 用不同 prompt** —— 你们的 `BaseAgent` 要不要支持 "agent_kind" 切换 prompt 套件？
- **回调 vs 轮询** —— Amp prompt 硬禁 orchestrator poll，要求下游主动 callback。你们的 `MessageKind` 有没有显式的 Callback 类型？
- **Workflow 固化** —— high-stakes action 做成 `WorkflowSkill` 类型（详见 `../alva-learnings/workflow-skill.md`）
