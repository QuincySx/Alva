# MCP OAuth Flow

Amp 对 MCP servers 支持完整的 OAuth 2.0 授权，关键组件：**Dynamic discovery / Shared callback server / Secret storage / 三件套 CLI 命令**。

## 触发条件

一个 MCP server 走 OAuth 路径当且仅当：

```js
// 反编译里的判断（status 命令中）
if ("url" in A && !("headers" in A) && !("command" in A)) {
  // OAuth path
}
```

也就是 config **只有 `url`**（没预先配 header，也不是本地 command）。这种情况 Amp 假设对方是个 remote OAuth-capable MCP server（典型：`https://huggingface.co/mcp`、`https://mcp.monday.com/sse`）。

## Shared callback server

**关键设计**：**所有**需要 OAuth 的 MCP server 共用一个本地回调 server，监听 `127.0.0.1:8976`。

```js
// from strings.txt:63069~63070
this.server.listen(this.port, NWR, () => {...});
// port 在 register_oauth_client 里 hard-coded 成 8976:
//   redirectUrl: "http://localhost:8976/oauth/callback"
```

回调路由用 `host` 头判断是哪个 server（反编译里看到 `handleSharedRequest`）。失败时的友好提示：

```
OAuth callback port 8976 is already in use.

1. Run: lsof -i :8976 | grep LISTEN
2. Kill the process: kill <PID>
3. Then retry the OAuth flow
```

## Discovery

第一次 `amp mcp oauth login <name> --server-url <url> --client-id <id>` 时，Amp 走 RFC 8414 discovery：

```js
// from strings.txt:63515~63517
`Discovering OAuth endpoints for ${serverUrl}...`
let C = new URL("/.well-known/oauth-authorization-server", serverUrl);
let o = await fetch(C.toString());
if (!o.ok) throw Error(
  `OAuth discovery failed (HTTP ${o.status}). Provide --auth-url and --token-url manually.`
);
let n = await o.json();
s = s || n.authorization_endpoint;  // auth URL
c = c || n.token_endpoint;          // token URL
if (!s || !c) throw Error(
  "OAuth endpoints not found in discovery metadata. Provide --auth-url and --token-url manually"
);
```

Discovery 失败 → fallback `--auth-url` / `--token-url` 手动传。

## 保存 client info

discovery 成功后，把整包 client info 写进 `secretStorage`：

```js
// from strings.txt:63517
await i.saveClientInfo(t, {
  clientId:     r.clientId,
  clientSecret: r.clientSecret,
  redirectUrl:  "http://localhost:8976/oauth/callback",
  authUrl:      s,
  tokenUrl:     c,
  scopes:       A,
  serverUrl:    r.serverUrl,
});
```

然后出文案：

```
✓ OAuth client registered for "<name>"
   Client ID: <id>
   Scopes: <scopes>
   
Your browser will automatically open for authorization when you start up the Amp coding agent.
```

**注意**：login 命令本身**不**立即走 OAuth 拿 access token，它只做 discovery + 保存 client info。真正的 browser flow 发生在 Amp 下次启动连 MCP server 时。

## 运行时 OAuth（CLI startup 触发）

MCP server 连接失败且错误带 `OAuth required` 时，Amp 显示：

```
OAuth Authorization Required
────────────────────────────────────────────────────────────
Open this URL in your browser to authorize:

<auth URL with state + code_challenge>

After authorizing, you will be redirected to a localhost URL.
The redirect will fail - this is expected in headless mode.
```

（headless 模式文案。交互式 TUI 模式会自动 `wy(T)` 即 `open` 浏览器。）

浏览器重定向到 `http://localhost:8976/oauth/callback?code=...&state=...` 被 shared callback server 吃掉，做 PKCE 换 token，token 存进 `secretStorage`。

## `amp mcp oauth` 三件套

```
amp mcp oauth login <server-name> --server-url <url> --client-id <id> [--auth-url <url>] [--token-url <url>] [--client-secret <s>] [--scopes <csv>]
amp mcp oauth logout <server-name>
amp mcp oauth status <server-name>
```

`logout`：`i.clearAll(name)` — 清 tokens + client info。

`status`：

```js
let C = await s.getTokens(t);        // { accessToken, refreshToken, expiresAt }
let o = await s.getClientInfo(t);    // { clientId, authUrl, tokenUrl, ... }

// 打印
OAuth status for <name>:
  Client ID: <id>
  Auth URL: <url> or (discovered dynamically)
  Token URL: <url> or (discovered dynamically)
  Access Token: ✓ Present / ✗ Missing
  Refresh Token: ✓ Present / ✗ Missing
  Expires in: <minutes> minutes  or  ✗ Expired
```

