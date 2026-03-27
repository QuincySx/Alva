# alva-engine-runtime
> 多引擎架构的统一运行时接口层，定义所有引擎适配器必须实现的 trait 和共享类型。

## 地位
本模块是多引擎架构的核心抽象层。上层消费者（如 alva-app-core）仅依赖此 crate 的 `EngineRuntime` trait 和类型定义，对具体引擎实现（alva-engine-adapter-alva、alva-engine-adapter-claude 等）保持无感知。所有适配器的输入（RuntimeRequest）、输出（RuntimeEvent）、错误（RuntimeError）和能力声明（RuntimeCapabilities）均在此统一定义。

## 逻辑
1. `EngineRuntime` trait 定义四个方法：`execute()` 返回事件流、`cancel()` 取消会话、`respond_permission()` 响应权限请求、`capabilities()` 查询引擎能力。
2. `RuntimeRequest` + `RuntimeOptions` 封装引擎无关的请求参数：prompt、resume_session、system_prompt、working_directory、streaming、max_turns 及 extra 透传字段。
3. `RuntimeEvent` 是带 `serde(tag = "event_type")` 的枚举，定义完整的事件生命周期：SessionStarted -> Message/MessageDelta/ToolStart/ToolEnd/PermissionRequest -> Completed。`Completed` 是唯一终端事件。
4. `RuntimeError` 用 thiserror 派生，覆盖 NotReady、SessionNotFound、PermissionNotFound、ProcessError、ProtocolError、Cancelled、Other 七种错误，并实现 `From<io::Error>` 和 `From<serde_json::Error>` 自动转换。
5. `RuntimeCapabilities` 声明引擎支持的特性集（streaming、tool_control、permission_callback、resume、cancel），供上层按能力分发。

## 约束
- `EngineRuntime` 必须是 object-safe（文件末尾有编译期断言 `_assert_object_safe`），支持 `dyn EngineRuntime` 动态分发。
- `execute()` 返回的 Stream 必须为 `Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>`，生命周期为 `'static`，不借用 `&self`。
- 适配器在错误时必须先发 `Error { recoverable: false }` 再发 `Completed { result: None }`，消费者应等待 `Completed` 后才做清理。
- `RuntimeEvent::Message.content` 不包含 ToolUse/ToolResult 块，这些被拆分为独立的 ToolStart/ToolEnd 事件。
- `RuntimeOptions.extra` 用于引擎特有配置透传，运行时层本身不解析。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| EngineRuntime | runtime.rs | 统一引擎 trait：execute / cancel / respond_permission / capabilities |
| RuntimeEvent | event.rs | 引擎输出事件枚举：SessionStarted、Message、MessageDelta、ToolStart、ToolEnd、PermissionRequest、Completed、Error |
| RuntimeUsage | event.rs | 引擎级使用统计：token 数、费用、时长、轮次 |
| RuntimeCapabilities | event.rs | 引擎能力声明：streaming、tool_control、permission_callback、resume、cancel |
| PermissionDecision | event.rs | 权限决策类型，用于 respond_permission 回传 |
| RuntimeRequest | request.rs | 引擎无关的请求参数：prompt、session 恢复、system prompt、工作目录、运行选项 |
| RuntimeOptions | request.rs | 运行选项：streaming、max_turns、extra 透传 |
| RuntimeError | error.rs | 统一错误类型：七种错误变体，含 io::Error 和 serde_json::Error 自动转换 |
| lib.rs | lib.rs | 模块入口，重导出所有公开类型 |
