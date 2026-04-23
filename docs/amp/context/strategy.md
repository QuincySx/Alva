# 四层上下文管理策略

> 为什么 Amp 的 300K 上下文看起来始终很干净？答案是系统性工程，不是某个聪明的算法。

---

## 总览

```
┌──────────────────────────────────────────────────────────┐
│  Layer 1: Input-side Truncation                           │
│  工具在写入 context 之前就限长                              │
│                                                            │
│  Read: 500 行 × 4096 字节/行                               │
│  MCP tool result: KB 上限 → 超了截断 + 提示                 │
│  Bash output: maxBufferBytes → 截断并 warn                 │
│  Directory listing: 20 entries + 剩余计数                  │
│  AGENTS.md: 32 KiB 硬预算                                   │
└──────────────────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────┐
│  Layer 2: Never Duplicate                                  │
│  不重发已有内容                                             │
│                                                            │
│  Skills: 只挂 name+description，不挂内容（load_skill 才加）│
│  MCP tools: includeTools glob 过滤（省 90%+）              │
│  Prompt caching: SHA 分片让 history 绝大部分命中缓存        │
│  fileChangeTracker: edit 历史独立管理，不全塞 message       │
└──────────────────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────┐
│  Layer 3: Subagent Absorption                              │
│  让子 agent 吃掉探索成本                                     │
│                                                            │
│  Task / Oracle / finder / Librarian 都在独立 context 跑     │
│  完成后用 Gemini 3 Flash 压成一段总结                        │
│  主 agent 永远看不到子 agent 的 tool call 细节              │
└──────────────────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────┐
│  Layer 4: Handoff (not compaction)                         │
│  真撑不下时，换 thread，不原地压                             │
│                                                            │
│  LLM 自己调 handoff tool                                    │
│  创建新 thread + first-person recap + top-10 files         │
│  旧 thread 仍保留，可追溯                                    │
│  用户感知：像"对话太长了，开个新对话"，自然                  │
└──────────────────────────────────────────────────────────┘
```

---

## Layer 1：输入端截断

### Read 的多重限制

```js
{
  maxLines:       500,         // pG
  maxLineBytes:   4096,        // XLT
  maxFileSizeBytes: 5_138_022, // eD (~5 MB)
  truncationStrategy: "ellipsis"
}
```

单次 Read 最多约 80 KB 文本，等价 ~20K tokens。对大部分文件够用。

**关键设计**：用 `read_range: [start, end]` 参数让 LLM 能分段读，**不是一次读全**。

### Tool Result 按大小截断

从 `tool_result` 生成代码里看到：

```
... [Tool result truncated - showing first ${Math.round(FL/1024)}KB of 
${e}KB total. The tool result was too long and has been shortened. 
Consider using more specific queries or parameters to get focused results.]
```

**设计亮点**：截断消息明确引导 LLM "下次用更精确的参数"。模型看到这条 notice，下次不会傻读一遍大文件。

### 目录列表截断

```
[... directory listing truncated, ${_} more ${h9(_,"entry","entries")} not shown ...]
```

### Bash 输出截断

```js
{
  maxBufferBytes: ...,
  // 超了会保留末尾，前缀被截
}
// 末尾追加:
// [Warning: Output was truncated due to buffer overflow]
```

### AGENTS.md 硬预算

```js
const BUDGET = 32768;  // cpR
let totalBytes = 0;
for (let block of agentMdBlocks) {
  let size = new TextEncoder().encode(block.text).length;
  if (totalBytes + size > BUDGET) {
    logger.warn("AGENTS.md guidance budget exceeded, truncating", {
      totalBytes, budgetBytes: BUDGET,
      includedBlocks: included.length,
      droppedBlocks: agentMdBlocks.length - included.length
    });
    break;
  }
  included.push(block);
  totalBytes += size;
}
```

保护"用户 AGENTS.md 写了 2 MB"的翻车场景。

---

## Layer 2：不重复 / 不默认加载

### Skills 懒加载

prompt 只列 skill `name` + `description`：

```xml
<available_skills>
  <skill>
    <name>web-browser</name>
    <description>Use for interacting with web pages, taking screenshots...</description>
    <location>/Users/x/.agents/skills/web-browser/SKILL.md</location>
  </skill>
  <skill>
    <name>code-tour</name>
    <description>Generate guided walkthroughs of diffs using the code_tour tool.</description>
    <location>builtin:///skills/code-tour/SKILL.md</location>
  </skill>
</available_skills>
```

Skill 完整内容只有调 `load_skill(name)` 才加载。

**影响**：30 个 skills 在 prompt 里只占几百 tokens，不管每个 skill 内容有多长。

### MCP 工具过滤

Amp 官方 skill 创建指南里的原文：

> MCP servers often expose many tools (chrome-devtools has 26 tools = 17,700 tokens).
> Always use `includeTools` to expose only what the skill needs.
>
> This reduces token cost by **90%+** and keeps the skill focused.

