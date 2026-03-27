# ACP Protocol Messages
> ACP (Agent Communication Protocol) 的纯数据类型层，定义 Host 与外部 Agent 之间所有 JSON 消息的 serde 结构体

## 地位
alva-protocol-acp crate 的核心子模块。仅依赖 serde/serde_json，不包含任何业务逻辑或 I/O，是协议的 single source of truth。上层的 transport、runner 等模块全部依赖此处的类型定义进行序列化/反序列化。

## 逻辑
协议分为三个阶段：
1. **Bootstrap** — Agent 进程启动后，Host 通过 stdin 写入一行 JSON (`BootstrapPayload`)，包含 workspace 路径、模型配置、沙箱级别等初始化参数。
2. **Inbound (Agent -> Host)** — Agent 通过 stdout 发送 13 种事件 (`AcpInboundMessage`)：会话/消息更新、权限请求、任务生命周期、工具调用通知、执行计划、心跳等。
3. **Outbound (Host -> Agent)** — Host 通过 stdin 发送 5 种命令 (`AcpOutboundMessage`)：用户 Prompt、权限响应、取消、关机、心跳 Pong。

所有消息均使用 `serde(tag = ...)` 进行内部标签化，以 JSON Lines 格式传输。

## 约束
- 纯数据层：禁止引入 serde/serde_json 以外的依赖
- 所有结构体必须派生 `Debug, Clone, Serialize, Deserialize`
- 字段命名使用 `snake_case`（通过 `rename_all`）
- 新增消息类型必须同时更新 `AcpInboundMessage` 或 `AcpOutboundMessage` 枚举
- `ContentBlock` 的 `is_delta` 字段默认 false，流式场景设为 true

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | mod.rs | 模块入口，re-export 所有子模块 |
| bootstrap | bootstrap.rs | 启动载荷：SandboxLevel、ModelConfig、BootstrapPayload |
| content | content.rs | 内容块枚举：Text（支持 delta）、ToolUse、ToolResult |
| lifecycle | lifecycle.rs | 任务生命周期：TaskStart/Complete、SystemMessage、Finish、Error |
| message | message.rs | 顶层信封：AcpInboundMessage (13 种)、AcpOutboundMessage (5 种) |
| permission | permission.rs | 权限模型：PermissionRequest、RiskLevel、四选项 HITL 响应 |
| special | special.rs | 特殊消息：PlanData（执行计划步骤）、PingPongData（心跳） |
| tool | tool.rs | 工具执行通知：PreToolUseData、PostToolUseData、ToolCallData |
