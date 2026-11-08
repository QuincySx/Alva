# 横向支撑 · MCP Protocol
> Native stdio 与 Streamable HTTP MCP 客户端及连续 owned I/O worker。

## 地位

协议不拥有启动授权、日志持久化/脱敏或模型提示策略，由产品/Plugin 消费。

## 逻辑

显式启动 → initialize/initialized → 分页目录/调用。stdout/stderr 持续观察，完整帧 observer
同步背压，失败立即关闭自有进程。工具目录与结果由上层引用投影，主 AI 不自动加载。

## 约束

1 MiB 帧、32 MiB/8192 工具/128 页目录、128 中间消息、取消和超时 fail-closed；不重放。
只声明已实现能力；服务器请求只支持 ping，其余明确拒绝，不代执行 sampling/elicitation。
stdio 有宿主权限不是安全沙箱，必须外部批准。Streamable HTTP：POST 串行请求，JSON 或有界 SSE 回复（1 MiB 帧/行、32 MiB 流、128 中间消息），会话与协议版本头，404 会话失效不重放，不跟随重定向、不读环境代理；Authorizer 只在 401/403 后重试一次，协议不保存凭据。旧版 HTTP+SSE 不支持，initialize 收到 400/404/405 明确报错。
stderr按原始字节完整行/EOF partial有界framing（1MiB/行），跨Read/UTF8不拆安全边界；
超限prefix明确truncated并停worker。脱敏政策在product journal。Close即使owner取消已发生，
也保留迟到observer失败但不echo内部原因；CleanupComplete单独报告worker/group/reap是否真正完成。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Contract | `mcp.go` | 配置（进程或 HTTP 二选一）、IO observer、工具与 transport contract |
| Streamable HTTP | `streamable.go` | Connect 分派、HTTPClient、Authorizer/AuthorizationError、legacy 识别与 DELETE 结束会话 |
| Catalog | `catalog.go` | 两种传输共用的分页工具目录预算 |
| HTTP regression | `streamable_test.go` | loopback 假服务器：JSON/SSE、会话/版本头、服务器 ping、失效不重放、legacy、授权重试、重定向与非法地址 |
| OAuth | `oauth/` | MCP 授权（OAuth 2.1 + PKCE）协议流程，不存储、不开浏览器 |
| Stdio | `stdio.go` | 持续 I/O、握手、JSON-RPC、目录预算与 owned cleanup |
| Regression | `stdio_test.go` | 本地子进程 fixtures、通知/日志/取消/预算 |
