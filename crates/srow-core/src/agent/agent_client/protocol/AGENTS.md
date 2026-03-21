# agent/agent_client/protocol
> ACP（Agent Communication Protocol）消息协议定义

## 地位
定义 srow 与外部 Agent（Claude Code、Qwen Code 等）之间的双向消息格式，是 ACP 通信的协议层。

## 逻辑
入站消息 `AcpInboundMessage`（外部 Agent -> srow）包含 13 种事件类型，出站消息 `AcpOutboundMessage`（srow -> 外部 Agent）包含 5 种命令。所有消息通过 serde JSON 序列化/反序列化。

## 约束
- 使用 `#[serde(tag = "acp_event_type")]` / `#[serde(tag = "type")]` 标签联合枚举
- 所有字段 snake_case 命名

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块 |
| message | `message.rs` | AcpInboundMessage、AcpOutboundMessage |
| bootstrap | `bootstrap.rs` | BootstrapPayload、ModelConfig、SandboxLevel |
| permission | `permission.rs` | PermissionRequest、PermissionData、RiskLevel、PermissionOption |
| content | `content.rs` | ContentBlock（Text/ToolUse/ToolResult） |
| lifecycle | `lifecycle.rs` | TaskStartData、TaskCompleteData、TaskFinishReason、SystemMessageData、ErrorData |
| special | `special.rs` | PlanData、PlanStep、PingPongData |
| tool | `tool.rs` | PreToolUseData、PostToolUseData、ToolCallData |
