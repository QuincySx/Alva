# DTW —— Distributed Thread Worker

> Amp 跑在云端的 executor runtime。底层是 Cloudflare Workers / Durable Objects。

---

## 证据链

从反编译里交叉验证出这套架构：

1. **调试 URL 提供 Cloudflare 链接**：
   ```
   - Thread URL: https://ampcode.com/threads/T-xxx
   - Cloudflare Logs: <WhT(threadID)>
   - Cloudflare Data Studio: <HhT(threadID)>
   ```

2. **CLI flag**：
   ```
   --apply <threadID>       # Apply the current DTW thread snapshot once and exit
   --checkout               # Automatically check out the thread commit when it differs
   --skip-checkout          # Skip the startup checkout prompt when commits differ
   --worker-url <url>       # Override DTW worker URL
   ```

3. **API**:
   ```js
   l3.createRemoteExecutorThread({ prompt, repositoryURL })
   // 返回 { ok: true, result: { url } }
   ```

4. **ThreadPool 状态**:
   ```js
   threadPool.isDTWMode?.()                    // boolean
   threadPool.getTransportConnectionState?.()
   threadPool.getTransportConnectionRole?.()
   ```

5. **Executor Kind**:
   ```js
   system.executor = { kind: "local" | "dtw" | "unknown" }
   ```

---

## 架构

```
┌───────────────────┐      WebSocket / HTTP      ┌──────────────────────┐
│  Local Amp CLI    │────────────────────────────│  DTW Worker          │
│                   │                             │  (Cloudflare Workers)│
│  - 用户 UI        │                             │                      │
│  - fileSystem     │  executor_guidance          │  - 独立 Durable       │
│  - fileChangeTracker│  _snapshot (分片)           │    Object per thread │
│                   │                             │  - in-memory state   │
│                   │◀───────────────────────────│  - 独立 git worktree  │
│                   │    message stream           │  - 全部 Amp tools    │
│                   │                             │                      │
│  [--apply]        │                             │                      │
│  拉 snapshot 回本地│                             │                      │
└───────────────────┘                             └──────────────────────┘
```

### Durable Object per Thread

Cloudflare Durable Objects 提供"每对象一份 in-memory state"的抽象。Amp 每个 execution thread 对应一个 DO：

- **持久化状态**：即使所有 client 断开，thread 仍活着
- **Geo-located**：DO 实例绑定到单一地理位置，减少 state 同步复杂度
- **WebSocket 原生支持**：长连接实时推消息

---

## `executor_guidance_snapshot` 协议

本地 CLI 连上 DTW 时，要把 AGENTS.md 等"环境规则"推过去（远程没有本地文件系统）：

```js
// 本地生成 snapshot ID
let snapshotID = crypto.randomUUID();

// 分片推送 AGENTS.md 文件
let chunks = splitIntoChunks(agentMdFiles, CHUNK_SIZE);
for (let i = 0; i < chunks.length; i++) {
  let isLast = i === chunks.length - 1;
  sendMsg({
    type: "executor_guidance_snapshot",
    snapshotID,
    files: chunks[i],
    isLast,
    userConfigDir: /* path */
  });
}
```

远程 DTW 拼回完整文件集，和本地看到的完全一致的 AGENTS.md 上下文。

---

## `--apply <threadID>`

"拉远程 snapshot 到本地"：

```bash
amp --apply T-xxx
# 或：
amp --apply https://ampcode.com/threads/T-xxx
```

**做什么**：
1. 连到 DTW thread
2. 拉它当前 worktree 的 snapshot（files + git commit hash）
3. 应用到本地工作目录
4. 可选 `--checkout`：如果 commit 不同，自动 `git checkout`

**场景**：
- 开发者在云端让 agent 跑 2 小时
- 完事后 `amp --apply T-xxx` 把结果拉本地 review
- 想重新跑可以回到 web UI 发新指令

---

## Thread Pool Transport

