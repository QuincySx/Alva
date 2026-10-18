# 横向支撑 · MCP Protocol
> Native stdio MCP 客户端及连续 owned I/O worker。

## 地位

协议不拥有启动授权、日志持久化/脱敏或模型提示策略，由产品/Plugin 消费。

## 逻辑

显式启动 → initialize/initialized → 分页目录/调用。stdout/stderr 持续观察，完整帧 observer
同步背压，失败立即关闭自有进程。工具目录与结果由上层引用投影，主 AI 不自动加载。

## 约束

1 MiB 帧、32 MiB/8192 工具/128 页目录、128 中间消息、取消和超时 fail-closed；不重放。
只声明已实现能力；服务器请求只支持 ping，其余明确拒绝，不代执行 sampling/elicitation。
stdio 有宿主权限不是安全沙箱，必须外部批准。未实现 HTTP/SSE，不以配置模型冒充支持。
stderr按原始字节完整行/EOF partial有界framing（1MiB/行），跨Read/UTF8不拆安全边界；
超限prefix明确truncated并停worker。脱敏政策在product journal。Close即使owner取消已发生，
也保留迟到observer失败但不echo内部原因；CleanupComplete单独报告worker/group/reap是否真正完成。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Contract | `mcp.go` | 配置、IO observer、工具与 transport contract |
| Stdio | `stdio.go` | 持续 I/O、握手、JSON-RPC、目录预算与 owned cleanup |
| Regression | `stdio_test.go` | 本地子进程 fixtures、通知/日志/取消/预算 |
