# `/compact` —— 同线程原地压缩

> 用户主动触发的 slash command。把当前 thread 的 history 压成 summary。

---

## 触发

```
用户在 TUI 输入: /compact
  │
  ▼
slash command 派发器
  │
  ▼
执行 compaction pipeline
```

**注意**：Amp **没有自动 /compact**（没找到阈值触发逻辑）。只能用户手动。

---

## Pipeline

```
1. 读当前 thread 的所有 messages
   │
   ▼
2. 组装压缩 prompt（见 ../prompts/compaction-recap.md）
   │
   ▼
3. 调 LLM：
     messages = 原 messages + [user: "<summary_prompt>"]
   │
   ▼
4. LLM 输出 <summary>...</summary>
   │
   ▼
5. 从输出里 extract summary 内容
   │
   ▼
6. 替换 thread messages:
     新 messages = [
       原来的第一条 user message(保留),
       info message: "[此处是历史压缩摘要]",
       assistant: summary,
       info: "[恢复对话]",
     ]
   │
   ▼
7. flushVersion 到 thread service
```

---

## 完整的 Compaction Prompt

(参考 `../prompts/compaction-recap.md`)

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

---

## 替换后的 message 长什么样

压缩前（假设）：
```
user:      "add auth to the API"
assistant: (text + 20 tool_uses reading files)
assistant: (text + 8 tool_uses writing edit_file)
assistant: "Done. Added JWT auth to POST /login and /signup."
user:      "now add rate limiting"
assistant: (text + 12 tool_uses)
...
```

压缩后：
```
user:      "add auth to the API"  [原样保留]
info:      "[Thread was compacted. Prior context summarized below.]"
assistant: "<summary>
## 1. Primary Request
Add auth to API, then add rate limiting.
## 2. Progress So Far
- Added JWT auth: src/auth/jwt.ts:12-45, src/routes/login.ts:8-22
- Started on rate limiting: src/middleware/rate-limit.ts (WIP)
## 3. Important Discoveries
- Redis is already used for sessions, use it for rate limiting too
- User wants sliding window, not fixed window
## 4. Next Steps
- Finish rate-limit.ts implementation
- Add tests in test/rate-limit.test.ts
## 5. Context to Preserve
- User prefers named exports over default
- All errors go through src/errors/AppError.ts
</summary>"
info:      "[Continue the conversation normally.]"
(用户后续的 message 继续追加在这后面)
```

---

## 设计细节

### 为什么要保留第一条 user message？

让 thread 的"入口话题"仍可见。用户看 thread list 时靠它识别。

### 为什么用 `<summary>` tag 而不是直接 markdown？

- LLM 对 XML tag 有 strong attention
- 未来如果要 re-compact，容易解析定位旧 summary
- UI 可以特殊渲染（折叠 / 高亮）

### 为什么是 5 个固定 section？

Amp 总结了高价值信息的类别：
1. **用户想要什么**（后续推理的根基）
2. **已经做了什么**（避免重做）
3. **学到了什么**（避免重犯错）
4. **还要做什么**（Next steps）
5. **用户偏好**（避免重问）

这 5 类覆盖了大部分"pickup-where-you-left-off"场景。

---

## 和 Task subagent summary 的区别

| 维度 | `/compact` | Task subagent summary |
|---|---|---|
| 触发 | 用户 slash | Task 完成时自动 |
| 执行者 | 主 agent 自己 | Gemini 3 Flash |
| 输出格式 | Markdown 带 `<summary>` tag | 结构化 JSON |
| 作用 | 替换当前 thread messages | 作为 tool_result 回给父 agent |
| Schema 强制 | 无（LLM 自由发挥）| 有（Zod schema）|

---

## 为什么 Amp 不自动触发？

从反编译中**完全没找到**自动触发的阈值检查代码。推断原因：

1. **准确判断时机难**：80% context 可能还能跑，95% 可能已经吐错答。
2. **用户失控感强**：对话中间被"吞掉"一段，体验差。
3. **handoff 是更好的替代**：与其压缩，不如开新 thread。

Amp 把判断权交给 LLM 自己（通过 `handoff` tool）或用户（通过 `/compact` slash command）。

---

## 对 Alva 的启发

你们 `alva-agent-context` 有 `compact.rs` + `auto_compact.rs` + `CompactionMiddleware`。对照 Amp：

1. **自动 compact 是否必要？** Amp 的实验结论是 no。建议把 `auto_compact` 做成**可选 extension**而不是默认装载，让用户可以选 "手动 /compact" 或 "自动阈值触发"。

2. **`CompactionMiddleware` 的 prompt 模板**：可以直接抄 Amp 的 5 节模板，已经经过生产验证。

3. **hook-driven 路径**：压缩 prompt 和触发时机应该分离。你们的 `ContextHooks` 8 钩子设计允许这种分离，充分用起来。
