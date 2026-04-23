# Headless Executor —— `kVT` Harness

> Amp 把自己 CLI 进程当作 "DTW executor" 反向接到云端 thread actor。**本地 terminal 成为 worker**，Web UI 或其他 client 下指令，本地跑 tools。

---

## 触发方式

```
$ amp --headless              # 或 --headless=<threadId>
```

二进制里的 gate（来自 `strings.txt:64071` 附近）：

```js
if (R.headless) {
  if (process.env.AMP_EXECUTOR !== "1" && (!m || !ci(m)))
    throw new A0("Headless executor mode is only available for Amp employees", 1);
  if (!rT) throw new A0("API key required for headless mode...");
  await my0({ ampURL, apiKey, workerUrl: process.env.AMP_WORKER_URL, workspaceRoot: process.cwd(),
              threadId, ownerUserId, threadVersion, agentMode, ... });
}
```

- **Feature-gated**：非 Amp 员工或没开 `AMP_EXECUTOR=1` 直接拒绝
- **必须 API key**：没登录就退
- **如果 `--headless=<threadId>` 带参**：attach 到现有 thread；否则创建新 thread

`my0()` 包装了一层 `thread-actors` 握手（`POST /api/thread-actors { usesThreadActors: true }` → `{ threadId, wsToken, ownerUserId, threadVersion }`），然后 `new kVT(options).start()`。

---

## `kVT` class 结构

```ts
class kVT {
  options;
  clients = new Map();                 // clientId -> ClientState
  clientCounter = 0;
  disposed = false;
  pluginsBootstrapped = false;
  pendingExitCode = null;
  toolSyncSubscription = null;
  resolveRunLoop = null;

  async start();                       // create 第一个 client + runLoop
  async createClient(threadId);        // 支持多 client 实例
  bindClientThread(client, threadId);  // client 切换 thread 时重绑 fsTracker
  async connectTransport(transport, id, phase);
  async recoverTransport(...)          // 发消息失败后的自愈
  async runLoop();                     // Ctrl+C / SIGTERM 守护
  async shutdown(exitCode, reason);
  async dispose();
}
```

---

## Client 状态 per-connection

每个 `client` 是一次 CLI ↔ thread-actor 会话：

```ts
ClientState = {
  id: "client-1",                   // internal clientCounter
  transport,                         // CVT 实例（WebSocket layer）
  threadId,                          // 服务器返回的权威 threadId
  executorRuntime: SeT,              // 上层：handshake + tool runner
  toolRunner: IVT,                   // tool lease 调度器
  fsTracker,                         // per-thread file change tracker
  recoveringTransport: false,
  hasLoggedPostBootstrapThreadURL: false,
}
```

**多 client 绑定**：`kVT.clients` 是 Map，可以有 N 个 client 同时跑（同一 CLI 进程多个 thread）。但 `start()` 默认只 `createClient(this.options.threadId)` 一个 —— 更多 client 是为 future-proofing，当前主路径单 client。

### `bindClientThread()` 细节

```js
bindClientThread(T, R) {
  let t = T.threadId !== R;         // thread 变了？
  T.threadId = R;
  if (T.fsTracker && !t) return;    // 没变就留用
  let r = this.options.fileSystem ?? fr,
      e = new TU(r);                // file storage
  T.fsTracker = ZWT({ fileChangeTrackerStorage: e }, r, R);
}
```

**关键**：`fsTracker` **per-thread** —— 切换 thread 要重建 tracker，因为 rollback / edit 作用域是 thread 级别的。

---

## Bootstrap Lifecycle

由 `oy0()`（`strings.txt:63349`）驱动：

```
┌──────────────────────────────────────────────────────────┐
│ Phase              │ Protocol action                     │
├──────────────────────────────────────────────────────────┤
│ 1. getSettingsSnapshot │ 本地读 config                   │
│ 2. executorHandshake   │ SEND: executor_connect          │
│                        │ RECV: { resumeBootstrap: bool } │
│ 3. environment         │ SEND: executor_environment_...  │
│                        │       snapshot                  │
│ 4. guidance            │ SEND: executor_guidance_snapshot│
│                        │       (chunked, isLast flag)    │
│ 5. skills              │ SEND: executor_skill_snapshot   │
│                        │       (chunked, 20 per batch)   │
│ 6. git                 │ SEND: executor_artifact_upsert  │
│                        │       (git status snapshot)     │
│ 7. tools               │ SEND: executor_tools_register   │
│                        │       (chunked)                 │
│ 8. complete            │ SEND: executor_tools_bootstrap_ │
│                        │       complete { ok: true }     │
└──────────────────────────────────────────────────────────┘
```

