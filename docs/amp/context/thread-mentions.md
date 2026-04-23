# Thread Mentions —— 用 `@T-xxx` 引用另一个线程

> Amp 废弃 `fork` 时给出的两条替代路径之一（另一条是 `handoff`）。
> `Fork has been replaced by handoff and thread mentions.` —— 反编译里的一行明示。

---

## 是什么

用户在 chat input 里输入另一个 thread 的 ID / URL，Amp 自动调用 `read_thread` 工具（LLM 主动调，基于工具描述的触发规则），把那个 thread 的相关片段抽出来放进当前 context。

一句话：**跨 thread 的 retrieval，粒度是"整个对话"，不是"文件行"。**

---

## 语法（用户输入侧）

反编译里 `read_thread` 的 description 明示 3 种写法（用户 message 里命中任一即触发）：

| 形式 | 例子 |
|---|---|
| 纯 thread ID | `T-a38f981d-52da-47b1-818c-fbaa9ab56e0c` |
| `@` 前缀 + ID | `@T-a38f981d-52da-47b1-818c-fbaa9ab56e0c` |
| ampcode.com URL | `https://ampcode.com/threads/T-xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` |
|   | `https://ampcode.com/v2/workspace/project/T-xxxx...` |
|   | `https://ampcode.com/v2/amp/amp/T-xxxx...`（内部 workspace 路径）|

Thread ID 是 `T-{uuid}`，uuid 就是标准 UUID v4（有时用 v7：`T-019d01b5-f70d-73ea-9445-...`）。

---

## 客户端辅助：`@@` 唤起选择器

反编译里有一个 CLI slash command（不在工具里，在 UI 命令面板里）：

```
mention-thread: "Mention another thread"
```

行为（伪码 from 反编译 ChatInput reducer）：

```
user 输入 "@@" → 打开 ThreadPicker（列出非当前 thread）
选中某个 thread → 把缓冲区里 "@@" 替换成 "@<threadID> "
```

`@@` 是**触发字符**，不是最终语法。被选中后替换成单个 `@T-xxx`，对 LLM 来说还是 `@T-xxx`。

同一个 `@` 前缀还接另外两个 completion kind（反编译 `buildOptions` 里明示）：

| kind | 触发 | 行为 |
|---|---|---|
| `file` | `@path` | 模糊匹配 workspace 文件，变成 `@relative/path.ts` |
| `commit` | `@:abc123` 或 `@:HEAD~1` | 补全 git commit hash |
| `thread` | `@@` + 选择 | 补全 thread ID → `@T-xxx` |

三者都进用户 message 的原始文本，LLM 自己看描述决定用哪个工具消费它。

---

## LLM 侧：`read_thread` 工具

用户 message 里出现 thread ID 后，LLM 在看到 `read_thread` 工具描述后**主动调**。调用 schema：

```json
{
  "name": "read_thread",
  "inputSchema": {
    "threadID": "T-{uuid} or ampcode.com/.../T-{uuid}",
    "goal":     "A clear description of what information you need"
  }
}
```

两个参数都 required。LLM 被训练在工具 description 里填 `goal`，不是简单"读全部"：

反编译给出 5 个 in-prompt 示例（训练 LLM 填 goal 的方式）：

| 用户说 | LLM 应该填 goal = ... |
|---|---|
| "Implement the plan we devised in `T-3f1b...`" | "Extract the implementation plan, design decisions, architecture approach, and any code patterns or examples discussed" |
| "Do what we did in `T-f916...`, but for the Oracle tool" | "Extract the implementation approach, code patterns, techniques used, ..." |
| "Take the SQL queries from `T-95e7...` and turn it into a reusable script" | "Extract all SQL queries, their purpose, parameters, ..." |
| "Apply the same fix from `T-019d...` to this issue" | "Extract the bug description, root cause, the fix or solution, ..." |
| "Apply the same fix from `@T-95e7...` to this issue" | "Extract the bug description, root cause, the fix/solution, ..." |

---

## 工具内部：用 subagent 压缩

`read_thread` 不是"把整个 thread 灌进 context"。内部流程（反编译 `A1R` 函数）：

