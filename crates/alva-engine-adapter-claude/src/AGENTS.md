# alva-engine-adapter-claude
> Claude Agent SDK 的 EngineRuntime 适配器，通过 Node.js bridge 子进程与 SDK 交互。

## 地位
作为 alva-engine-runtime 的具体实现之一，本模块通过进程间通信桥接 Anthropic 官方 Claude Agent SDK（Node.js），使上层消费者可以通过统一的 `EngineRuntime` trait 调用 Claude Code 引擎。属于"多引擎架构"中的外部引擎适配层。

## 逻辑
1. `ClaudeAdapter::new()` 接收 `ClaudeAdapterConfig`（含 API key、model、权限模式、云厂商开关等），构建适配器实例。
2. `execute()` 调用 `ensure_bridge_script()` 将编译时内嵌的 bridge JS 脚本写入用户缓存目录（幂等），然后把 `RuntimeRequest` 中的 `working_directory` / `system_prompt` / `streaming` / `max_turns` / `resume_session` 组装进 `BridgeConfig` JSON，并通过 `BridgeProcess::spawn()` 启动 Node.js 子进程。
3. `BridgeProcess` 管理子进程生命周期：通过 stdin/stdout 以 JSON-line 协议通信，stderr 单独监控并记录日志，shutdown 时带 5 秒超时。
4. bridge 子进程输出 `BridgeMessage`（sdk_message / permission_request / done / error），由 `EventMapper` 有状态映射为 `RuntimeEvent` 流。
5. 权限请求流程：bridge 发出 `permission_request` -> 适配器发出 `RuntimeEvent::PermissionRequest` -> 上层调用 `respond_permission()` -> 通过 channel 发送 `BridgeOutbound::PermissionResponse` 写入子进程 stdin。
6. 支持 Bedrock / Vertex / Azure 云厂商切换，通过环境变量注入子进程；Claude SDK 侧通过 `maxTurns` 和 `resume` 选项消费 `max_turns` / `resume_session`。

## 约束
- 依赖宿主机安装 Node.js（默认路径 "node"，可通过 `node_path` 自定义）。
- bridge 脚本通过 `include_str!` 编译时内嵌，仅在内容变化时重写磁盘文件。
- `BridgeMessage` 使用 `serde(tag = "type")` 反序列化，未知类型以 `Unknown` 静默忽略，保证前向兼容。
- `PermissionMode` 决定工具执行审批策略：default（SDK 默认）/ acceptEdits / bypassPermissions。
- `resume` capability 为 true，表示适配器支持通过 session id 恢复历史会话。
- 返回的 Stream 为 `'static`，不借用 `&self`。
- 子进程 kill_on_drop 确保适配器析构时自动清理。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| ClaudeAdapter | adapter.rs | EngineRuntime trait 实现，管理 session map、启动 bridge 进程、转发权限决策 |
| BridgeProcess | process.rs | Node.js 子进程生命周期管理：spawn、stdin 写入、stdout 逐行读取、graceful shutdown |
| bridge | bridge.rs | bridge 脚本部署：编译时内嵌 JS，运行时幂等写入缓存目录 |
| ClaudeAdapterConfig | config.rs | 适配器配置：Node 路径、API 认证、模型参数、权限模式、云厂商开关、沙箱、MCP 服务器等 |
| EventMapper | mapping.rs | 有状态事件映射器，将 BridgeMessage/SdkMessage 转换为 RuntimeEvent，维护工具名查找表 |
| BridgeMessage / SdkMessage | protocol.rs | bridge 通信协议类型定义：入站消息（sdk_message、permission_request、done、error）和出站消息 |
| lib.rs | lib.rs | 模块入口，重导出 ClaudeAdapter、ClaudeAdapterConfig、PermissionMode |
