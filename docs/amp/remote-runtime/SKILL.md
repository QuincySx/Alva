---
name: amp-remote-runtime
description: Amp 的远程 / subprocess 运行形态 —— DTW 在 Cloudflare Workers 上跑的 remote executor，以及 --execute --stream-json 的 NDJSON subprocess 协议。想让 agent 被程序化调用或做云端 executor 时加载。
trigger_words:
  - DTW
  - remote executor
  - Cloudflare Workers
  - Durable Objects
  - stream-json
  - NDJSON
  - execute mode
  - headless agent
  - --apply
  - --headless
  - subprocess IPC
  - executor_guidance_snapshot
  - tool_lease
  - remote runtime
  - CI agent
  - WebSocket protocol
  - thread actor
---

# Amp Remote Runtime

多种"非本地 TUI"运行形态 + 对应协议。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./dtw.md` | Distributed Thread Worker (Cloudflare Workers + Durable Objects) | 想懂 Amp 怎么跑远程 agent |
| `./headless-executor.md` | `--headless` 模式：CLI 反向接云端 thread actor，本地跑 tools | 想让本地 terminal 成为 controller 的 worker |
| `./websocket-protocol.md` | CLI ↔ thread-actor 完整 WebSocket 协议（消息类型 + 分片 snapshot + fs RPC）| 想设计/实现 Alva 的 RemoteEngineRuntime |
| `./stream-json.md` | `--execute --stream-json` subprocess NDJSON IPC 协议 | 想让 agent 被 CI 或其他 agent 调用 |

## 三种场景区分

| 维度 | DTW | Headless Executor | Stream-JSON |
|---|---|---|---|
| 用途 | 云端长运行 | 本地 CLI 作 worker，web 指挥 | 单次短运行（CI pipeline 一步）|
| 触发 | ampcode.com web UI + remote thread | `amp --headless` (employee-only + `AMP_EXECUTOR=1`) | `amp --execute --stream-json` |
| 协议 | WebSocket + DO state | WebSocket thread-actor (Rivet) | NDJSON stdin/stdout |
| Amp 实例数 | 2 个（local observer + cloud executor）| 2 个（cloud controller + local executor）| 1 个（local 进程）|
| 持久性 | 持久化，可断线重连 | Reconnect buffer + resumeBootstrap 差分 | 一次性，进程结束即终 |
| 权限 | user 认证 + quota | feature-gated, Amp 员工 | `--dangerously-allow-all` 逃生口 |

## Headless Executor 速查

- **`--headless` / `--headless=<threadId>`** —— `kVT` class 启动点
- **`AMP_EXECUTOR=1`** 环境变量 + API key + employee-only gate
- **`kVT.clients: Map`** —— 理论上多 client，实际单连路径
- **Bootstrap 8 阶段**：settings → handshake → environment → guidance (chunked) → skills (chunked) → git → tools → complete
- **`resumeBootstrap`**：reconnect fast-path，server 说"你来过了"，跳 env/guidance/skill/git
- **`pendingTerminalResults`**：tool result 发不出去先 buffer，重连后 flush —— 保一致性关键点
- **`bindClientThread`**：切 thread 重建 `fsTracker`（rollback/edit 按 thread 隔离）

## DTW 速查

- **"DTW" 推断** = Distributed/Durable Thread Worker
- **跑在** Cloudflare Workers + Durable Objects
- **CLI flag**：`--apply <threadID>`, `--checkout`, `--worker-url`
- **`executor_guidance_snapshot`** = 本地把 AGENTS.md 分片推给 DTW
- **ThreadPool 状态**：`isDTWMode()` / `getTransportConnectionState()` / `getTransportConnectionRole()`
- **两个 Amp 实例**：本地 CLI 和 DTW worker 都是 Amp，通过 thread service 同步 messages

## Stream-JSON 协议速查

**Input (stdin, NDJSON)**：
```jsonl
{"content":"fix the auth bug"}
{"content":"also add tests","agentMode":"deep"}
```

**Output (stdout, NDJSON)**：
```jsonl
{"result":"Fixed auth.ts:42...","usage":{"input_tokens":12000,"output_tokens":450,"cache_creation_input_tokens":8000,"cache_read_input_tokens":120000}}
```

**严格模式**：坏 JSON 立即退出 code 1。
**自动检测**：stdout 非 TTY + 没 `--stream-json` → 自动 execute mode（human-readable 输出）。
**工具白名单**：execute 模式下未 allowlist 的 tool 直接失败（没法弹 HITL）。

## 用例

CI 里用：
```yaml
- run: |
    echo '{"content":"check failing test and fix it"}' \
      | amp --execute --stream-json --dangerously-allow-all \
      > result.jsonl
    cat result.jsonl | jq '.result'
```

上游 agent 里用：
```ts
let output = await $`amp --execute --stream-json --dangerously-allow-all < ${input}`;
let parsed = output.stdout.trim().split("\n").map(JSON.parse);
```

## 对 Alva 的启发

Alva 是 local-first，不需要 DTW 整套。但可以抄：

- **`EngineRuntime` trait 预留 remote 扩展点**
- **`--apply <session-id>`** one-shot 模式（CI 友好）
- **`executor_guidance_snapshot` 分片上传 + `snapshotId` + `isLast`**（最值得抄的点）
- **`pendingTerminalResults` 缓冲**：断线时 buffer tool result，重连 flush —— 避免"本地干完活服务器不知道"
- **NDJSON subprocess 协议比 JSON-RPC 简单**（对 CI 调用更合适）
- **Executor-worker 反转**：`RemoteEngineRuntime` 不一定只"发请求到远端"，也能是"把本地进程接成远端的 worker"
