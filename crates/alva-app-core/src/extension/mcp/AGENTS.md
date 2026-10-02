# mcp
> MCP（Model Context Protocol）协议集成层

## 地位
提供 MCP Server 的生命周期管理、工具枚举/调用适配和配置文件读写，是 alva-app-core 与外部 MCP Server 交互的统一网关。

## 逻辑
`McpManager` 管理所有 MCP Server 的注册、连接、断开和工具调用。`McpToolAdapter` 将 MCP 工具包装为标准 `Tool` trait 实现。`McpRuntimeTool` 提供面向 Agent 的 meta-tool 操作接口。`McpConfig` 处理 `mcpServerConfig.json` 的读写。

## 约束
- 传输层抽象由 `skills::skill_ports::McpTransport` trait 定义
- MCP 逻辑身份：`mcp:<server_id>:<tool_name>`；模型工具名复用协议层 `mcp_` + 240-bit SHA-256 alias（64 ASCII bytes），避免冒号/点/超长名称被 provider 拒绝，description 保留 server/tool。
- 模板及 agent spawn 的旧 exact logical name 统一转换成注册 alias；不展开 wildcard，不增加工具或授权。
- 连接超时由 McpServerConfig 配置
- `McpPlugin::from_servers` 接已授权 snapshot/cwd，不发现外部配置；native stdio 真连接，SSE/wasm 显式 unsupported。`try_finalize` 将全部 MCP 工具及 meta-tool 加入既有 SecurityGuard 的 HITL 集合，无 guard 时 fail-closed，不相信 server read-only 注解。
- 单 server 握手及发现受配置 timeout 限制，全部自动连接有 60 秒 assembly budget；已关闭 transport 不伪装 Connected，显式重新连接会清理旧工具。
- 握手与工具目录共用连接超时；目录失败也进入 Failed，不遗留 Connecting。取消关闭的 transport 不投影成 Connected 或列为可用工具。
- 完全相同 config 的重复注册保留现有连接/目录；配置真正改变才替换。取消 connect 不遗留 Connecting，关闭的 transport 在调用前拒绝，不以旧目录冒充可用。
- `with_progressive_discovery` 是显式 harness policy：只注册 `mcp_runtime` 和有界 server 索引，不 fan-out tool schemas；旧 caller 不变。search 默认 5/最多 20 条、query ≤200 字符、96 字符描述预览、稳定 offset/has_more；describe 只取一个方法，call 核对当前已连接目录精确身份和 object arguments，入口 input ≤256 KiB。所有 gateway action 保持既有 HITL；结果耐久投影由产品 ContextRetrievalPlugin 消费，不在协议层存第二份日志。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| extension | `extension.rs` | 文件发现/显式 snapshot、可选单 gateway/server 提示、stdio 接线与 finalize 审批 |
| runtime | `runtime.rs` | McpManager：生命周期、host-side catalog、有界 metadata page/单工具契约和调用 |
| tools | `tools.rs` | McpRuntimeTool：面向 Agent 的 MCP meta-tool |
| progressive tools | `progressive_tools.rs` | opt-in 单入口按需发现/describe/call；5000-tool native fixture、精确身份/输入预算/安全 guard 回归 |
| config | `config.rs` | McpConfig：mcpServerConfig.json 读写、McpServerEntry/McpTransportEntry |
| tool_adapter | `tool_adapter.rs` | McpToolAdapter：单个 MCP 工具 -> Tool trait 适配、build_mcp_tools 批量转换 |
