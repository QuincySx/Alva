# Amp WebSocket Protocol

> CLI ↔ thread-actor (Rivet/DTW) 之间实时双工协议。用在两个场景：headless executor（CLI as worker）和 DTW attach（Web UI as controller + CLI as observer）。

---

## 1. Transport Layer（`CVT` class）

```js
config = {
  baseURL,                     // https://ampcode.com  OR  Rivet endpoint
  threadId,                    // T-xxxx（UUID base-62）
  apiKey,
  wsToken,                     // optional 2-step auth
  webSocketProvider,           // Rivet thread-actor getOrCreate().webSocket('/')
  WebSocketClass: WebSocket,
  reconnectDelayMs: 1000,
  maxReconnectDelayMs: 30000,
  maxReconnectAttempts: +Infinity,
  pingIntervalMs: 5000,        // heartbeat
  connectTimeoutMs,
  useThreadActors: true,       // new path via Rivet
}
```

**状态机**：`disconnected → connecting → authenticating → connected → reconnecting → disconnected`
**角色**：`null | "observer" | "executor"` —— 每个 client 连进去时未定，server 在 handshake 后分配。

认证两条路：
- **1-step**：CLI 直接 `wss://.../threads?threadId=T-xxx`，header `Authorization: Bearer <apiKey>`
- **2-step**：先 REST `POST /api/thread-actors` 换 `wsToken`，再 Rivet actor 认证（headless 走这条）

**心跳**：`ping/pong` 每 5s；长时间无 pong 触发 `ping_timeout`。

---

## 2. Message 总清单

消息格式统一为：`{ "type": "<name>", ...fields }`。下面按方向分组（从 `strings.txt:63347`, `63354`, `65970` 交叉验证）。

### 2.1 Client → Server （CLI 发）

| Type | Payload 关键字段 | 用途 |
|---|---|---|
| `client_spawn_executor` | — | 请求 server 进入 executor attach 流程 |
| `client_append_user_msg` | `text`, `content` | 发用户消息 |
| `client_append_manual_bash_invocation` | `command`, `output` | 把本地手动跑的命令塞进历史 |
| `client_edit_message` | `messageId`, `content` | 编辑历史消息 |
| `client_cancel` | — | 打断当前 generation |
| `client_interrupt_queued_msg` | — | 停所有排队消息 |
| `client_remove_queued_msg` | `id` | 移掉一条排队的 |
| `client_mark_message_read` / `_unread` | `messageId` | 只读状态 |
| `client_retry` | `fromMessageId` | 从某条消息重跑 |
| `client_resume` | — | 断线后恢复 |
| `client_set_thread_title` | `title` | 改 thread 标题 |
| `client_tool_approval_response` | `toolCallId`, `accepted`, `denyFeedback?` | HITL tool approval |
| `client_filesystem_read_file` | `uri` | observer 请求对端读文件 |
| `client_filesystem_read_directory` | `uri` | observer 请求对端 ls |
| `client_upsert_notification_subscription` | `kind`, `config` | 通知订阅 |

### 2.2 Server → Client / Executor （thread-actor 发）

| Type | Payload 关键字段 | 方向 | 用途 |
|---|---|---|---|
| `snapshot` | `value` | 广播 | 整个 thread state snapshot（reconnect 首帧） |
| `delta` | `blocks`, `state: "generating" \| ...` | 广播 | LLM token stream |
| `message_added` / `message_edited` | `message` | 广播 | thread message 增量 |
| `queued_message_added` / `_removed` | `message` | 广播 | 队列变化 |
| `thread_title` | `title` | 广播 | title 变更 |
| `thread_status` | `status` | 广播 | thread 状态（generating/idle/error） |
| `thread_relationships` | `relationships[]` | 广播 | fork/handoff/mention |
| `artifacts_snapshot` / `artifact_upserted` / `artifact_deleted` | `key`, `contentBase64` | 广播 | git 状态、guidance、skill 等产物 |
| `agent_state` | — | 广播 | agent 当前 state |
| `environment_update` | `environment` | 广播 | env 变化 |
| `executor_connected` | `clientId` | 广播 | 有 executor 接入了 |
| `executor_status` | `phase` | 广播 | bootstrap 进度 |
| `executor_error` | `error` | 广播 | executor 侧错误 |
| `tool_lease` | `toolCallId`, `toolName`, `args` | → executor | 派 tool 给 executor |
| `tool_progress` | `toolCallId`, `progress` | 广播 | tool 增量输出 |
| `executor_tool_lease_revoked` | `toolCallId`, `reason` | → executor | 撤回 tool |
| `executor_rollback_request` | `editId`, `toolUseIdsToRevert[]` | → executor | 要求回滚文件 |
| `executor_filesystem_read_file` | `requestId`, `uri` | → executor | 远端请求读文件 |
| `executor_filesystem_read_directory` | `requestId`, `uri` | → executor | 远端请求读目录 |
| `tool_approval_queue` | `pending[]` | 广播 | HITL 待批队列 |
| `observers` | `count` | 广播 | 连了几个观察者 |
| `compaction_started` / `_complete` / `_records` | — | 广播 | context compaction 生命周期 |
| `retry_scheduled` / `_started` / `_cancelled` | — | 广播 | retry 状态 |
| `error_set` / `error_cleared` / `error` | `error` | 广播 | error 状态 |
| `edit_rejected` | `editId`, `reason` | 广播 | edit 被拒 |
| `plugin_message` | `message` | 广播 | plugin 内部事件 |

