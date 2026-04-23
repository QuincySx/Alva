# Compaction 与 Handoff Recap 提示词

Amp 有**两套完全不同的**"上下文转移" prompt。什么时候用哪套要分清楚。

---

## 路径 A：`/compact` —— 同线程内原地压缩

**触发**：slash command `/compact`，用户主动触发。

**机制**：让 LLM 自己把整个 message history 压缩成一段 `<summary>...</summary>`，然后用这段 summary 替换老消息。**线程 ID 不变，message 数量骤降。**

**完整 prompt**：

```
## 1. Primary Request
- The user's core request and success criteria
- Any clarifications or constraints they specified

## 2. Progress So Far
- What has been completed so far
- Files created, modified, or analyzed (with paths if relevant)
- Key outputs or artifacts produced

## 3. Important Discoveries
- Technical constraints or requirements uncovered
- Decisions made and their rationale
- Errors encountered and how they were resolved
- What approaches were tried that didn't work (and why)

## 4. Next Steps
- Specific actions needed to complete the task
- Any blockers or open questions to resolve
- Priority order if multiple steps remain

## 5. Context to Preserve
- User preferences or style requirements
- Domain-specific details that aren't obvious
- Any promises made to the user

Be concise but complete—err on the side of including information that would 
prevent duplicate work or repeated mistakes. Write in a way that enables 
immediate resumption of the task.

Wrap your summary in <summary></summary> tags.
```

**输出**：一段 markdown summary 包在 `<summary>` tag 里，替换老 messages。

---

## 路径 B：`handoff` —— 跨线程开新会话

**触发**：LLM 自主决定调用 `handoff` 工具。prompt 里告诉它什么时候该调：

> - The current thread is getting too long and context is degrading
> - You want to start a new focused task while preserving context from the current thread
> - The current thread's context window is near capacity

**机制**：内部调用 `create_handoff_context` 工具，让 LLM 用**第一人称**重写上下文：

### Handoff Recap prompt

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

### `create_handoff_context` 工具 schema

```json
{
  "name": "create_handoff_context",
  "description": "A tool to extract relevant information from the thread and select relevant files for another agent to continue the conversation. Use this tool to identify the most important context and files needed.",
  "inputSchema": {
    "properties": {
      "relevantInformation": {
        "type": "string",
        "description": "Extract relevant context from the conversation. Write from first person perspective ('I did...', 'I told you...')... Focus on capabilities and behavior, not file-by-file changes. Avoid excessive implementation details (variable names, storage keys, constants) unless critical. Format: Plain text with bullets. No markdown headers, no bold/italic, no code fences."
      },
      "relevantFiles": {
        "type": "array",
        "items": { "type": "string" },
        "description": "An array of file or directory paths (workspace-relative) that are relevant to accomplishing the goal. IMPORTANT: Return as a JSON array of strings, e.g., ['lib/services/web_filtering_service.dart', 'ios/Runner/AppDelegate.swift']\n- Maximum 10 files. Only include the most critical files needed for the task.\n- You can include directories if multiple files from that directory are needed\n- Prioritize by importance and relevance. PUT THE MOST IMPORTANT FILES FIRST.\n- Return workspace-relative paths\n- Do not use absolute paths or invent files"
      }
    },
    "required": ["relevantInformation", "relevantFiles"]
  }
}
```

### 触发后的完整流程

```
1. Main agent 调 handoff({goal, follow: boolean})
   │
   ▼
2. Harness 内部调 create_handoff_context
   - 让 LLM 产出 {relevantInformation, relevantFiles}
   │
   ▼
3. 开新 thread（child thread，可选 follow mode）
   - system prompt 正常装配（AGENTS.md + env + 规则 + 工具）
   - 首条 user message =
       goal
       + relevantInformation (first-person recap)
       + relevantFiles 内容
   │
   ▼
4. 老 thread 可继续跑（parent 不终止）
   - 用户如果 follow=true，UI 切到新 thread
   - 用户如果 follow=false，新 thread 在后台跑
```

---

## 路径 C：Task Subagent 完成时自动总结

**触发**：Task subagent 结束时（不是 LLM 决定，是 harness 决定）。

**机制**：harness 把整个 subagent 的 turn log 喂给 **Gemini 3 Flash**，用固定 JSON schema 输出 summary。

**调用配置**：

```js
j$(xU, [
  { role: "user", parts: [{ text: workLog }] },
  { role: "user", parts: [{ text: summaryPrompt }] }  // 即上面 1-5 节模板
], ..., {
  responseMimeType: "application/json",
  responseJsonSchema: zpT.toJSONSchema(),
  thinkingConfig: { thinkingLevel: "MINIMAL" }
});
```

**用意**：主 agent 永远看不到 Task subagent 的 tool call 细节，只看到 `{ summary, filesChanged, commandsRun, otherToolsUsed }` 结构体。

---

## 三种路径对比

| 维度 | /compact | handoff | Task summary |
|---|---|---|---|
| 触发方 | 用户 slash | LLM 自决定 | Harness |
| 位置 | 同线程原地 | 新子线程 | Tool result |
| Summary 作者 | 当前 LLM 自己 | 当前 LLM 自己 | Gemini 3 Flash |
| 保留线程 ID | ✅ | 旧 ID 仍存在 + 新 ID | 无线程，只是工具结果 |
| 输出格式 | Markdown `<summary>` | First-person bullets + files | 结构化 JSON |
| 上下文恢复粒度 | 整线程 | 只摘关键信息 + top 10 文件 | 只保留 key findings |

---

## 设计洞察

1. **`/compact` 用 Markdown headers**（`## 1. Primary Request`），handoff 用 **plain text bullets**。这是因为 `<summary>` 会作为"消息体"嵌入后续对话，而 handoff 的 recap 是**新线程的首条 user message**，需要更口语化、更像用户直接说的话。

2. **First-person 视角**是 handoff 特色。"I did..." "I told you..." 让新 thread 的 LLM 读起来像真的用户说的，减少"上一个 agent 的摘要"这层间接感。

3. **`relevantFiles` 限制 10 个**。超过用户会懵，不够又丢上下文。10 是经验值。

4. **"Focus on capabilities and behavior, not file-by-file changes"** —— recap 不重演 diff。diff 在 git 里就能看到，recap 要保留的是**意图和约束**。

5. **Task subagent summary 用 Gemini**，因为子 agent 做的是探索类工作，主 agent 只要结论。用便宜模型压缩，不损失核心 signal。
