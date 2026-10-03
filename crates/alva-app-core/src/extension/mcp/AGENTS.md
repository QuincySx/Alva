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
- `with_recorded_interactions` 可选接产品 observer/实际 native owner：强制单 gateway，将 stdio 放入 owned 后台 I/O worker（不是第二模型），连握手/目录页/空闲通知/stderr 的连续协议观察；四动作 gateway input/output 也保留，只回短日志引用。队列 1 项，caller 取消关闭 RPC/连接并记录 acceptance unknown，不重放；最后 handle/记录失败回收 transport。执行 context 不符 owner 在任何日志/宿主动作前拒绝，不能复制 root gateway 授予 child 父 journal。持久化、脱敏、日志/检索预算归产品，legacy caller 保持不变。
- optional `McpSpawnPolicy` 仅初始化由产品发布；AgentSpawnTool 先核对直接父实际授予的 `mcp_runtime` 和共享 SecurityGuard，再按实际 child AgentSession 创建自有 gateway/journal/worker。`bind_recorded_gateway` 仅接受 recorded progressive 显式 snapshot，无 ambient config；启动遵循父取消且消耗同一 child timeout，绑定失败不回退父 gateway，空授权不启动。每层新 gateway 可再次受限授予孙会话，不授予父日志或 root 工具。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| extension | `extension.rs` | 文件发现/显式 snapshot、可选单 gateway/server 提示、recorded stdio worker 与获准 child gateway 重绑、finalize 审批 |
| spawn | `spawn.rs` | McpSpawnPolicy/McpGatewayBinding：产品日志权威向 app spawn 提供 own-session gateway 的窄初始化能力 |
| runtime | `runtime.rs` | McpManager：生命周期、host-side catalog、有界 metadata page/单工具契约和调用 |
| tools | `tools.rs` | McpRuntimeTool：面向 Agent 的 MCP meta-tool |
| progressive tools | `progressive_tools.rs` | opt-in 单入口发现/describe/call、session-bound gateway journal/短引用；精确身份/输入预算/安全 guard 回归 |
| worker | `worker.rs` | owned 串行 I/O task，队列/取消未知状态/observer 失败/最后 handle 的回收，无第二 LLM/落盘政策 |
| config | `config.rs` | McpConfig：mcpServerConfig.json 读写、McpServerEntry/McpTransportEntry |
| tool_adapter | `tool_adapter.rs` | McpToolAdapter：单个 MCP 工具 -> Tool trait 适配、build_mcp_tools 批量转换 |
