# Handoff —— 跨线程开新会话

> Amp 处理"上下文满了"的**首选方式**。LLM 自主决定，不需要用户干预。

---

## 触发方式

不是 harness 定时检查触发，是 **LLM 看到 `handoff` 工具文档后自己决定调**。

工具描述里的触发条件（prompt 里明文）：

```
When to use the handoff tool:
- The current thread is getting too long and context is degrading
- You want to start a new focused task while preserving context from the 
  current thread
- The current thread's context window is near capacity
```

---

## `handoff` 工具 schema

```json
{
  "name": "handoff",
  "description": "<详细触发条件文档>",
  "inputSchema": {
    "type": "object",
    "properties": {
      "goal": {
        "type": "string",
        "description": "A short description of the next task to accomplish in the new thread. Should be a single sentence or at most one paragraph. Focus on what needs to be done next, not what was already completed."
      },
      "follow": {
        "type": "boolean",
        "default": false,
        "description": "If true, navigate to the new thread after creation."
      },
      "mode": {
        "type": "string",
        "description": "Agent mode for the new thread (deep, smart, rush, etc.)"
      }
    },
    "required": ["goal"]
  }
}
```

---

## 执行流程

```
1. LLM 决定调 handoff({goal: "finish rate limiting impl", follow: true})
   │
   ▼
2. Harness 拿到 handoff tool call，不直接新建 thread，而是：
     先内部调 create_handoff_context 工具
   │
   ▼
3. create_handoff_context 的 schema:
     {
       relevantInformation: string,   // first-person recap
       relevantFiles: string[]        // max 10 workspace-relative paths
     }
   LLM 根据模板（见 ../prompts/compaction-recap.md 路径 B）填出这俩字段
   │
   ▼
4. Harness 开新 thread:
     - threadID = 新生成的 T-{uuid}
     - system prompt 正常装配（AGENTS.md + env + 规则 + tools）
     - parent_thread_id = 当前 thread ID
   │
   ▼
5. 往新 thread 发首条 user message:
     goal
     + "\n\n<prior_context>\n" + relevantInformation + "\n</prior_context>"
     + relevantFiles 的内容（每个读一下）
   │
   ▼
6. 新 thread 自己跑循环
   │
   ├── follow: true  → TUI 切到新 thread
   └── follow: false → 新 thread 后台跑，老 thread 也继续
```

---

## Handoff Context Prompt（完整）

```
Extract relevant context from the conversation above for continuing this work.
Write from my perspective (first person: "I did...", "I told you...").

Consider what would be useful to know based on my request below. Questions 
that might be relevant:
- What did I just do or implement?
- What instructions did I already give you which are still relevant 
  (e.g. follow patterns in the codebase)?
    - What files did I already tell you that's important or that I am 
      working on (and should continue working on)?
- Did I provide a plan or spec that should be included?
- What did I already tell you that's important (certain libraries, patterns,
  constraints, preferences)?
- What important technical details did I discover (APIs, methods, patterns)?
- What caveats, limitations, or open questions did I find?

Extract what matters for the specific request below. Don't answer questions 
that aren't relevant. Pick an appropriate length based on the complexity of 
the request.

Focus on capabilities and behavior, not file-by-file changes. Avoid excessive
implementation details (variable names, storage keys, constants) unless critical.

Format: Plain text with bullets. No markdown headers, no bold/italic, no 
code fences. Use workspace-relative paths for files.
```

---

## `relevantFiles` 规则

```
- Maximum 10 files. Only include the most critical files needed for the task.
- You can include directories if multiple files from that directory are needed
- Prioritize by importance and relevance. PUT THE MOST IMPORTANT FILES FIRST.
- Return workspace-relative paths (e.g., "core/src/threads/thread.ts")
- Do not use absolute paths or invent files
```

---

## CLI 入口

```bash
# 用户可以手动触发 handoff（bypass LLM 决策）:
echo "Continue working on the auth feature" | amp threads handoff T-xxx
amp threads handoff T-xxx --goal "Continue working on the auth feature"
amp threads handoff --goal "Fix the remaining tests"  # Uses last thread

# --print 只打印新 thread ID 不进 TUI
amp threads handoff --goal "..." --print
```

---

## 设计哲学

### 为什么 handoff 比 /compact 好

1. **用户感知自然**：像"新开个对话继续"，不像"被吞了一段"。
2. **可追溯**：旧 thread 完整保留，想看原 context 点一下 thread list。
3. **可回退**：handoff 做错了（recap 丢了关键信息）？回老 thread 重来。
4. **LLM 自决定比阈值更准**：模型能感知 "我开始回答不准了"，阈值感知不到。

### 为什么要让 LLM 用工具触发（不是 harness）

工具化的好处：
1. LLM 的决策过程可以被 prompt engineering 调整（"你什么时候应该 handoff"）
2. 同一套机制支持"用户主动"（CLI flag）和"LLM 自决定"两种路径
3. handoff 能带 `goal` 参数，让新 thread 有明确入口

### 为什么 recap 要第一人称

```
✗ "The user asked me to add auth. I implemented JWT..."
✓ "I asked you to add auth to the API. You implemented JWT in src/auth/..."
```

新 thread 的 LLM 读到第二种，像真的是用户写的首条 message。减少"前一个 agent 的摘要"的 meta 感，更自然地接着对话。

---

## `fork` → `handoff` 的演进

Amp 曾经有 `fork` 命令，复制 thread 的完整 history 分叉。后来**主动废弃**了：

```
The fork command has been deprecated.
Fork has been replaced by handoff and thread mentions.
See: https://ampcode.com/news/stick-a-fork-in-it
```

理由（推断）：
- fork 带完整 history 过去，新 thread 没省 context
- 用户不知道什么时候该 fork（技术细节）
- handoff 让 LLM 决策，用户不用懂

---

## 和 Task subagent 的区别

两者都是"派生新的 context"，但：

| 维度 | `handoff` | `Task` subagent |
|---|---|---|
| 层级 | 同级 thread | 嵌套子 agent |
| Parent 存活 | 是（旧 thread 保留） | 是（父 agent 等子返回） |
| 用户可见 | 是（是个正式 thread）| 否（只是一次 tool call）|
| 结果合并 | 不合并（是独立 thread）| 合并（summary 回给父）|
| 用途 | 换话题 / context 满 | 委托探索任务 |

---

## 对 Alva 的启发

详细设计见 [`../alva-learnings/handoff-tool.md`](../alva-learnings/handoff-tool.md)。

核心要点：

1. **`handoff` 做成 first-class tool**（不是 harness 隐式逻辑）
2. **Two-step** 流程：`handoff` 触发后调 `create_handoff_context`
3. **prompt 模板直接抄 Amp**（经过验证）
4. **支持 follow / print 两种 CLI 模式**（交互 vs CI 友好）
5. **新 thread 继承 parent_thread_id 字段**（追溯链）
