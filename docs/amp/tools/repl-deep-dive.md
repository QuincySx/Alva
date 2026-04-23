# REPL 工具 —— 嵌套 LLM 循环控制流

> 这是 Amp 唯一一个**在工具执行内部跑自己的 LLM 循环**的 tool。
> catalog 里只有 4 行的条目，实际是个小型 agent 系统。

---

## 概念

`repl` 是"一个工具里塞一个 mini agent"：

1. 父 LLM（主 thread）调 `repl({binary, args, objective, ...})`
2. 工具内部 spawn 子进程（`node` / `python -i` / `psql` 等）
3. 工具内部**自己跑一圈 inference 循环**（不用父 LLM）
4. 每轮：子进程 stdout → 子 LLM → 子 LLM text response → 子进程 stdin
5. 子 LLM 调 `stop` 工具 → 循环结束
6. 工具返回总结给父 LLM

父 LLM 视角：这是一个异步工具调用，只看到最终 summary；不参与交互。
子 LLM 视角：自己是个 REPL operator，只能打 REPL 命令，只有一个工具可用（`stop`）。

---

## 工具 spec

```json
{
  "name": "repl",
  "inputSchema": {
    "binary":         { "type": "string" },
    "args":           { "type": "array", "items": { "type": "string" } },
    "objective":      { "type": "string" },
    "replDescription":{ "type": "string" }
  }
}
```

**典型调用：**

```json
{ "binary": "python3", "args": ["-i", "-u"], 
  "objective": "Compute the 10th fibonacci number",
  "replDescription": "Python 3 REPL" }
```

父 LLM 的工具 description 明示：

```
This tool spawns a REPL process (like node, python, psql, mysql, redis-cli, 
etc.) and runs an autonomous agent loop that:
1. Sends commands to the REPL's stdin
2. Reads output from the REPL's stdout
3. Uses an LLM to decide what commands to send next based on the objective

WHEN TO USE THIS TOOL:
- When you need to interactively explore a database (psql, mysql, sqlite3, redis-cli)
- When you need to test code snippets in a REPL (node, python3, irb, ghci)
- When you need to interact with any command-line tool that has a REPL interface
- When the task requires multiple back-and-forth interactions with a subprocess

WHEN NOT TO USE THIS TOOL:
- For simple one-off commands (use Bash instead)
- When you don't need interactive exploration
- When the command exits immediately after output

- The agent's text responses are sent DIRECTLY to the REPL's stdin
- The agent should only output valid REPL commands, no explanations
- The agent has a "stop" tool it can call when the objective is complete
- The subprocess HAS NO PTY - some programs like python3 or bash need an 
  extra flag in that case, often -i.
```

---

## 子 LLM 的 system prompt（原文）

反编译 `Z6R(replDescription, objective, ...)` 函数：

```
You are a REPL operator. Your text responses are sent DIRECTLY to a {replDescription}.

1. Your response text goes VERBATIM to the REPL - no exceptions
2. ONLY output valid REPL commands/expressions
3. NO explanations, NO commentary, NO markdown, NO prose
4. If you want to explain something, use the REPL's comment syntax
5. One command per response (unless the REPL supports multi-line input)

WRONG (do NOT do this):
Let me check the date:
I'll define a function to help:
function add(a, b) { return a + b; }

// Define helper function
function add(a, b) { return a + b; }

**Your Objective:** {objective}

**Important:** The REPL runs as a subprocess without a TTY. Some programs 
require flags to enable interactive mode:
- bash: use `bash -i` for interactive mode
- python: use `python -i` or `python -u` for unbuffered output
- node: works interactively by default

- User messages prefixed with [REPL output:] contain REPL output
- Your entire text response is piped to the REPL stdin
- Call the `stop` tool when done (with a summary message)

Remember: You are typing INTO the REPL. Act like it.
```

注意几个有意思的 prompt 设计点：

1. **先讲规则再举反例**（"WRONG"），比"什么都不要写"更明确
2. **明示 prose 会被写进 REPL**（因为子 LLM 本能想解释，这就是 bug）
3. **tty 提示放在 prompt 里**（而不是 tool description 里）—— 因为子 LLM 看不到父 LLM 的工具 description，只看自己的 system prompt
4. **objective 直接模板拼接**，不分 block，因为子 LLM 的任务极窄

---

## 子 LLM 可用工具：只有 `stop`

子 LLM 的 tool set 只有一个工具（反编译常量 `xqT` = stop tool spec）：

```json
{
  "name": "stop",
  "description": "Call this tool when the objective is complete or impossible.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "message": { "type": "string", "description": "Summary of what was accomplished" }
    }
  }
}
```

反编译的识别逻辑：

