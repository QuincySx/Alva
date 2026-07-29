# alva-engine-adapter-claude
> Claude Code 的 EngineRuntime 适配器，**双模式**：Full（Agent SDK bridge）与 Light（`claude -p`）。

## 地位
作为 alva-engine-runtime 的具体实现之一，本模块把 Claude Code 接到统一 `EngineRuntime`。
提供两条对称路径：
- **Full**（`ClaudeAdapter`）— Node bridge + `@anthropic-ai/claude-agent-sdk`（cindy 完整 harness）
- **Light**（`ClaudePrintAdapter`）— `claude -p --output-format stream-json`（对标 `codex exec --json`）

Hub 通过 `--claude-mode light|full` 选择；两种模式共用 `EventMapper`（print 线格式 ≈ SDK 消息）。

## 逻辑

### Full（SDK）
1. `ClaudeAdapter::new()` 接收 `ClaudeAdapterConfig`，构建适配器实例。
2. `execute()` 部署 bridge JS，spawn Node 子进程，双工 stdin/stdout JSON-line。
3. `BridgeProcess` + control task：cancel/permission 与 stdout 读并行。
4. `EventMapper`：`BridgeMessage` → `RuntimeEvent`（含 idle watchdog）。
5. 权限：`permission_request` → `respond_permission` → stdin 回传。
6. SDK 解析：显式 path → cwd node_modules → process cwd → `npm root -g`，fail-closed。

### Light（CLI print）
1. `ClaudePrintAdapter` spawn `claude -p --output-format stream-json --include-partial-messages`。
2. 默认 `--bare` + `--permission-mode bypassPermissions`（Hub 无 TTY 一锤子）。
3. stdout JSONL 直接反序列化为 `SdkMessage`，经同一 `EventMapper` 产出 `RuntimeEvent`。
4. 无 interactive permission callback（`Unsupported`）；cancel 杀子进程。

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
| ClaudeAdapter | adapter.rs | **Full** EngineRuntime：SDK bridge、session map、权限转发、idle watchdog |
| ClaudePrintAdapter | print_adapter.rs | **Light** EngineRuntime：`claude -p` spawn + JSONL 映射 |
| ClaudeRuntimeMode | print_config.rs | full / light 选择器 |
| ClaudePrintConfig | print_config.rs | light 模式：binary 路径、permission、bare、cwd |
| ClaudePrintProcess | print_process.rs | `claude -p` 子进程生命周期 |
| BridgeProcess / BridgeControl | process.rs | Node.js 双工生命周期 |
| bridge | bridge.rs | bridge 脚本部署 |
| ClaudeAdapterConfig | config.rs | Full 模式配置 |
| EventMapper | mapping.rs | BridgeMessage/SdkMessage → RuntimeEvent（两模式共用） |
| BridgeMessage / SdkMessage | protocol.rs | 线格式类型 |
| lib.rs | lib.rs | 重导出 Full/Light 适配器与配置 |
