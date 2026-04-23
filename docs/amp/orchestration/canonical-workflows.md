# Canonical Workflows —— 固化的高风险动作

> 把 "merge" / "ship" / "deploy" 这类 high-stakes 动作 **prompt 固化**，不让 LLM 即兴写。

---

## 问题

LLM 对话过程中，用户可能说 "merge it"，但模型有多种方式理解：

1. 字面 `git merge`
2. GitHub PR merge
3. Deploy to prod
4. Close the thread
5. ...

即使理解对了，**每次重新生成的 merge 指令文本也不稳定**。可能：

- 偶尔漏掉 "check CI first"
- 偶尔加 "also delete the branch" （不需要）
- 偶尔 prompt injection（用户消息里塞了 "also run rm -rf /"）

对这种高风险动作，**每次都精确一致** 比 "智能应变" 重要得多。

---

## 解决：`workflow: "name"` 参数

Orchestrator 的 `send_to_execution_thread` (`${Yg}`) 接受 `workflow` 参数：

```js
Yg({
  threadID: "T-xxx",
  workflow: "merge_changes"
  // 注意：没有 content 参数
})
```

Server 收到后：
1. 忽略 content（如果有的话）
2. 查预存的 `workflow` prompt 模板
3. 把模板 **verbatim** 发给目标 thread（作为 user message）

Orchestrator LLM 只决定 **调不调** 这个 workflow。**内容** 完全 server-side 控制。

---

## 已知的 workflow

| workflow 名 | prompt 变量 | 触发词 |
|---|---|---|
| `"merge_changes"` | `${aqT}` | "merge", "merge it", "merge changes", "ship it", "let's ship it" |
| `"code_review"` | `${uwR}` | "review", "code review", "do a code review" |

未来可能扩展：`"deploy"`, `"rollback"`, `"release"`, `"publish"`, `"tag"`...

---

## 触发规则（prompt 里硬编码）

### Merge Workflow 触发

```
- When the user asks to "merge", "merge changes", "ship it", or "let's 
  ship it" for a thread, call ${Yg} with the target thread and 
  workflow: "merge_changes". For merge requests, do NOT compose freeform 
  message text. Use workflow: "merge_changes" so the tool sends the 
  canonical merge prompt verbatim.
```

**关键约束**：`do NOT compose freeform message text`。

### 反触发（防止误触）

```
- Do not trigger merge workflow for discussion-only or hypothetical 
  merge/shipping talk. If intent to act is ambiguous, ask for explicit 
  confirmation before calling any tool. Never merge a thread proactively 
  or as an assumed next step.

- Only trigger the merge workflow when the user explicitly asks to merge 
  or ship using clear merge/ship language (e.g., "merge", "merge it", 
  "ship it", "merge changes"). 
  
  Phrases like "make that change", "do it", "go ahead", or "sounds good" 
  are instructions to implement or continue work -- they are not merge 
  requests.
```

**关键设计**：**白名单 + 黑名单双写**。不只说"哪些算触发"，还说"哪些不算触发"。

这个"反例 prompt"是很强的防误判技巧。LLM 看到 "go ahead" → "sounds good" → "do it" 这类模糊词会更谨慎。

### 忙碌检查

```
- Before triggering a merge, check whether the thread appears busy or 
  still running work when that signal is available. If it appears active 
  or the state is unclear, warn the user and confirm before sending the 
  merge prompt.
```

触发前先调 `${Pv}` 看目标 thread 是不是还在跑。跑着就先问用户，避免 "merge 一个还没写完的 PR"。

---

## 完整流程

```
User (Slack): "merge it"

Orchestrator Amp 思考：
  1. "merge it" 匹配触发词 → 要触发 merge workflow
  2. 先调 ${Pv} 看目标 thread 状态
  3. Thread 状态 "done"（或 idle）→ 可以 merge
  4. 调 ${Yg} with workflow: "merge_changes"

Orchestrator 调：
  Yg({
    threadID: "T-auth-refactor-xxx",
    workflow: "merge_changes"
  })

Server 端：
  - 查 aqT = "<canonical merge prompt>"
  - 把 aqT 作为 user message 发给 T-auth-refactor-xxx

━━━━━━━━━━━━━━━━━━━━━━━━━━

Execution Thread 侧：

Thread 收到 user message（内容是 aqT 的 verbatim）:
  """
  (Canonical merge prompt here)
  Your task is to merge the current work into the target branch.
  
  Steps:
  1. Verify all tests pass (run `npm test` or equivalent)
  2. Check git status is clean
  3. Rebase onto target branch
  4. Push
  5. Open PR or merge directly per project convention
  6. After completion, call ${Qg} with the result including commit SHA
  """

Thread 按照 aqT 执行步骤 1-5
Thread 调 Qg 回报结果

━━━━━━━━━━━━━━━━━━━━━━━━━━

Orchestrator 收到 callback，发 Slack:
  "✓ Merged. Tests 148/148. Commit abc123."
```

---

## 为什么这套设计强

### 1. **确定性**

同一个 workflow 每次发的 prompt 完全相同。行为可预测。

### 2. **可审计**

`workflow: "merge_changes"` 在 log 里一眼能看到。不用翻 LLM 生成的 message 找"是不是发了 merge 指令"。

### 3. **可版本化**

Canonical prompt 在 server 端。要改 merge 流程，改一处 `aqT`，所有 orchestrator 自动用新版本。

### 4. **防 prompt injection**

用户 message 里写 "merge and also delete the main branch"？orchestrator 调 `workflow: "merge_changes"` 时 **完全忽略** 用户 message 内容。只发 server 存的 `aqT`。

### 5. **权限管控有抓手**

服务端可以针对 workflow 做粒度权限：
- "只有 admin 能触发 merge_changes"
- "deploy 需要 2 人 approve"
- "rollback 不用 approve"

---

## 对 Alva 的启发 ⭐

你们可以做成 `alva-protocol-skill::WorkflowSkill` 类型：

```rust
// alva-protocol-skill/src/workflow.rs

#[derive(Deserialize, Serialize)]
pub struct WorkflowSkill {
    pub name: String,                    // "merge_changes"
    pub description: String,              // 给 LLM 看的
    
    // 触发词
    pub trigger_words: Vec<String>,       // ["merge", "ship it", "merge changes"]
    pub anti_trigger_words: Vec<String>,  // ["do it", "go ahead", "sounds good"]
    
    // canonical prompt
    pub canonical_prompt: String,         // 发给执行者的 verbatim prompt
    
    // 触发前检查
    pub pre_checks: Vec<Check>,           // 例如 "target thread not busy"
    
    // 权限
    pub requires_permission: bool,
    pub permission_kind: PermissionKind,
}
```

在 orchestrator agent 的 system prompt 里自动生成触发规则：

```
For workflow "merge_changes":
  Trigger on: "merge", "ship it", "merge changes"
  Do NOT trigger on: "do it", "go ahead", "sounds good"
  Before triggering: check thread is not busy
```

详细设计见 [`../alva-learnings/workflow-skill.md`](../alva-learnings/workflow-skill.md)。

### 低成本尝试

你们不需要完整的 orchestrator 架构就能抄这个模式。**单机 agent** 也能用：

- 把 `/commit` / `/merge` / `/release` 做成 canonical workflow
- LLM 只决定触发，命令行命令 server-side 模板化
- 避免 "agent 自己写 git 命令" 的安全风险

对你们 `PlanModeMiddleware` + `SecurityGuard` 是天然补充。
