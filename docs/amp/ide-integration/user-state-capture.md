# User State Capture

WebSocket-based IDE bridge 的协议——IDE 插件怎么把 `activeEditor` / `selectionRange` / `visibleFiles` 发给 Amp CLI；Amp 反向怎么 `openURI` / `getDiagnostics`。从反编译看是双向 JSON-RPC（不是 MCP/LSP，自己 rollout 的协议）。

## 通道设置：lockfile → WebSocket

### 插件启动时写 lockfile

路径：`~/.local/share/amp/ide/*.json`（通过 XDG_DATA_HOME 推导）。Schema (Zod `HPT`):

```ts
{
  workspaceFolders: string[],       // ["file:///Users/me/proj"]
  port: number,                      // 插件本地监听端口
  ideName: string,                   // "VS Code" / "IntelliJ IDEA" / "Cursor"
  authToken: string,                 // 32-char random, 验证用
  pid: number,                       // 插件进程 pid（插件进程 = Amp 用 s$(pid) 判断存活）
  connection?: "ws" | "query",       // 缺省 "ws"
  workspaceId?: number               // query-based 独有
}
```

### Amp CLI 连 WebSocket

```js
let ws = new WebSocket(`ws://localhost:${T.port}?auth=${encodeURIComponent(T.authToken)}`);
```

**只在 localhost**，authToken 塞进 query string。没有 TLS（本机安全假设）。

### 连接生命周期

- `onopen` → set `{connected: true, authenticated: true, connectionState: "connected"}`
- `onclose` code `1000` + reason `"Authentication failed"` → 特殊错误 `"IDE authentication failed - try restarting your IDE"`
- 其他断开 → `scheduleReconnect()` 指数退避
- `onerror` → clear pending requests

## 协议 Schema

全部走 Zod 定义，单条消息是 envelope `UPT`:

```ts
UPT = {
  serverNotification?: AsT,          // IDE → Amp (push-style, 不要求 response)
  serverResponse?: csT               // IDE → Amp (request 的回复)
}

GbR = { clientRequest: ssT }          // Amp → IDE
```

### 发送：`Amp → IDE` 的 3 种 request

```ts
ssT = {
  id: string,
  ping?: { message: string },         // 心跳
  getDiagnostics?: {                  // 取编译错误/警告
    path: string                       // "Absolute path to file or directory"
  },
  openURI?: {                          // 反向打开文件
    uri: URL                           // "URI to open in the IDE (file://, http://, etc.)"
  }
}
```

### 收回：`IDE → Amp` 的 response (`csT`)

```ts
csT = {
  id: string,                          // 对应 request 的 id
  error?: { code: number, message: string },
  ping?: { message: string },          // "beepboop" 回声
  getDiagnostics?: {
    entries: [{
      uri: string,                     // file URI
      diagnostics: [{
        range: { startLine, startCharacter, endLine, endCharacter },
        severity: string,              // "error" | "warning" | "info" | "hint"
        description: string,
        lineContent: string,           // 出错那行文本
        startOffset: number,           // 字节 offset
        endOffset: number
      }]
    }]
  },
  openURI?: {
    success: boolean,
    message?: string                   // "Optional error or info message"
  }
}
```

### 收回：`IDE → Amp` 的 3 种 notification (`AsT`)

```ts
AsT = {
  selectionDidChange?: osT,            // 用户改了光标/选区
  visibleFilesDidChange?: nsT,         // tab 切换 / 打开 / 关闭
  pluginMetadata?: CsT                 // 插件自报版本，启动时发一次
}

osT = {
  uri: URL,                            // active file
  selections: [{                       // 多光标/多选区
    range: { startLine, startCharacter, endLine, endCharacter },
    content: string                    // "The selected text. When range is offset (start === end), content is the text of the line containing the offset."
  }]
}

nsT = {
  uris: URL[]                          // 所有可见 tab
}

