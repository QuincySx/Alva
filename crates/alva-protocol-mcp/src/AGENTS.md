# alva-protocol-mcp
> MCP (Model Context Protocol) 客户端库，管理多服务器连接和工具适配

## 地位
MCP 协议的独立客户端实现。负责与外部 MCP 服务器的连接管理、工具发现和调用，并通过 McpToolAdapter 将 MCP 工具适配为 alva-kernel-abi 的 Tool trait，使其可被 agent 引擎直接使用。

## 逻辑
```
McpClient
  ├─→ connect(server_config) → 建立传输连接（stdio / SSE）
  ├─→ list_tools(server_id) → Vec<McpToolInfo>
  ├─→ call_tool(server_id, tool_name, args) → ToolResult
  └─→ disconnect(server_id)

McpTransport trait
  ├─→ StdioTransport (子进程 stdin/stdout)
  └─→ SseTransport (HTTP SSE)

McpToolAdapter
  └─→ MCP tool → alva_kernel_abi::Tool (trait object)

config::McpConfigFile
  ├─→ from_str(json) → 解析 JSON（全平台含 wasm）
  ├─→ load(path) → 读文件（非 wasm）
  └─→ save(path) → 写文件（非 wasm）
```

## 约束
- McpClient 管理多个 server 连接，每个 server 独立生命周期
- McpTransport 是 async trait，实现者需处理底层 I/O
- McpToolAdapter 将 MCP 的 JSON Schema 映射为 alva-kernel-abi 的 Tool trait
- config 模块支持 wasm 平台（from_str 全平台可用，文件 I/O 用 cfg gate 隔离）

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| 客户端 | `client.rs` | McpClient 多服务器生命周期管理（connect / disconnect / list / call） |
| 传输层 | `transport.rs` | McpTransport trait 及传输实现 |
| 工具适配 | `tool_adapter.rs` | McpToolAdapter 将 MCP 工具适配为 alva_kernel_abi::Tool |
| 类型定义 | `types.rs` | McpServerConfig、McpServerState、McpToolInfo 等 |
| 配置 | `config.rs` | MCP 配置管理：from_str（全平台）+ load/save（非 wasm，cfg-gated） |
| 错误 | `error.rs` | MCP 相关错误类型 |
