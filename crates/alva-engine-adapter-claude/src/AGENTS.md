# alva-engine-adapter-claude
> Claude Agent SDK 的 EngineRuntime 适配器，通过 Node.js bridge 子进程与 SDK 交互。

## 地位
作为 alva-engine-runtime 的具体实现之一，本模块通过进程间通信桥接 Anthropic 官方 Claude Agent SDK（Node.js），使上层消费者可以通过统一的 `EngineRuntime` trait 调用 Claude Code 引擎。属于"多引擎架构"中的外部引擎适配层。

## 逻辑
1. `ClaudeAdapter::new()` 接收 `ClaudeAdapterConfig`（含 API key、model、权限模式、云厂商开关等），构建适配器实例。
2. `execute()` 调用 `ensure_bridge_script()` 将编译时内嵌的 bridge JS 脚本写入用户缓存目录（幂等），然后把 `RuntimeRequest` 中的 `working_directory` / `system_prompt` / `streaming` / `max_turns` / `resume_session` 组装进 `BridgeConfig` JSON，并通过 `BridgeProcess::spawn()` 启动 Node.js 子进程。
3. `BridgeProcess` 将 stdin 包装为可 clone 的 `BridgeControl`，由独立 task 转发带 oneshot ack 的 cancel/permission controls；write/flush 与 ack 都有有限时，失败会通知 stdout owner 统一 Error → kill/cleanup → Completed。只有实际 flush 成功才向调用方返回成功。stdout loop 同时观察 event receiver 关闭，首事件前消费者放弃也会立即终止 child。stderr 单独监控，shutdown 时带 5 秒超时。
4. bridge 子进程输出 `BridgeMessage`（sdk_message / permission_request / done / error），由 `EventMapper` 有状态映射为 `RuntimeEvent` 流。
5. 权限请求流程：bridge 发出 `permission_request` -> 适配器发出 `RuntimeEvent::PermissionRequest` -> 上层调用 `respond_permission()` -> 通过 channel 发送 `BridgeOutbound::PermissionResponse` 写入子进程 stdin。
6. bridge 按显式 `sdk_package_path` → request cwd `node_modules` → process cwd → `npm root -g` 解析 SDK；显式 override 与任何已 resolve 的 ambient SDK import 均 fail-closed，绝不静默回退到另一版本。支持 Bedrock / Vertex / Azure 云厂商切换，并通过 `maxTurns` / `resume` 消费 runtime request。

## 约束
- 依赖宿主机安装 Node.js（默认路径 "node"，可通过 `node_path` 自定义）。
- bridge 脚本通过 `include_str!` 编译时内嵌，仅在内容变化时重写磁盘文件。
- `sdk_package_path` 是 SDK package 的 ESM specifier/path override，绝不是 Claude Code executable path；解析失败信息只列尝试来源，不得包含认证信息。
- SDK-load 错误只能使用稳定来源标签（不能输出 raw specifier、绝对路径或 Node 原始错误）；Windows 通过固定 `cmd.exe /d /s /c "npm root -g"` 查询 global root，两平台均有限时。
- 显式 filesystem SDK path（包括 Windows drive path）必须先以 request cwd（否则 process cwd）解析，再经 `pathToFileURL` import；只有真实 URL 可直接 import。bridge 收到缺少 Result 的 `done` 必须报错后完成，绝不可表示成功。
- `BridgeMessage` 使用 `serde(tag = "type")` 反序列化，未知类型以 `Unknown` 静默忽略，保证前向兼容（cindy 同款：不认识的 SDK 帧 logger + drop）。
- `EventMapper` 遵守 `alva-engine-runtime` stream contract：`MessageDelta` 只转发 Text/Reasoning；`input_json_delta` 不得泄漏为 MessageDelta；fatal `Error` 后只接 `Completed`。
- cindy 对齐策略：`empty-response`（零 substance + 零 usage）发 fatal Error + Completed；空 thinking 不算 substance；`Completed.result` 优先 SDK result 否则累计助手正文。
- **双层 idle watchdog**（cindy `upstream_response_idle_timeout`）：
  1. 进程内：默认注入 `CLAUDE_ENABLE_STREAM_WATCHDOG=true` + `API_TIMEOUT_MS=900000`（用户 env 优先不覆盖）
  2. 宿主：bridge stdout 在「等模型」时静默超过阈值 → fatal Error + kill + Completed；默认 30min，`ALVA_CLAUDE_BRIDGE_IDLE_TIMEOUT_MS` / `bridge_idle_timeout_ms` 覆盖，`0` 关闭；工具执行与 permission 等待不计时
- `PermissionMode` 决定工具执行审批策略：default（SDK 默认）/ acceptEdits / bypassPermissions。
- `resume` capability 为 true，表示适配器支持通过 session id 恢复历史会话。
- 返回的 Stream 为 `'static`，不借用 `&self`。
- 子进程 kill_on_drop 确保适配器析构时自动清理。
- 同一 native session ID 的 active route 不能被新 child 覆盖；closed sender 才可替换。完成/错误先 abort control 并按 sender identity 清理 route，再发送 `Completed`，因此完成事件后的 control 必须稳定得到 `SessionNotFound`。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| ClaudeAdapter | adapter.rs | EngineRuntime trait 实现，管理 session map、启动 bridge 进程、转发权限决策 |
| BridgeProcess / BridgeControl | process.rs | Node.js 双工生命周期：独立 stdin control 写入、stdout 逐行读取、session 结束清理 |
| bridge | bridge.rs | bridge 脚本部署：编译时内嵌 JS，运行时幂等写入缓存目录 |
| ClaudeAdapterConfig | config.rs | 适配器配置：Node 路径、API 认证、模型参数、权限模式、云厂商开关、沙箱、MCP 服务器等 |
| EventMapper | mapping.rs | 有状态事件映射器，将 BridgeMessage/SdkMessage 转换为 RuntimeEvent，维护工具名查找表 |
| BridgeMessage / SdkMessage | protocol.rs | bridge 通信协议类型定义：入站消息（sdk_message、permission_request、done、error）和出站消息 |
| lib.rs | lib.rs | 模块入口，重导出 ClaudeAdapter、ClaudeAdapterConfig、PermissionMode |