CsT = {
  version: string,                     // 插件版本
  pluginDirectory?: string             // 插件安装路径
}
```

**极其重要**：`selections` 是数组——**多光标/多选区**直接支持。每个 selection 带 `content`（选中文本），empty selection (cursor) 返回**整行文本**，跟 query 模式 (`hbR`) 行为一致。

## Amp 端的状态合并：`handleNotification`

```js
handleNotification(T) {
  if (!T) return;

  if (T.selectionDidChange) {
    this.sendStatus({
      selections: T.selectionDidChange.selections,
      openFile: T.selectionDidChange.uri
    });
  }
  else if (T.visibleFilesDidChange) {
    let R = T.visibleFilesDidChange.uris;
    this.sendStatus({
      visibleFiles: R,
      ...(R.length === 0 && { openFile: void 0, selections: void 0 })
    });
  }
  else if (T.pluginMetadata) {
    this.sendStatus({
      pluginVersion: T.pluginMetadata.version,
      pluginDirectory: T.pluginMetadata.pluginDirectory
    });
  }
}
```

**关键语义**：
- `visibleFiles = []`（全部关闭）触发**连带清空 openFile 和 selections**——避免 stale state
- `selectionDidChange` 同时更新 `openFile` 和 `selections`（uri 顶级字段代表当前 active editor）

所有状态都 flow 进 `statusSubject` (RxJS BehaviorSubject)，UI 和 prompt 构建器 subscribe。

## Amp 端的请求封装：`sendRequest(method, params)`

```js
sendRequest = (T, R) => {
  let t = this.ws;
  if (!t || this.ws.readyState !== 1) return Promise.reject(Error("WebSocket is not open"));
  return new Promise((resolve, reject) => {
    let id = `${this.id++}`,
        envelope = { clientRequest: { id, [T]: R } },
        timeout = setTimeout(() => {
          this.pendingRequests.delete(id);
          reject(Error(`Timeout after ${bsT}ms`));
        }, bsT);
    this.pendingRequests.set(id, { resolve, reject, timeout, method: T });
    t.send(JSON.stringify(envelope));
  });
};
```

**Request/response matching**：
- id 是自增字符串（`"1"`, `"2"`, ...）
- 每次 reconnect 重置 id
- pendingRequests `Map<id, {resolve, reject, timeout, method}>`
- `handleResponse` 按 id 查找，按 method 名取 response 里对应字段

### `handleResponse`

```js
handleResponse(T) {
  if (!T?.id) return;
  let R = this.pendingRequests.get(T.id);
  if (!R) return;
  clearTimeout(R.timeout);
  this.pendingRequests.delete(T.id);
  if (T.error) { R.reject(Error(JSON.stringify(T.error))); return; }
  let t = T[R.method];                   // T.openURI / T.getDiagnostics / T.ping
  if (t) R.resolve(t);
  else R.reject(Error(`Invalid response for method ${R.method}`));
}
```

**按 method 名取字段** —— 也就是说 IDE 插件回复 `openURI` 请求时，payload 是 `{id, openURI: {success, message}}`，不是 `{id, result: {...}}`。这是 Amp 自造的协议风格，不跟 JSON-RPC 2.0 完全一致（JSON-RPC 是 `{id, result}`）。

## Amp 端的 ping 心跳

```js
async isConnected() {
  if (this.projectConfig?.connection === "query") return this._status.connected === true;
  if (!this._status.authenticated) return false;
  if (!this.isWsOpen()) return false;
  try {
    return (await this.sendRequest("ping", { message: "beepboop" }))?.message === "beepboop";
  } catch { return false; }
}
```

每次 reconnect 验证时 ping `"beepboop"` → 期望回 `"beepboop"`。简单对称 echo，不是时间戳。

## `requestDiagnosticsFromIDE` — 让 IDE 跑 LSP

```js
async requestDiagnosticsFromIDE(T) {
  try { return await this.sendRequest("getDiagnostics", { path: T }); }
  catch (R) { TT.debug("ide-diags: failed...", { error: R, path: T }); }
}
```

这是 agent 的 `get_diagnostics` tool (`SFR` in reverse) 底层实现——**让 IDE 的 LSP 跑**，而不是 Amp 自己起一个 LSP 客户端。优势：
- 不用 Amp 维护 N 个 LSP client（TypeScript / Rust / Python / Go ...）
- 复用 IDE 的 LSP 配置（用户已经配好 workspace settings）
- IDE 已经在 index，Amp 直接读结果

`SFR` 是只在 **IDE 连接上时才注册的 tool**：

```js
let S = Nr.status.pipe(
  T0(E => Boolean(E.connected && E.authenticated && E.ideName && FbR(E.ideName))),
  $9()
).subscribe(E => {
  if (E) { if (!p) p = A.registerTool(SFR); }
  else p?.dispose(), p = void 0;
});
```

`FbR(ideName)` 判断是否 JetBrains（只有 JetBrains 分支暴露 diagnostics？——这个条件有点怪，但代码里就是这样）。

## 插件端需要实现什么（给想写 Amp 插件的）

1. **启动时写 lockfile**：`~/.local/share/amp/ide/<random>.json`，schema 见上
2. **监听 WebSocket** on `localhost:<port>`，query param `?auth=<token>`，验 token
3. **订阅 IDE 事件**：
   - editor cursor/selection 变化 → 发 `selectionDidChange`
   - tab 打开/关闭/切换 → 发 `visibleFilesDidChange`
4. **响应 request**：
   - `ping` → 直接 echo
   - `openURI` → 调 IDE 的 "open file at position" API
   - `getDiagnostics` → 从 LSP / problem panel 拿错误列表
5. **退出时**：删掉 lockfile（或者靠 Amp 启动时 `FPT()` 清理僵尸）

## 已知缺陷（反编译能看到的）

1. **Windows 的 XDG_DATA_HOME fallback** —— Amp 的 `nN = path.join(XDG_DATA_HOME ?? join(home, ".local/share"), "amp")` 在 Windows 上会变成 `C:\Users\xxx\.local\share\amp`（不规范，应该是 `%LOCALAPPDATA%\amp`）。
2. **多 workspace 时只用 `workspaceFolders[0]`** —— lockfile 里是数组，但很多地方取 `[0]`，多根 workspace 信息丢失。
3. **diagnostics 只在 JetBrains 注册 tool** —— `FbR()` 过滤掉 VS Code。看反编译注释似乎 VS Code 插件的 diagnostics 流没实现，或者有 bug。
4. **没有 `appendToChat` 从 IDE 推消息进 agent** —— 这个功能可能是通过 plugin 系统（见 `../plugins/`）的 `thread.append` RPC 间接实现。

## 对 Alva 的启发

1. **抄整个 Zod schema 当协议** —— Alva 也用 TypeScript / Rust，可以完全复刻 `selectionDidChange` / `visibleFilesDidChange` / `pluginMetadata` / `getDiagnostics` / `openURI`。这套协议覆盖 **90% 的 IDE 集成需求**，很成熟。
2. **ping echo "beepboop"** —— 看起来幼稚但可读性好（debug log 很容易识别），比返回 `{ timestamp }` 还直观。
3. **lockfile + authToken** —— Tauri 应用通常 app 里有个 dev server，这里学 Amp：不是让 IDE 插件发现 Tauri，而是让 IDE 插件**主动写 lockfile**、Tauri 去扫。这样 Tauri 也有 query 模式（插件不在时降级），同时 authToken 防 cross-origin。
4. **"diagnostics 让 IDE 跑 LSP" —— 对 Alva 很重要**。Alva 如果要支持 `get_diagnostics` tool，不要自己起 LSP 客户端（慢、维护成本高），走 IDE bridge 发 `getDiagnostics` request。
5. **状态合并策略抄**：
   - `visibleFiles = []` 连带清空 `openFile` / `selections`
   - `selectionDidChange` 同时更新 `openFile`（active file = 带 selection 的 file）
   - RxJS BehaviorSubject / Rust 的 `tokio::sync::watch` 做 状态广播
6. **多光标从第一天就支持** —— `selections: Selection[]`（不是 `selection: Selection | null`）。Amp 只是很少用到，但协议留了口子。
7. **别全用 JSON-RPC 2.0** —— Amp 按 method 名直接放字段（`{openURI: {...}}` 而不是 `{result: {...}, method: "openURI"}`），代码可读性更好，type-safe。Alva 用 Rust serde 更容易建这种 tagged union。