如果 server 回 `resumeBootstrap: true`（reconnect 场景），**跳过** phase 3-6（环境、guidance、skill、git）只做 tool 注册差分同步。

---

## 状态机 / Connection Roles

Transport（`CVT` class）状态：`disconnected | connecting | authenticating | connected | reconnecting`。
Role：`null | "observer" | "executor"`。

```js
handleConnectionChange(T) {
  // T.state !== "connected" → reset, 什么都不做
  // T.role === "executor"   → mark ready, 触发 resolveReadyWaiter
  // 其它                    → tryHandshake("connect")
}
```

**Handshake Manager**（`fVT`）做指数退避：
- `baseDelayMs`, `maxDelayMs`, `maxAttempts` 可配
- 每次 connection 变化 `generation++`，把旧的 handshake 废掉
- 耗尽重试后调 `onExhausted` → `kVT.shutdown(1, reason)`

---

## Tool Lease 流（实时编排）

整个 runtime 核心就是 server 推 `tool_lease` → client 响应：

```
Server → SEND:  { type: "tool_lease", toolCallId, toolName, args, ... }
Client → SEND:  { type: "executor_tool_lease_ack", toolCallId }
        （本地开始跑 tool，invokeTool 返回 Observable）
Client → SEND*: { type: "tool_progress", toolCallId, progress }   ← 多次
Client → SEND:  { type: "executor_tool_result", toolCallId, run: {...} }
Client → SEND*: { type: "executor_guidance_discovery", toolCallId, files, isLast }
                    （如果 tool 返回 discoveredGuidanceFiles）
Client → SEND:  { type: "executor_artifact_upsert", ..., toolCallId }
                    （跑完后自动发 git status 增量）
```

`IVT` (`ToolRunner`) 管理 active tool 的 Map。Server 可以 `tool_lease_revoked`：

```js
handleToolRevocation(T) {
  let r = this.activeTools.get(R);
  if (r) {
    r.abortController.abort();   // 让 tool 观察到 signal 中断
    r.subscription.unsubscribe();
    this.activeTools.delete(R);
  }
}
```

## Transport 自愈（`recoverTransport`）

`sendTransportMessage` 失败时触发。核心逻辑：

```js
async recoverTransport(clientId, msgType, error, ctx) {
  let e = this.clients.get(clientId);
  if (!e || e.recoveringTransport || this.disposed) return;
  e.recoveringTransport = true;
  // 如果连接中，等；否则 disconnect 重连
  if (info.state === "connected") e.transport.disconnect();
  let ok = await this.connectTransport(e.transport, clientId, "recovery");
  let newThreadId = e.transport.getThreadId();
  if (newThreadId) this.bindClientThread(e, newThreadId);
  if (ok) e.executorRuntime.ensureHandshake("retry");
}
```

**send 失败不是致命错误** —— terminal result 会 buffer 到 `pendingTerminalResults: Map<toolCallId, run>`，重连后 `flushBufferedTerminalResults()` 自动补发。

---

## 对 Alva 的启发

### 1. `EngineRuntime::RemoteRuntime` 角色反转

Amp 最精妙的设计点：**local CLI 是 executor worker，云端是 controller**。
对 Alva 来说，这意味着 `RemoteEngineRuntime` 不一定只是"把请求发到远端"，也可以是"把本地 CLI 变成远端的 worker 进程"：

```rust
// Variant A: Alva-as-executor
let runtime = RemoteExecutorRuntime::connect(ws_url, api_key).await?;
// 本地工具、agent loop 都不跑，只响应远端 tool_lease

// Variant B: Alva-as-controller
let runtime = LocalRuntime::new();
```

### 2. Per-thread `fsTracker` 重建

`bindClientThread` 展示了：**切 thread 要重建 file tracker**，因为 rollback 范围和 edit 历史按 thread 隔离。Alva 的 `CheckpointExtension` 现在是 session 级，想支持多 thread 同 CLI 就得像这样做一层 tracker 工厂。

### 3. `pendingTerminalResults` 缓冲

**这是最值得抄的点**。tool 跑完但 WebSocket 断了？不要丢 result，buffer 到 Map，重连后 flush。避免"本地干了活但服务器不知道"的一致性漏洞。

### 4. `resumeBootstrap` 的差分同步

server 说 "你上次 bootstrap 过了" → 跳 env/guidance/skill/git，只做 tool 差分。Alva 如果做远程协议，这个 fast-path 能把 reconnect 从 N 秒压到 <200ms。