### 2.3 Executor → Server （headless CLI 发）

这是 bootstrap + tool result 的核心。`sendXxx` 映射到 `CVT.sendXxx()`：

| Type | Payload 关键字段 | Bootstrap phase |
|---|---|---|
| `executor_connect` | `clientId`, `capabilities`, `executorType` | handshake |
| `executor_environment_snapshot` | `environment: { workspaceRoot, platform, tags, os }` | phase 3 |
| `executor_environment_update` | `environment` | runtime |
| `executor_guidance_snapshot` | `snapshotId`, `files[]`, `isLast`, `userConfigDir` | phase 4, **chunked** |
| `executor_skill_snapshot` | `snapshotId`, `skills[]`, `isLast` | phase 5, **chunked, 20/batch** |
| `executor_artifact_upsert` | `{ key, dataType, contentBase64 }`, `toolCallId?` | phase 6 + per-tool |
| `executor_artifact_delete` | `key` | runtime |
| `executor_tools_register` | `tools[]` | phase 7, chunked |
| `executor_tools_unregister` | `toolNames[]` | runtime |
| `executor_tools_bootstrap_complete` | `ok`, `error?` | phase 8 |
| `executor_tool_lease_ack` | `toolCallId` | 回 `tool_lease` |
| `tool_progress` | `toolCallId`, `progress` | 跑 tool 中 |
| `executor_tool_result` | `toolCallId`, `run: { status, result \| error \| reason, progress, trackFiles }` | tool 完成 |
| `executor_guidance_discovery` | `toolCallId`, `files[]`, `isLast` | tool 发现 guidance |
| `executor_rollback_ack` | `editId`, `ok`, `error?` | 回 rollback request |
| `executor_filesystem_read_file_result` | `requestId`, `ok`, `contentBase64 \| error` | 回 fs 请求 |
| `executor_filesystem_read_directory_result` | `requestId`, `ok`, `entries \| error` | 回 fs 请求 |
| `executor_tool_approval_request` | `toolCallId`, `toolName` | HITL 转发 |
| `executor_settings_update` | `settings` | config 变化 |
| `executor_plugin_message` | `message: { type:"event", event, data }` | plugin notify 转发 |

---

## 3. `executor_guidance_snapshot` 分片协议

**问题**：AGENTS.md / `.amp/` 配置文件可能几千行。WebSocket 每帧有大小限制，需要分片。

**实现**（`strings.txt:63350`, function `ey0`）：

```js
let snapshotId = crypto.randomUUID();                // 每次 snapshot 独立 ID
let chunks = lH(files, chunkSize, (chunk) => ({      // 按大小切
  type: "executor_guidance_snapshot",
  snapshotId,
  files: chunk,
  isLast: false,
  userConfigDir,
}));

for (let i = 0; i < chunks.length; i++) {
  let isLast = (i === chunks.length - 1);
  transport.sendExecutorGuidanceSnapshot({
    snapshotId,
    files: chunks[i],
    isLast,
    userConfigDir,
  });
}

// 空文件集也要发一条 isLast:true 通知
if (files.length === 0) {
  transport.sendExecutorGuidanceSnapshot({ snapshotId, files: [], isLast: true, userConfigDir });
}
```

**Server 组装规则**（推断）：
- 见到新 `snapshotId` → 开新 buffer
- 累积所有 `files[]` 到 buffer
- `isLast: true` → commit buffer，替换 thread 上绑定的 guidance snapshot
- 跨 snapshot **没有依赖** —— 新 snapshotId 来了旧的立即丢

File shape:
```ts
{ uri: "file:///path/AGENTS.md", content: "...", lineCount: 123, hash?: string }
```