每个 skill 的 `mcp.json` 要明确写出需要哪些 tool：

```json
{
  "chrome-devtools": {
    "command": "npx",
    "args": ["-y", "chrome-devtools-mcp@latest"],
    "includeTools": ["navigate_page", "take_screenshot", "click"]
  }
}
```

未在 `includeTools` 里的工具直接不注册到 agent。

### Prompt Caching（SHA-256 分片对齐）

详见 [`../prompts/assembly-pipeline.md`](../prompts/assembly-pipeline.md)。要点：

- System prompt 是 block 数组
- 每个 block 独立 SHA
- 逐 block 比对，告诉调用者哪变了
- 帮助维持 Anthropic 的 `cache_read_input_tokens` 高比率

### fileChangeTracker 不进 message

文件 edit 历史存在独立的 `FileChangeTracker` 里：

```js
Map<toolUseID, Map<fileURI, {oldContent, newContent, timestamp, reverted}>>
```

Tool call 的参数（`old_str` / `new_str`）和 result（diff）还是在 message 里，但 **tracker 让 UI / CLI / undo 不用重读 message 历史**，也避免把 full diff 反复塞 context。

---

## Layer 3：子 agent 吃掉成本

### 核心假设

> 大部分的探索（语义搜索、代码理解、外部仓库查询）都产生**巨量 tool call + 多轮 LLM 迭代**。
> 如果这些全在主 context 里跑，300K 很快爆。

### 解决：4 种子 agent

```
主 agent 的一次 "请你调研一下 X" 在主 context 里只占：
  - 一次 tool_use {name: "Task", input: {prompt: "调研 X"}}
  - 一次 tool_result {content: "<子 agent 总结的 2000 tokens>"}

但在子 context 里实际跑了：
  - N 次 LLM turn
  - M 次 Bash / Read / Grep
  - 最后 Gemini 3 Flash 把整个 work log 压成 summary
```

**关键工具**：

| 工具 | 子 agent 类型 |
|---|---|
| `Task` | 通用执行者 |
| `Oracle` | 高 reasoning 顾问 |
| `codebase_search_agent` (finder) | 语义代码搜索 |
| `Librarian` (read_github) | 跨仓库理解 |

### Gemini Flash 压缩

Task / finder 完成时，**自动**把整个 work log 喂给 `gemini-3-flash-preview`，用结构化 schema 输出 summary：

```js
j$(xU, [
  { role: "user", parts: [{ text: workLog }] },
  { role: "user", parts: [{ text: summaryPrompt }] }  // 见 compaction-recap.md
], ..., {
  responseMimeType: "application/json",
  responseJsonSchema: zpT,
  thinkingConfig: { thinkingLevel: "MINIMAL" }
});
```

**成本**：~1000 output tokens 级别，几乎可以忽略。

---

## Layer 4：Handoff > Compaction

Amp **没有自动触发的压缩**。当主 thread 真的撑不下时：

1. LLM 自主决定调 `handoff(goal)` 工具
2. 内部开新 thread
3. 父 thread 的关键信息以 **first-person recap + top 10 files** 形式注入新 thread

详见 [`handoff.md`](./handoff.md)。

---

## 对比：为什么不做自动压缩？

| | 自动 `/compact` 压缩 | Handoff 开新 thread |
|---|---|---|
| 上下文重置程度 | 部分（保留 summary） | 完全（只带 recap）|
| 用户感知 | "刚才的对话被我吞了" | "开了个新对话继续" |
| 可追溯性 | 丢了细节，想追要翻 UI | 旧 thread 还在，点一下就回去 |
| 触发者 | 通常是 harness 按阈值 | **LLM 自己决定**（更准确）|
| 成本 | 一次额外 LLM 调用（summary）| 一次额外 LLM 调用（recap）+ 新 thread 启动 |

Amp 选了 handoff 的原因（推断）：

1. **"压缩丢信息" 的用户反感度高**：用户不知道你压了什么。handoff 至少能点回去看原对话。
2. **LLM 自己判断比阈值准**：阈值可能早于或晚于真正的 context degradation。
3. **Handoff 顺便解决"change topic"场景**：不只是 context 满，换话题也适合 handoff。

---

## 小结：每一层的价值

| Layer | 节省 tokens（估）| 实现成本 |
|---|---|---|
| 1. 工具截断 | ~20%（单次调用避免爆）| 低 |
| 2. Skills/MCP 懒加载 | ~30%（初始 prompt 更瘦）| 中（需要 lazy 加载机制）|
| 3. 子 agent 压缩 | ~40%（探索成本外包）| 高（需要独立 runtime）|
| 4. Handoff | 100%（真爆时重置）| 中（需要 thread service）|

所有层一起用，才形成**"读几十个文件上下文还干净"**的体感。
