# Execution Threads —— Orchestrator 的指挥链

---

## 三个核心工具

Orchestrator 模式下的工具集里，最核心的是这三个：

| 工具变量 | 推断工具名 | 用途 |
|---|---|---|
| `${$iT}` | `create_execution_thread` | 创建 clean-slate 执行线程 |
| `${Yg}` | `send_to_execution_thread` | 给已有执行线程发消息 |
| `${Qg}` | `callback` | 执行线程完成后回调 orchestrator |

---

## `create_execution_thread` (`${$iT}`)

**schema（推断）**：

```ts
{
  name: "create_execution_thread",
  inputSchema: {
    type: "object",
    properties: {
      prompt:        { type: "string", description: "Initial prompt for the new thread" },
      repositoryURL: { type: "string" },
      agentMode:     { type: "string", enum: ["smart", "deep", "rush", ...] },
      projectID:     { type: "string" }
    },
    required: ["prompt"]
  }
}
```

**行为**：
- 在后端创建新 thread（默认 DTW executor）
- 返回 `{ url, threadID }`
- orchestrator 应立即回应用户并**停止**，不 poll 进度

**prompt 硬规则**：

```
Use ${$iT} for clean-slate execution and ${Yg} to continue existing work.
After calling ${$iT} or ${Yg}, respond to the user and stop. 
Do NOT poll or loop with ${Pv} to check progress.
```

---

## `send_to_execution_thread` (`${Yg}`)

**schema（推断）**：

```ts
{
  name: "send_to_execution_thread",
  inputSchema: {
    type: "object",
    properties: {
      threadID: { type: "string" },
      content:  { type: "string", description: "Message text (for freeform follow-ups)" },
      workflow: { 
        type: "string", 
        enum: ["merge_changes", "code_review", "deploy", ...]
        description: "Canonical workflow name (sends server-stored prompt verbatim)"
      }
    },
    required: ["threadID"]
    // content 和 workflow 二选一
  }
}
```

### 两种模式

**A) Freeform message**：
```js
Yg({ threadID: "T-xxx", content: "also check the token refresh flow" })
```
消息按字面发给目标 thread。

**B) Canonical workflow**：
```js
Yg({ threadID: "T-xxx", workflow: "merge_changes" })
```
Server 不看 content，查预存的 `aqT`（merge_changes 对应的 prompt 模板），verbatim 发给目标 thread。

---

## `callback` (`${Qg}`)

**schema（推断）**：

```ts
{
  name: "callback",
  inputSchema: {
    type: "object",
    properties: {
      targetThreadID: { type: "string", description: "Thread to notify" },
      summary: { type: "string" },
      status: { type: "string", enum: ["success", "failure", "partial"] },
      artifacts: { 
        type: "array", 
        items: { type: "object" },
        description: "URLs, file paths, commit hashes, etc."
      }
    },
    required: ["targetThreadID", "summary"]
  }
}
```

**用法**：**execution thread 在完成时主动** 调这个工具，告诉 orchestrator 结果。

```
Orchestrator thread → 发 message + 指令调 Qg 回调
   │
   ▼
Execution thread 跑任务
   │
   ▼
Execution thread 完成时调 Qg(orchestrator_thread, summary)
   │
   ▼
Orchestrator 收到新 message，可以进一步处理
   │
   ▼
（可选）Orchestrator 发 Slack 通知等
```

---

## 显式指令调用 `callback`

Orchestrator 在发 message 时要明确告诉 execution thread "完成后 call callback"：

```
When you tell the user you'll do something after a thread finishes 
(for example, "I'll let you know when it's done" or "I'll let you know 
the results"), include an explicit instruction to call ${Qg} when done.

When the user is asking for an answer back (for example, "investigate why 
CI is failing"), include an instruction to call ${Qg} when done so you 
can report the result.
```

**反例**：
```
For fire-and-forget actions with no follow-up (for example, "post this 
to #shipped" or "add a reaction"), do not ask the execution thread to 
call ${Qg}.
```

---

## 指挥链的完整例子