**Skill 也用同样模式**（`Ay0`）：`snapshotId` + 20 skills/chunk + `isLast` flag。

**Discovery 变种**（`executor_guidance_discovery`）：tool 跑完后如果 result 里带 `discoveredGuidanceFiles`，以 `toolCallId` 为分组 key 推回 —— 动态发现的 guidance（比如 agent 新建的 SKILL.md）。

---

## 4. Artifact 协议（双向）

Artifact 是"state-like 的 KV"，典型 key 有 `git-status`、`environment`、`tool-registry`。

```ts
Artifact = {
  key: string,
  dataType: string,                     // MIME, e.g. "application/json"
  contentBase64: string,
  toolCallId?: TU-XXX,                  // 可选关联
  updatedAt: string,
}
```

- `executor_artifact_upsert` (exec → server): 覆盖 artifact
- `executor_artifact_delete` (exec → server): 删
- `artifacts_snapshot` / `artifact_upserted` / `artifact_deleted` (server → observers): 广播

**git status 特殊处理**：每次 tool 跑完 `IVT.queueGitStatusSnapshot(toolCallId)` 自动触发 —— debounced（`inFlight` flag），同时多个请求 coalesce 成一个。

---

## 5. 文件系统 RPC（server 拉本地文件）

当 observer（比如 Web UI）要看 executor 本地文件：

```
Observer → SEND:  { type: "client_filesystem_read_file", requestId, uri }
     ↓ server forwards
Executor ← RECV:  { type: "executor_filesystem_read_file", requestId, uri }
Executor → SEND:  { type: "executor_filesystem_read_file_result",
                    requestId,
                    ok: true, contentBase64 }
     ↓ server relays
Observer ← RECV:  { type: "client_filesystem_read_file_result", ... }
```

错误码枚举（来自 schema `hC0`）：
```
INVALID_URI | EXECUTOR_NOT_CONNECTED | NOT_FOUND | NOT_DIRECTORY |
IS_DIRECTORY | ACCESS_DENIED | INTERNAL_ERROR
```

**安全含义**：server **可以**透过 executor 读任意本地文件。executor 没有白名单拦截（至少这一层没有）。假设：信任来自服务器的请求 = 信任登录的用户。

---

## 6. 错误处理与 drop 语义

- 发送 message 抛 `NetworkError (pt)` → transport 不丢 message 进 queue，返回 `false`，调用方决定 retry
- `executor_tool_result` 特殊处理：失败时 buffer 到 `pendingTerminalResults: Map<toolCallId, run>`，重连后 `flushBufferedTerminalResults()` 自动补发
- 其他 message（progress, artifact upsert）失败就丢，依赖上层幂等性（git status 反正下次 tool 完成会重发）
- server 发 `close` / `connect_failed` / `ping_timeout` → transport 进 `reconnecting`，reconnect lifecycle 带 `reconnectCause` 上下文

---

## 7. Message 校验

从 `Nb0` / `wb0` 看，每条 message server 侧用 Zod schema 校验。失败分类：

```
invalid_json | invalid_shape | missing_type | invalid_type | unknown_type
```

`unknown_type` → "likely protocol version mismatch"。说明 Amp 允许客户端带未知 type，server 忽略不崩。前后兼容性通过"`type` 是 discriminator + 未知 type 不致命"保证。

---

## 对 Alva 的启发

### 抄这一个：**分片 snapshot + `snapshotId` 语义**

Alva 本地没远程，但你们有 `EngineRuntime` 抽象可能接 remote。`executor_guidance_snapshot` 的 pattern 是最干净的：

```rust
#[derive(Serialize)]
#[serde(tag = "type")]
enum RuntimeMsg {
    #[serde(rename = "guidance_snapshot")]
    GuidanceSnapshot {
        snapshot_id: Uuid,         // 每次 bootstrap 一个新 ID
        files: Vec<GuidanceFile>,  // 这一片
        is_last: bool,             // 最后一片才 commit
        user_config_dir: Option<PathBuf>,
    },
}
```

优点：
1. **接收方不需要 total count** —— 不用预告 "我要发 N 片"
2. **`isLast` 天然表示 commit 点** —— 避免 "我发了 5 片但 server 只收了 4 片" 的悬挂状态
3. **新 snapshotId 覆盖旧的** —— reconnect / 文件变更后直接发新 snapshot，旧的 GC
4. **天然 chunked** —— HTTP / WebSocket / gRPC stream 都能跑

同样可以套用到 skill、tool registry、environment snapshot 的任何增量 / 快照类状态。