```js
let pT = Y.find((kT) => kT.name === "stop");
if (pT) {
  D = pT.input.message ?? "Session ended";
  L = true;  // 循环终止 flag
  ...
  break;
}
// 任何其他 tool name 都被当成 unknown tool
let mT = Y.map((kT) => ({
  type: "tool_result",
  tool_use_id: kT.id,
  content: `Unknown tool: ${kT.name}`
}));
```

**设计要点：** 子 LLM 理论上不能调别的工具。如果它幻觉调了别的（`Read` / `Bash`），harness 直接 echo 回 `"Unknown tool: xxx"`，子 LLM 学到"只能 stop 或者打 REPL 命令"。

---

## 循环控制流

反编译后伪码骨架：

```
# Setup
d = spawn(binary, binaryArgs, { stdio: "pipe" })   # 无 PTY
messages = []
startup = await readStdoutFor(5000 ms)
messages.push({ role:"user", content: startup 
    ? `[REPL started.]\n${startup}` 
    : "[REPL started. Awaiting your input.]" })
contextBudget = floor(model.maxTokens * 0.9)

# 主循环
for q in 0..50:
    # 早停门闸（见下节）
    if aborted / processExited / outputOverflow / tokensOver: break
    
    # LLM 推理（60s 单轮超时）
    response = await infer(messages, tools=[stop], system=Z6R(...), 
                           signal=AbortSignal.any([parent, timeout(60_000)]))
    
    # 路径 A：LLM 调 stop → 终止
    if toolUses.has("stop"):
        terminationReason = stop.input.message
        break
    # 路径 B：LLM 幻觉其他工具 → 硬 echo "Unknown tool: xxx"
    if toolUses.length > 0:
        messages.push({ role:"user", content: toolUses.map(tu => 
            { tool_use_id: tu.id, content: `Unknown tool: ${tu.name}` }) })
        continue
    # 路径 C：LLM 输出纯文本 → 逐行写进 stdin
    text = response.content.filter(type=="text").join("")
    for line in text.split("\n"): 
        if !stdin.write(line+"\n"): break_with_error
    
    # 等 stdout
    output = await readStdoutFor(2000 ms)
    if output: messages.push({role:"user", content: `[REPL output:]\n${output}`})
    elif isFirstWrite and !processExited:
        # 诊断：大概率 TTY 问题
        return error(list bash/python/irb flags)
    else:
        messages.push({role:"user", content: "[No output received. ...]"})

# Collect result
finally: d.kill()
result = [terminationReason, `Process exit code: ${exitCode}`?, lastOutput, 
         "[Warning: Output was truncated]"?].filter(Boolean).join("\n")
emit({ status: "done", result, "~debug": { inferences, exitCode }})
```

三条路径全在 `if toolUses.length > 0` 的分支里分开（stop / 幻觉 / 无 toolUse），Amp 反编译代码在这一处特别紧凑。

反编译原文片段（loop body，截一段看下识别逻辑）：

```js
let Y = Z.message.content.filter((pT) => pT.type === "tool_use");
if (Y.length > 0) {
  let pT = Y.find((kT) => kT.name === "stop");
  if (pT) {
    D = pT.input.message ?? "Session ended", L = true;
    messages.push(Z.message, { role: "user", content: [toolResult(pT.id, "done")] });
    break;
  }
  // 其他 tool → "Unknown tool: xxx"
  continue;
}
```

---

## 关键常量表

| 常量 | 值 | 含义 |
|---|---|---|
| `tNR` | 50 | 最多 50 次推理循环 |
| `rNR` | 2000 ms | 每轮等 stdout 的时间窗 |
| `QmT` | 100 | （未确认用途，可能是 stdout chunk 大小）|
| `eNR` | 1500 | （未确认，可能是首次等待内部）|
| `ZmT` | 10_485_760 = 10 MiB | 累计 stdout 上限 |
| `JmT` | 60_000 ms | **单轮 LLM 推理**超时 |
| `TpT` | 5000 ms | spawn 后等首次 startup output |
| `hNR` | 0.9 | Context 预算安全系数（使用到 90% 就停）|

50 次 × 60s/次 = 最坏 50 分钟。实际大多数会话 5-10 次就 stop。

---

## 早停条件（优先级从高到低）

1. 父 signal 被 abort（用户 Ctrl-C）
2. 子进程崩溃（`processExited` 且 exitCode ≠ 0）
3. 子进程报错事件（error event）
4. stdout 累计超 10 MiB
5. Token 用量 >= 90% context window
6. 单轮推理超 60s
7. 子 LLM 调 `stop` 工具
8. stdin write 失败（管道破裂）
9. 首次 input 无响应（诊断为 TTY 问题）
10. 循环达到 50 次上限

每个都走独立 break 路径，各自有不同的错误/完成消息。这是个**防御深度很好**的设计。