## `Sl` class — OAuth credential storage

反编译里看到 `new Sl(secretStorage)` 被反复用。`Sl` 是 OAuth credential helper，封装了：

| 方法 | 用途 |
|---|---|
| `saveClientInfo(name, info)` | 写 discovery + client registration 结果 |
| `getClientInfo(name)` | 读回 client info（刷新 token 时用）|
| `saveTokens(name, tokens)` | 写 access / refresh token |
| `getTokens(name)` | 读 token，带 expiry 检查 |
| `clearAll(name)` | logout |

实际存储后端是 `secretStorage` —— 在 CLI 里就是 keytar / libsecret / Keychain 取决于平台（`T(h)` 解析出来的 `secretStorage`）。

## 错误分类（auth-failed）

从 `vCT()` 函数（错误规范化）：

```js
// from strings.txt:62345
let e = "server-error";
if (r.includes("timeout") || r.includes("Timeout")) e = "timeout";
else if (r.includes("OAuth") ||
         r.includes("authorization") ||
         r.includes("Unauthorized") ||
         r.includes("401"))
  e = "auth-failed";
else if (r.includes("fetch failed") ||
         r.includes("network") ||
         r.includes("ECONNREFUSED") ||
         r.includes("ECONNRESET") || ...)
  e = "network";
```

错误信息会附带补救提示：

```
If this is due to stale OAuth credentials, clear them and retry:
  amp mcp oauth logout <name>

Try registering OAuth credentials manually:
  amp mcp oauth login <name> --server-url <url> --client-id <id>
Required: --server-url, --client-id
Optional: --auth-url, --token-url (auto-discovered if not provided), --client-secret, --scopes

If this server doesn't support OAuth, add authentication headers to your config.
If it does support OAuth, ensure you've registered with:
  amp mcp oauth login <server-name> --server-url <url> --client-id <id> --auth-url <url> --token-url <url>
```

## 对 Alva 的启发

`alva-protocol-mcp` 当前**完全没有 OAuth**。要接 Sourcegraph / HuggingFace / Monday 这类 server 几乎必须上。最小实现路径：

1. **新建 `crates/alva-protocol-mcp/src/oauth.rs`**
   - `OauthCredentialStore` trait：`save_client_info / get_client_info / save_tokens / get_tokens / clear_all`
   - 实现：`FileOauthStore`（写到 `~/.config/alva/oauth/<server>.json`，0600 perm）；后续做 Keychain 实现（macOS `security-framework` / Linux `secret-service`）。
2. **Shared callback server**
   - 单例 `OauthCallbackServer::new()`，`start()` 监听 127.0.0.1 上固定端口（建议选 `17891` 之类，避免跟 Amp `8976` 冲突）。
   - `register_pending(state, tx)`：auth 开始时注册 `state → oneshot::Sender`；callback handler 按 `state` 匹配分发。
   - 端口被占时给和 Amp 一样的诊断文案。
3. **在 `McpClient::connect()` 前置**
   - 判断 `McpServerConfig` 是 `Url + no headers`（= OAuth path）；查 `OauthCredentialStore::get_tokens()`；expired 就 refresh；没 token 就 raise `McpError::OauthRequired { auth_url }`，让 extension 层决定弹浏览器。
4. **CLI / Tauri 三件套**
   - `alva mcp oauth login` 用 RFC 8414 discovery（`reqwest` 打 `/.well-known/oauth-authorization-server`）
   - `alva mcp oauth logout` / `status`
5. **`McpErrorCode` 枚举**（不光字符串匹配）
   ```rust
   pub enum McpErrorCode {
       Timeout,
       AuthFailed,
       Network,
       ServerError,
   }
   ```
   Amp 用字符串匹配 `includes("401") / includes("OAuth")` 是 JS 惯用法但很脆，Rust 应该依赖 reqwest 的 StatusCode 精确分类。

**不要**把 OAuth 耦合进 `McpClient::connect()` —— 应该保持 `McpClient` 是纯协议层，OAuth 预处理走独立 `OauthGuard` service，Connect 之前先过 guard。这样 Alva 以后做 API-key / mTLS / 企业 SSO 都只是加 guard。