```
1. Resolve threadID
   - 本地 threadService.getThread(id)（如果 synced 过）
   - fallback: 从服务端 fetch（synced thread）
2. Render thread 为 markdown
   - 内部常量 "BEGIN THREAD MARKDOWN" marker
3. 起一个 subagent（不是主模型），input:
     system: "You are helping me extract relevant information from the 
              mentioned thread based on a goal."
     user:   goal + thread-as-markdown
4. 输出 JSON:
     { relevantContent: "<extracted markdown>" }
5. 返回给主 LLM: "Here is the mentioned thread content:\n<relevantContent>"
```

完整 extraction prompt（反编译原文）：

```
You are helping me extract relevant information from the mentioned thread 
based on a goal.
I am talking to another user. They mentioned a thread (a conversation) in 
their message last message. I turned the thread into Markdown and provided 
it to you, along with a goal of what I want you to extract.

1. Analyze the mentioned thread's content
2. Identify information that is relevant to the goal
3. Extract and preserve those relevant parts with full fidelity
4. Omit clearly irrelevant content to keep the context concise

**Preserve Fidelity**: When content IS relevant, include it completely 
with all important details, code snippets, explanations, and context.
**Be Selective**: When content is clearly NOT relevant to the user's query, 
omit it entirely.
**Maintain Structure**: Keep the extracted content well-organized and 
coherent. If multiple parts are relevant, preserve their logical flow.
**Technical Precision**: Preserve exact technical details like file paths, 
function names, error messages, and code snippets that are relevant.

Format your response as JSON with:
- relevantContent: The extracted relevant information (as markdown text)
```

这和 Task/Oracle subagent 的套路一致：**用便宜模型把大 context 压成主模型吃得下的片段**（见 `../context/strategy.md` 第 3 层策略）。

---

## 和 Handoff / Fork 的对比

| 维度 | `fork`（已弃）| `handoff` | Thread mention (`read_thread`) |
|---|---|---|---|
| 发起者 | 用户手动 CLI | LLM 主动调工具 | **用户**在 message 里写 `@T-xxx`，LLM **发现后主动调 read_thread** |
| 新线程？ | 是（复制 history）| 是（空线程 + recap）| **否** |
| 老 context 去向 | 完整拷到新 thread | 压成 first-person recap | 压成 goal-relevant 片段，**注入当前 thread** |
| 模型费用 | 0（纯复制）| 1 次 extract pass | 1 次 extract pass（subagent）|
| 适合场景 | —（已废）| 换话题 / context 满 | 跨 thread **引用** / 借鉴 / 复用方案 |
| 权限 | 用户能不能看到 thread | 不涉及跨 thread | **需要能 access 目标 thread**（本地或 synced）|

三者**正交**：handoff 是"开新对话"，thread mention 是"把另一段对话的精华拖进来"，fork 原本是两者都想兼顾，结果两头不讨好所以废掉。

---

## Thread 关系元数据

反编译里每个 thread 有 `relationships[]` 字段，每项 `{type, threadID, role}`：

```js
a = h.relationships.find(i => 
  i.role === "child" &&
  R.has(i.threadID) &&                // parent 存在
  (i.type === "fork" || i.type === "handoff"))
```

注意：**只有 `fork` 和 `handoff` 两种类型建立 parent-child 关系**。Thread mention **不进 relationships**，它是一次性的 context 注入，不构成线程树。

这是个重要的设计：mention 是轻量 retrieval，不污染 thread 拓扑。

---

## `parentThreadID` 和 read_thread 的复合用法

subagent（Task / Oracle）的 system prompt 里塞了这么一段（反编译原文）：

```
Parent thread: ${T.parentThreadID}
You can use the read_thread tool with this ID to read the full conversation 
that invoked you if you need more context.
```

意思：**子 agent 默认拿不到父 agent 的 message history**（Amp 的 Task 工具只给 summary 不给明细）。但留了 `read_thread` 当逃生口 —— 子 agent 如果觉得 summary 不够用，可以主动调 `read_thread(threadID=父ID, goal="...")` 回头查。

