# mcp
> MCP（Model Context Protocol）协议集成层

## 地位
提供 MCP Server 的生命周期管理、工具枚举/调用适配和配置文件读写，是 alva-app-core 与外部 MCP Server 交互的统一网关。

## 逻辑
`McpManager` 管理所有 MCP Server 的注册、连接、断开和工具调用。`McpToolAdapter` 将 MCP 工具包装为标准 `Tool` trait 实现。`McpRuntimeTool` 提供面向 Agent 的 meta-tool 操作接口。`McpConfig` 处理 `mcpServerConfig.json` 的读写。

## 约束
- 传输层抽象由 `skills::skill_ports::McpTransport` trait 定义
- MCP 工具名称格式：`mcp:<server_id>:<tool_name>`
- 连接超时由 McpServerConfig 配置

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| runtime | `runtime.rs` | McpManager：Server 注册/连接/断开/工具枚举/调用 |
| tools | `tools.rs` | McpRuntimeTool：面向 Agent 的 MCP meta-tool |
| config | `config.rs` | McpConfig：mcpServerConfig.json 读写、McpServerEntry/McpTransportEntry |
| tool_adapter | `tool_adapter.rs` | McpToolAdapter：单个 MCP 工具 -> Tool trait 适配、build_mcp_tools 批量转换 |
