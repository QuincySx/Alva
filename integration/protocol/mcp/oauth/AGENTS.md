# 横向支撑 · MCP Authorization
> 远程 MCP 的 OAuth 2.1 + PKCE 协议流程，不保存令牌、不打开浏览器。

## 地位

纯协议包：调用方提供 HTTP client、浏览器打开函数和存储。go-sdk 的 `oauthex`/`auth` 只用于受保护资源/授权服务器元数据发现与动态注册。

## 逻辑

未授权 initialize 拿到 401 质询 → RFC 9728 资源元数据（质询提示、路径后缀、根）→ RFC 8414/OIDC 授权服务器元数据 → 预注册 client ID 或 RFC 7591 动态注册（只登记本次 127.0.0.1 回调）→ 浏览器授权（PKCE S256、256 位 state、resource）→ loopback 单次回调 → iss 校验 → 换取令牌。刷新带 resource 并接受轮换；撤销按 RFC 7009 先撤 refresh 再撤 access。

## 约束

授权服务器所有端点必须 https，loopback 除外；不声明 S256 或没有元数据即拒绝。回调只绑 127.0.0.1、精确路径与 Host、常量时间比对 state、错误 state 超过 8 次中止。令牌响应限 64 KiB 且必须是 Bearer；Grant 的格式化输出不含令牌。不做 CIMD。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| Flow | `oauth.go` | Metadata/Client/Token/Grant、Discover、Register、Receiver、AuthorizationURL、Exchange、Refresh、Revoke、Authorize |
| Regression | `oauth_test.go` | 本地假授权服务器：全流程、篡改 state、iss 混淆、无 S256、外来回调地址、PKCE 校验、回调 Host 与端点 TLS 政策 |