这个设计很漂亮：**父 → 子默认精简，子主动拉详细**。既省 token 又不堵死信息路径。

---

## 设计哲学

### 为什么不直接复制整段 thread？

反编译里明确：

> This tool fetches a thread (locally or from the server if synced), 
> renders it as markdown, and **uses AI to extract only the information 
> relevant to your specific goal**. This keeps context concise while 
> preserving important details.

原则：**retrieval，不是 copy**。跨 thread 引用的代价永远是 1 次 extract pass，不是整个 thread 的 token。

### 为什么用户在 message 里写 ID 就够了？

Amp 没做 "mention 是一等 UI 元素" 那套（比如像 Linear 的 `@user` 会变成 pill）。反编译里只是纯文本：

- 用户写 `T-xxx` 或 `@T-xxx` 或 URL → 就是纯 text
- LLM 看到纯 text + 看到 `read_thread` 工具 description 里的"当用户提到 thread ID 时调我" → 就调
- 工具内部把 thread 解析好塞回来

这是**工具化（LLM-driven），不是语法糖（UI-driven）**。改 prompt 就能加新触发方式，不用改 UI。

### `goal` 字段的必要性

如果没 goal 字段，LLM 会倾向"读整个 thread"，浪费 token。强制 `goal: required` 逼 LLM 在调用前先想清楚"我要什么"。这和 web_search / read_web_page 的 `objective` 参数一样，是 Amp 一贯的**聚焦原则**。

---

## 对 Alva 的启发

Alva 架构里：
- `Blackboard`（在 SpawnScope 下）已经能承载"跨 child agent 共享状态"
- `SubAgentExtension` 已经能 spawn 子 agent
- 缺一块：**跨 thread 引用的轻量 retrieval 工具**

### 最值得抄的 1 个点：`read_thread` 作为 first-class 工具

```rust
// 放进 Alva 的 built-in extension 清单
pub struct ThreadMentionExtension;

impl Extension for ThreadMentionExtension {
    fn register_tools(&self, ...) -> Vec<Tool> {
        vec![Tool {
            name: "read_thread",
            description: "<抄 Amp 原文，明示触发规则 + 5 个示例>",
            input_schema: json!({
                "threadID": { "type": "string", ... },
                "goal": { "type": "string", ... }
            }),
            handler: |args, ctx| async move {
                let thread = ctx.thread_store.get(&args.thread_id).await?;
                let markdown = render_thread_to_markdown(thread);
                // 用便宜 model 做 goal-driven extraction
                let summary = ctx.spawn_subagent(
                    extract_prompt(args.goal, markdown),
                    cheap_model!()  // Gemini Flash 等价
                ).await?;
                Ok(format!("Here is the mentioned thread content:\n{}", summary))
            }
        }]
    }
}
```

### 和 Alva 现有系统的契合点

1. **Thread store 已经有**（`ThreadExtension` 路径存 messages + metadata）—— 直接查就行
2. **SubAgentExtension 可以复用**：`spawn_subagent(extract_prompt, summary_model)` 就是 Amp 的同构操作
3. **Blackboard 无关**：thread mention 是**点对点** retrieval，不进 blackboard（blackboard 是共同工作区）
4. **relationships 字段**：新 thread 建立时只在 `handoff` / `fork` 路径写 parent，mention **不写**，保持一致

### 可以顺手抄的小设计

- `parentThreadID` 注入到 subagent system prompt（让子 agent 能回查父 thread）
- Thread ID 格式统一用 `T-{uuid}`（和 Alva 现在 session ID 风格一致，加个 `T-` 前缀易识别）
- UI 层 `@@` 触发 thread picker（Tauri GUI 可以抄）
- `goal` 字段 required，逼 LLM 聚焦

### 不要抄的

- Amp 的 `@@` 是 TUI input parser 的实现细节，Alva 不一定要做同样的 trigger key
- 服务端 fetch thread 的逻辑（Amp 是 DTW，Alva local-first 没这个问题）
- Thread visibility 层（private/workspace/public/group）—— 个人工具不需要这个复杂度