```
User (Slack): "@amp please merge the auth-refactor branch"

Slack webhook → Orchestrator Amp

Orchestrator 思考：
  - 用户想 merge
  - 找到对应的 thread (auth-refactor)
  - 发 canonical merge workflow

Orchestrator 调：
  Yg({
    threadID: "T-auth-refactor-xxx",
    workflow: "merge_changes"
  })
  + "After merging, call ${Qg} with result"

Orchestrator 回用户: "Merge started in thread T-xxx"

[Orchestrator turn 结束]

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[Execution Thread 侧]

Execution Thread 收到 canonical merge prompt (aqT)
  按照 aqT 的步骤：
  1. 检查分支状态
  2. 跑测试
  3. Merge
  4. 调 Qg

Execution Thread 调：
  Qg({
    targetThreadID: "orchestrator_thread_id",
    summary: "Merged auth-refactor into main. 148/148 tests pass. Commit: abc123",
    status: "success"
  })

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[Orchestrator 侧，下一轮]

Orchestrator 收到 callback 消息
  
Orchestrator 调：
  jiT({ channel: "user_slack_channel", thread_ts: "original_ts",
        message: "✓ Merged. Tests 148/148. Commit abc123." })
```

---

## 其他 Orchestrator 专用工具

### `read_thread` (`${Pv}`)

读 thread 内容（不是读进对话，是返回结构化信息）：

```ts
{
  name: "read_thread",
  inputSchema: {
    threadID: { type: "string" }
  }
}
```

**用于**：
- 在 orchestrator 回答用户前，先看目标 thread 当前状态
- 判断 thread 是否还在跑（`thread.isActive`）

### `search_threads` (`${bd}`)

```ts
{
  name: "search_threads",
  inputSchema: {
    query: {
      type: "string",
      description: "DSL query: keywords, file:, repo:, ref:, author:, after:, before:"
    },
    limit: { type: "number", default: 20 }
  }
}
```

同 `amp threads search` CLI 的 DSL（见 `../storage/sync-protocol.md`）。

### `create_project` (`${JCR}`)

```ts
{
  name: "create_project",
  inputSchema: {
    repositoryURL: { type: "string" },
    name: { type: "string" }
  }
}
```

为 repository 创建 v2 project。当 `create_execution_thread` 因 "no matching project" 失败时，orchestrator 先调这个。

### `archive_thread` / `restore_thread` (`${TAR}` / `${RAR}`)

简单的归档 / 恢复。

### `workspace_doc_*` (`${NCR}` / `${UCR}` / `${HCR}`)

读写 workspace 级别的 notes / docs。

### Slack 工具

- `${XW}` = Slack 读（user / channel / thread / reaction）
- `${jiT}` = Slack 发消息

### GitHub 工具

- `${tAR}` = GitHub repository history / commits / diffs / CI

---

## 设计哲学

### 1. **Orchestrator 不干活**

Prompt 硬禁：

```
The user will primarily request you to perform workflow management tasks
—finding threads, creating or replying to existing threads, navigating 
repositories, checking CI, and communicating via Slack—but you should do 
your best to help with any task requested of you.
```

翻译：**你的工作是协调，不是实现**。

### 2. **绝不轮询**

```
After calling ${$iT} or ${Yg}, respond to the user and stop. 
Do NOT poll or loop with ${Pv} to check progress.
```

理由：
- 轮询消耗 orchestrator 的 turn 次数
- Execution thread 可能跑几小时，orchestrator 不能一直挂
- Callback 模型更清晰，谁完成谁报告

### 3. **Status 检查 ≠ 终止**

```
Status/progress checks like "how's it going?" or "ETA?" mean ask for a 
brief update only, not to stop or wrap up early.
```

用户问 "怎么样" ≠ "停下来"。Orchestrator 要能区分。

---

## 对 Alva 的启发

你们 `Blackboard` + `BoardMessage` + `MessageKind` 已经在做这个方向。对照 Amp：

### 1. Callback 是 first-class MessageKind

```rust
pub enum MessageKind {
    UserInput,
    AgentResponse,
    ToolCall,
    ToolResult,
    Callback {                                // ← 新增
        from_agent_id: AgentID,
        to_agent_id: AgentID,
        reference: Option<ToolUseID>,         // 响应哪次调用
    },
    ...
}
```

### 2. Orchestrator 和 Executor 不同 prompt

`BaseAgent::builder()` 可以接受 `agent_kind: AgentKind`:

```rust
pub enum AgentKind {
    Executor,            // 默认
    Orchestrator,        // 用 orchestrator prompt + 受限工具集
}
```

### 3. "no polling" 硬约束

Orchestrator 的 system prompt 明确禁止 poll。否则 LLM 可能为了保险一直轮询。这是 prompt engineering 的关键。

### 4. `SpawnScope` + 跨 scope 消息传递

你们现有的 `SpawnScope` 隔离机制是对的。但 Callback 需要**跨 scope** 发消息（execution thread 回到 orchestrator thread）。确认 Blackboard 支持这种"定向跨 scope"通信。
