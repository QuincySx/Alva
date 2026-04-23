# Stream-JSON Protocol

> `amp --execute --stream-json` subprocess IPC 协议。让 Amp 可以被当作"函数"调用。

---

## 触发 Flags

三个正交的 flag：

```
--execute                 # 单次执行后退出，不进 REPL
--execute-mode            # --execute 的别名
--stream-json             # 启用 JSON line 协议（input + output）
```

自动模式推断：

```js
executeMode = !!T.execute || (!process.stdout.isTTY && !T.streamJson)
// stdout 不是 TTY 且没开 stream-json → 默认进入 execute 模式
```

这意味着：

```bash
amp "fix the bug"              # TTY → interactive TUI
amp "fix the bug" | cat        # pipe → 自动 execute mode (人类可读输出)
amp --stream-json < input.jsonl # → stream-json mode（机器可读 NDJSON）
```

---

## Input 协议（stdin）

```ts
interface StreamJsonInput {
  content: string;                // user message text
  agentMode?: "smart" | "deep" | "rush" | ...;
  // ...
}
```

每行一条，JSON-serialized：

```jsonl
{"content":"fix the auth bug"}
{"content":"also add tests","agentMode":"deep"}
{"content":"/compact"}
```

### 读取实现

```js
async function* Oo0(input) {
  let reader = readline.createInterface({ 
    input, 
    crlfDelay: Infinity 
  });
  let lineNo = 0;
  
  for await (let line of reader) {
    lineNo++;
    if (line.trim() === "") continue;       // 跳空行（允许 keepalive）
    
    let parsed;
    try {
      parsed = JSON.parse(line);
    } catch (e) {
      throw new ValidationError(
        `Invalid JSON on stdin line ${lineNo}: ${e.message}`, 
        1  // exit code
      );
    }
    yield parsed;
  }
}
```

**严格模式**：任何坏 JSON **立即** 报错退出。不容错。这让上游 agent 发现协议问题时快速失败。

---

## Output 协议（stdout）

每条 assistant 回复一行：

```jsonl
{"result":"Fixed auth.ts:42 by guarding undefined user. Tests pass 148/148.","usage":{"input_tokens":12000,"output_tokens":450,"cache_creation_input_tokens":8000,"cache_read_input_tokens":120000}}
```

### Schema

```ts
interface StreamJsonOutput {
  result: string;                 // assistant 最终 message text
  usage: {
    input_tokens: number;
    output_tokens: number;
    cache_creation_input_tokens: number;
    cache_read_input_tokens: number;
  };
}
```

### 写出实现

```js
l = n.thread$.subscribe(async (j) => {
  if (d = j, NdT(j.messages) > m) {
    let E = re(j, "assistant");
    if (E && Z0T(E)) {
      if (E.content.some(b => b.type === "tool_use")) return;   // 跳过中间 tool_use
      let text = ya(E.content).trim();
      
      if (streamJson) {
        let usage = E.usage;
        let output = {
          result: text,
          usage: {
            input_tokens: usage?.inputTokens || 0,
            output_tokens: usage?.outputTokens || 0,
            cache_creation_input_tokens: usage?.cacheCreationInputTokens || 0,
            cache_read_input_tokens: usage?.cacheReadInputTokens || 0
          }
        };
        process.stdout.write(JSON.stringify(output) + "\n");
      } else if (text) {
        process.stdout.write(text + "\n");
      }
      await cleanup();
    }
  }
});
```

**关键点**：
- **只输出最终 text**（tool_use 中间过程不输出到 stdout）
- **usage 每条都带**（方便上游监控 cost）
- **一行一个 JSON**（NDJSON）

---

## 工具白名单

execute 模式下**所有 tool 必须预先 allowlist**，否则立即失败：

```
Error: The Bash tool tried to run a command that isn't allowlisted. 
Rerun with --dangerously-allow-all to bypass, or add to the command 
allowlist in permissions (https://ampcode.com/manual#permissions).
```

原因：execute 模式 = headless，没有 UI 弹窗问用户 approve。未授权工具直接失败比挂在 "waiting for approval" 好。

---

## Error Handling

各类错误都转换成清晰的 stderr message：

```js
function UdT(T) {
  if (T instanceof Error) {
    if (G3T(T)) return "Unauthorized. Check your access token.";
    if (B$(T))  return "Context window limit reached.";
    if (MU(T))  return "Model provider overloaded. Try again in a few seconds.";
    if (LU(T))  return "Model stream timed out. Try again in a few seconds.";
    if (F3T(T)) return "Insufficient credit balance.";
    return T.message;
  }
  return String(T);
}
```

**退出码约定**（推断）：
- `0` —— 成功
- `1` —— 一般错误（包括 JSON parse 失败）
- `130` —— Ctrl+C（SIGINT）
- 其他 —— 特定错误类型

---

## 使用场景

### 1. CI pipeline

```yaml
# .github/workflows/auto-fix.yml
- run: |
    echo '{"content":"check the failing test and fix it"}' \
      | amp --execute --stream-json --dangerously-allow-all \
      > result.json
    cat result.json | jq '.result'
```

### 2. 上游 agent 调下游 agent

```ts
let output = await $`amp --execute --stream-json --dangerously-allow-all < ${input}`;
let parsed = output.stdout.trim().split("\n").map(JSON.parse);
```

### 3. Batch 处理

```bash
cat queries.jsonl | amp --execute --stream-json > answers.jsonl
```

---

## 和 `--execute` 不带 `--stream-json` 的区别

```bash
amp --execute "fix the bug"
```

输出：human-readable text，不是 JSON：

```
Fixed auth.ts:42 by guarding undefined user.
Tests pass 148/148.
```

这适合人类在终端里看。`--stream-json` 适合程序化调用。

---

## 对 Alva 的启发

你们 `alva-app-extension-loader` 是 JSON-RPC 2.0。对照 Amp 这套更简单的 NDJSON：

### 主要差异

| 维度 | JSON-RPC 2.0 | NDJSON stream |
|---|---|---|
| 消息格式 | `{jsonrpc, id, method, params}` | 直接是 payload |
| 配对 | 需要 id 关联 request/response | 无，单向流 |
| 双向 | 双向 RPC | 单向 in / 单向 out |
| 错误 | `error` 字段 | exit code + stderr |
| 复杂度 | 高 | 低 |

### 建议

对于 **agent 被上游调用** 场景（CI / 管道化），NDJSON 更合适：

```rust
// alva-app-cli 加 --stream-json mode
#[derive(Parser)]
struct CliArgs {
    #[arg(long, help = "Enable NDJSON streaming mode (machine-readable)")]
    stream_json: bool,
    
    #[arg(long, help = "Run once and exit (headless-friendly)")]
    execute: bool,
    
    #[arg(long, help = "Bypass all permission prompts (CI only)")]
    dangerously_allow_all: bool,
}

// 自动推断：
let execute_mode = args.execute || (!atty::is(atty::Stream::Stdout) && !args.stream_json);
```

### 兼容性

这不跟 AEP 冲突：

- **AEP (JSON-RPC 2.0)** = plugin 和 agent 之间的双向 RPC
- **Stream-JSON** = agent 和调用者之间的单向流

两套并存。
