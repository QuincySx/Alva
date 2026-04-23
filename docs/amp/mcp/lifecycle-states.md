# MCP Server Lifecycle State Machine

Amp 给每个 MCP server 维护了一个状态机，状态变化会反映在 `mcpService.servers` Observable 上，UI / doctor / approval flow / `tools` 输出都订阅这个流。

## 状态枚举（反编译还原）

```js
type McpServerStatus =
  | { type: "loading" }                  // 初始化 / 刚注册
  | { type: "connecting" }               // transport 在握手
  | { type: "connected" }                // 握手 OK，tools 可能仍在 listing
  | { type: "reconnecting" }             // 连接掉了，自动重试
  | { type: "failed", error: Error }     // 连接或 handshake 爆了
  | { type: "denied" }                   // 用户主动拒绝（交互式弹窗）
  | { type: "awaiting-approval" }        // workspace-trust 挡住了
  | { type: "blocked-by-registry" }      // 企业/组织 registry 禁用
```

`tools` 字段和 status 正交：`status === "connected"` 的同时 `tools` 可能是 `Error`（listing 失败）或 `Array<McpTool>`（成功）或 `[]`（空）。

## 状态转换图

```
            ┌─────────────┐
            │   loading   │          <- 刚从 config 读到
            └──────┬──────┘
                   │ workspace? no-trust?
        ┌──────────┼────────────────────┐
        │          │                    │
        ▼          ▼                    ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐
│ awaiting-    │ │ connecting   │ │ blocked-by-registry  │
│ approval     │ └──────┬───────┘ │  (terminal)          │
└──────┬───────┘        │         └──────────────────────┘
       │ approve        │ handshake
       │                ▼
       │         ┌───────────────┐
       │         │  connected    │ ──── tools listing ok ────┐
       │         └───────┬───────┘                           │
       │                 │ disconnect                        │
       │                 ▼                                   ▼
       │         ┌───────────────┐                    ┌────────────┐
       │         │ reconnecting  │                    │ tools: [...]│
       │         └───────┬───────┘                    └────────────┘
       │                 │ failure
       │                 ▼                    tools listing failed
       │         ┌───────────────┐           ┌───────────────────┐
       ├────────▶│    failed     │           │ tools: Error(...)  │
       │  error  └───────────────┘           └───────────────────┘
       │
       │ deny
       ▼
┌───────────────┐
│    denied     │
└───────────────┘
```

## UI 呈现

反编译里的 UI 代码（TUI widget 构造）明确列出每一种状态的图标和文案：

```js
let u = y.type === "connected" ? "●" : "○";
let p =
    y.type === "connected" ? a.app.toolSuccess :
    y.type === "failed" || y.type === "denied" ? a.app.toolError :
    a.colors.warning;
// ...
if (y.type === "failed")           → ` └─ ${y.error.message}`     (红)
else if (y.type === "denied")      → ` └─ Denied by user`          (红)
else if (y.type === "awaiting-approval") → ` └─ Awaiting approval` (黄)
```

状态字符串格式（CLI 文本路径，`amp mcp doctor` 用到）：

```js
// from strings.txt:63054
case "connected": {
  return `${name}${e}: connected (${tools.length} tools: ${list.join(", ")})`;
}
case "connecting": return `${name}${e}: connecting...`;
case "reconnecting": return `${name}${e}: reconnecting...`;
case "failed": return `${T.name}${e}: error connecting — ${t.error.message}`;
```

## 观察路径

**`mcpService.servers`**：反编译里的直接引用：

```js
// from strings.txt:63796 (diagnostics/inspect)
await Promise.all([T.mcpService.initialized, T.toolboxService.initialized]);
let R = await g0(T.mcpService.servers);
for (let r of R)
  if (r.status.type === "failed")
    T.stderr.write(`error connecting to ${r.name}: ${r.status.error.message}`);
```