---

## Transcript / 返回给父

`progress.transcript` 是线性的 `[{type:"input"|"output", content}, ...]`，以 progress events 流给父 LLM（和 `stream-json` 一致）。`status: "done"` 时带最后 output 作为 result string。

还有个 `~debug` 字段（下划线前缀表示 harness log 专用，不给父 LLM 看）：

```js
"~debug": {
  threadID: subThreadID,
  inferences: [{ inferenceTimeMs, usage: { model, inputTokens, outputTokens, 
                                           cacheWriteTokens, cacheReadTokens }}],
  exitCode
}
```

`amp context` 命令用这个统计"REPL 子 thread 花了多少 token"，和主 thread 分开算。

---

## 和其他子 agent 类工具的对比

| 工具 | 子 LLM 模型 | 子 LLM 工具集 | 交互数 | 和外部交互 |
|---|---|---|---|---|
| `Task` | 主 model | 完整工具集（subset）| 多轮 LLM-driven | 读写文件 / bash |
| `Oracle` | 高级 reasoning | **只读工具** | 一轮 zero-shot | 只读 |
| `analyze_file` | Gemini 3 Flash | ∅ | 一轮 | 读单文件 |
| **`repl`** | **主 model** | **只有 `stop`** | 最多 50 轮 | **子进程 stdin/stdout** |

**repl 特殊在**：子 LLM 的"动作"是纯文本（写进 stdin），不是 tool call。这是唯一一个**反向** tool contract —— 不调工具才是正常行为。

---

## 设计选择备注

**用主 model 不用 Flash**：REPL 需要"解析 traceback + 写正确语法"，便宜模型会把 ambiguous stdout 搞错。Amp 靠 3 层硬上限（50 轮 / 90% ctx / 60s）兜底烧钱风险。

**不用 PTY**：可移植（Windows 无 PTY）、stdin/stdout 纯 pipe 好控 buffer、无需解析 ANSI escape。代价：用户得记 `-i` / `-u`，子 LLM system prompt 里写建议算软补救。

---

## 对 Alva 的启发

### 最值得抄的 1 个点：工具内嵌 LLM 循环的**封装契约**

Amp 的 `repl` 展示了"工具不只是函数调用，可以是子 agent"的模式。对应到 Alva：

```rust
// SubAgentExtension 现在 spawn 的是"完整 agent"
// 可以加一个"受限 agent" 变种：工具内部的 LLM loop
pub trait InnerLlmLoop {
    fn system_prompt(&self, args: &Args) -> String;
    fn tools(&self) -> Vec<Tool>;              // 通常是 1 个 stop tool
    fn process_llm_text(&self, text: &str) -> LoopAction;
    fn process_external_event(&self, evt: ExternalEvent) -> LoopState;
    fn max_iterations(&self) -> usize { 50 }
    fn iteration_timeout(&self) -> Duration { 60.seconds() }
    fn context_safety(&self) -> f32 { 0.9 }
}
```

这样可以做出一族工具：`repl` / `negotiate_api` / `reverse_engineer_cli` / `interactive_debug`。都是"我控制一个外部进程 + 内嵌 LLM 决策"。

### 和 Blackboard / SubAgentExtension 的关系

- **`SubAgentExtension`** 现有的设计偏向"开一个平级 agent 做独立任务"，消息透明回流。
- **REPL 模式**不一样：子 LLM 是**完全封装**的，父 agent 不看 inferences 细节。只看 transcript 和 final result。
- 两者应**并存**，不是二选一。新建一个 `InnerLoopExtension`（或复用 `SubAgentExtension` 加 `opaque: true` 标志）。

### 具体抄点

1. **10 道早停门闸**：复制清单，每个都显式 break
2. **stdout OUTPUT_READ_MS = 2s**：不要无限等，超时也得继续让 LLM 决策
3. **"Unknown tool" echo**：幻觉时不 crash，硬 echo 回错误让 LLM 自纠
4. **首次无响应 = TTY 错误**：诊断消息直接列 `python -i` / `bash -i` / `irb --noautocomplete`，不让用户猜
5. **`~debug` 字段**：调用统计和父 thread 分开，Alva 的 telemetry 可以学
6. **系统 prompt 给反例（WRONG:）**：对话式工具的 prompt 硬核原则

### 不要抄的

- **用主 model 跑子循环**：Alva 可以允许但默认应该是便宜 model（Alva 对成本敏感度更高）
- **Anthropic-specific 的 `wfR` 调用约定**：Alva 已有 `provider` 抽象，不需要耦合
- **CLI binary 硬编码假设**：Alva 本来就是 local-first + sandbox-friendly，REPL tool 应该走 sandbox 子进程而不是直接 `spawn`
