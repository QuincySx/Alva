# alva-mcp
> MCP (Model Context Protocol) 客户端库，管理多服务器连接和工具适配

## 地位
MCP 协议的独立客户端实现。负责与外部 MCP 服务器的连接管理、工具发现和调用，并通过 McpToolAdapter 将 MCP 工具适配为 alva-types 的 Tool trait，使其可被 agent 引擎直接使用。

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
  └─→ MCP tool → alva_types::Tool (trait object)

config::McpConfig
  └─→ 读写 mcpServerConfig.json
```

## 约束
- McpClient 管理多个 server 连接，每个 server 独立生命周期
- McpTransport 是 async trait，实现者需处理底层 I/O
- McpToolAdapter 将 MCP 的 JSON Schema 映射为 alva-types 的 Tool trait
- config 模块仅处理 JSON 文件的序列化/反序列化

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| 客户端 | `client.rs` | McpClient 多服务器生命周期管理（connect / disconnect / list / call） |
| 传输层 | `transport.rs` | McpTransport trait 及传输实现 |
| 工具适配 | `tool_adapter.rs` | McpToolAdapter 将 MCP 工具适配为 alva_types::Tool |
| 类型定义 | `types.rs` | McpServerConfig、McpServerState、McpToolInfo 等 |
| 配置 | `config.rs` | mcpServerConfig.json 读写 |
| 错误 | `error.rs` | MCP 相关错误类型 |