- `mcpService.initialized`：一个 `Promise` / observable，resolve 时说所有 server 至少进了 terminal state（要么 connected，要么 failed/denied/awaiting-approval/blocked-by-registry）
- `mcpService.servers`：Observable<Array<ServerEntry>>，UI 订阅后每次状态变就重 render
- `approveWorkspaceServer(name)`：用户交互 approve 时调
- `restartServers()`：`mcp reload` 命令背后的方法

## 监听完成（`mcp doctor` 命令）

反编译里看到 doctor 命令的 `wait until stable` 模式：

```js
// 订阅直到每个 server 都 "稳定"
o.subscribe({
  next: (b) => {
    if (b.every((s) => {
      if (s.status.type === "failed" ||
          s.status.type === "denied" ||
          s.status.type === "awaiting-approval" ||
          s.status.type === "blocked-by-registry")
        return true;
      if (s.status.type === "connected" && s.tools instanceof Error) return true;
      if (s.status.type === "connected" && Array.isArray(s.tools) && s.tools.length > 0) return true;
      return false;
    }) && b.length > 0) {
      o.unsubscribe();
      done();
    }
  },
});
```

意思：**loading / connecting / reconnecting 都不算稳定**，其它都算稳定（包括 connected but tools === `[]` 吗？上面代码看是不算的，`tools.length > 0` 才停；但实际情况应是最终 loading→connected→tools-ready 单向）。

## Workspace trust 的 transition

反编译里看到 `XXT` 类（类名 mangled）：

```js
class XXT {
  mcpService;
  workspaceFolder;
  pendingServers = new Set;      // 待审批的 server 名字
  notificationTimeout;
  _pendingServersSubject = new BehaviorSubject([]);
  pendingServers$ = this._pendingServersSubject;
  
  setupListener() {
    this.mcpService.onUntrustedWorkspaceServer = (serverName, reason) => {
      this.pendingServers.add(serverName);
      // debounce 通知
    };
  }
}
```

所以 workspace-trust 不是在 `register()` 阶段就 denied，而是：
1. Server 读到了 workspace config
2. 状态直接进 `awaiting-approval`
3. 触发 `onUntrustedWorkspaceServer` callback，UI 收集起来批量弹通知
4. 用户在 UI 点 approve → `mcpService.approveWorkspaceServer(name)` → 状态 `awaiting-approval → connecting → connected`

## `blocked-by-registry` 是啥？

猜测：企业版 Amp 里，Sourcegraph admin 可以设置"禁用 MCP server"白名单 / 黑名单。某个 server 被 registry 禁用时直接进这个终态，不尝试连接。

文档里没明确规则，但字符串枚举里它和 `awaiting-approval / denied / failed` 并列为"terminal non-connected"状态，doctor 命令认可这几种是稳定态。

## 对 Alva 的启发

`alva-protocol-mcp/types.rs` 当前有最小 `McpServerState`。建议扩成 Amp 的 8 态：

```rust
#[derive(Clone, Debug)]
pub enum McpServerState {
    Loading,                                  // 新增
    Connecting,
    Connected {
        tools_status: McpToolsStatus,         // 新增 tools 副状态
    },
    Reconnecting {                            // 新增
        attempt: u32,
        next_retry_ms: u64,
    },
    Failed {
        error: McpErrorCode,                  // 用分类错误，不光是字符串
        message: String,
    },
    Denied,                                   // 新增
    AwaitingApproval,                         // 新增
    BlockedByRegistry,                        // 新增（企业版 / 策略）
}

#[derive(Clone, Debug)]
pub enum McpToolsStatus {
    Loading,
    Ready(Vec<McpToolInfo>),
    Failed(String),
}
```

然后 `McpClient::servers_stream()` 返回一个 `tokio::sync::watch::Receiver<Vec<ServerEntry>>`，GPUI extension 订阅即可实时更新 UI。**这里必须是 `watch`** 而不是 `broadcast`，因为新订阅者应该立即看到最新快照（UI 冷启动、切 tab 回来等）。

`approve_workspace_server(name)` 方法 + `trust_list` 字段保存在 `McpClient` 内部即可；不要每次连接都 I/O 去读 settings。配置 reload 走专门的 `reload_config()` 路径。