```js
threadPool = {
  isDTWMode: () => boolean,
  getTransportConnectionState: () => "connecting" | "connected" | "disconnected" | ...,
  getTransportConnectionRole: () => "client" | "worker" | ...,
  // ...
}
```

CLI 有多连接模式：

- **Local only**：没有 DTW，所有 thread 纯本地
- **DTW attached**：连到一个 DTW thread 做 remote executor
- **DTW observer**：只看 DTW 状态，不 执行

---

## 远程 executor 的"双 Amp" 配置

DTW 实际上是**另一个 Amp 实例**跑在 Cloudflare Worker 里：

```
用户本地 CLI (Amp 实例 A)
   │
   │ WebSocket
   ▼
Cloudflare Durable Object (运行 Amp 实例 B)
   │
   ├── 独立 worktree
   ├── 独立 tool registry（全套 builtin）
   ├── 独立 skill store
   ├── 独立 agent loop
   └── 独立 plugin 系统（但 .amp/plugins 按 DO 的 FS 加载）
```

两个 Amp 实例通过 thread service（ampcode.com）同步 messages。双方都能看到同一个 thread 的完整历史。

---

## 为什么是 Cloudflare Workers / DO

推断的设计动机：

1. **Global deployment**：用户在全球任何地方连的延迟都低
2. **Durable state 天然持久化**：agent 跑 2 小时不用操心 session 丢
3. **每 thread 独立进程**：安全隔离天然成立
4. **按需冷启动**：thread 不活跃时释放资源，用户重连时冷启
5. **原生 WebSocket**：双向流直接支持

---

## Security / Sandbox

`meta.executorType === "sandbox"` 触发额外的 prompt 内容：

```
Sandbox preview URLs: The user cannot open sandbox-local URLs directly, 
so never tell them to use raw localhost or 127.0.0.1 for sandbox web servers.
Only share a preview URL when this environment or repo explicitly provides 
how to derive one.
```

DTW 可能是 `sandbox` executor type 的一种，或者是独立 type。

---

## 对 Alva 的启发

你们是 local-first，DTW 整套不适合直接抄。但有几个点值得吸收：

### 1. `EngineRuntime` trait 设计要能支持 remote

你们 `alva-engine-runtime` 已经抽象了 `EngineRuntime` trait。加个 `RemoteEngineRuntime` adapter 让同一套 Alva agent loop 能跑在不同执行后端：

```rust
trait EngineRuntime {
    async fn execute(&self, req: ExecuteRequest) -> impl Stream<Item = RuntimeEvent>;
    async fn cancel(&self, request_id: RequestID);
    async fn capabilities(&self) -> RuntimeCapabilities;
}

// 现有：
impl EngineRuntime for LocalAlvaRuntime { ... }
impl EngineRuntime for ClaudeCodeRuntime { ... }

// 未来：
impl EngineRuntime for RemoteWorkerRuntime { ... }   // 跑在 Docker / Podman / cloud
```

### 2. `executor_guidance_snapshot` 的分片上传模式

如果 Alva 未来有远程 agent，AGENTS.md / Skills / plugin 必须能**分片**推过去（HTTP body 有大小限制）。

### 3. Durable state / Checkpoint

Alva 已有 `CheckpointExtension`。对照 Amp：

- **Mid-turn checkpoint**：不只是 turn 之间，turn 内部（比如每个 tool call 完成后）也能恢复
- **Checkpoint 包括 pending tool calls 和 context** 的完整状态，不只是 messages
- **断线后能从 checkpoint 无缝恢复**（用户 Ctrl-C 重开也能接着）

### 4. `--apply <session-id>` 的 one-shot 应用模式

对 CI 很友好：

```bash
alva apply S-xxx        # 从远程/checkpoint 把改动应用到本地 worktree
alva apply S-xxx --checkout   # 自动 git checkout 到 agent 做改动时的 commit
```

### 5. Executor Kind 作为 prompt 上下文

告诉 LLM "你现在跑在 sandbox / 本地 / 远程"。LLM 可以据此改变行为（sandbox 下不说 localhost 预览）。你们的 Environment block 应该包含这个信息。
