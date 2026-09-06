# alva-engine-runtime
> 多引擎架构的统一运行时接口层，定义所有引擎适配器必须实现的 trait 和共享类型。

## 地位
本模块是多引擎架构的核心抽象层。上层产品消费者（当前为 `alva-hub-core`）仅依赖此 crate 的 `EngineRuntime` trait 和类型定义，对具体引擎实现保持无感知。所有适配器的输入（RuntimeRequest）、输出（RuntimeEvent）、错误（RuntimeError）和能力声明（RuntimeCapabilities）均在此统一定义。

## 逻辑
1. `EngineRuntime` trait 定义四个方法：`execute()` 返回事件流、`cancel()` 取消会话、`respond_permission()` 响应权限请求、`capabilities()` 查询引擎能力。
2. `RuntimeRequest` + `RuntimeOptions` 封装引擎无关的请求参数：prompt、resume_session、system_prompt、working_directory、streaming、max_turns 及 extra 透传字段；适配器必须消费自己支持的字段，对不支持的能力显式返回 `RuntimeError::Unsupported`，而不是静默忽略。
3. `RuntimeEvent` 是带 `serde(tag = "event_type")` 的枚举，定义完整的事件生命周期：SessionStarted -> Message/MessageDelta/ToolStart/ToolEnd/PermissionRequest -> Completed。`Completed` 是唯一终端事件。
4. `RuntimeError` 用 thiserror 派生，覆盖 NotReady、Unsupported、SessionNotFound、PermissionNotFound、ProcessError、ProtocolError、Cancelled、Other 八种错误，并实现 `From<io::Error>` 和 `From<serde_json::Error>` 自动转换。
5. `RuntimeCapabilities` 声明引擎支持的特性集（streaming、tool_control、permission_callback、resume、cancel），供上层按能力分发。

## 约束
- `EngineRuntime` 必须是 object-safe（文件末尾有编译期断言 `_assert_object_safe`），支持 `dyn EngineRuntime` 动态分发。
- `execute()` 返回的 Stream 必须为 `Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>`，生命周期为 `'static`，不借用 `&self`。
- **新增 `impl EngineRuntime`（含内部 adapter 扩展）必须附带 conformance harness 实例化**（`feature = "test-support"` 的 `conformance.rs`），五个剧本一个不少：`Normal` / `SpawnFailure` / `KilledMidRun` / `EofWithoutResult` / `OutputAfterFatal`——契约断言的位置必须在 `execute()` 边界上（漏发 Completed、fatal 后继续发事件这类违约只发生在 select 循环里，mapper 级测试测不到）。
- **流契约**（`stream_contract.rs`）是多引擎共同底线：`Completed` 唯一且收尾；fatal `Error` 后**只能**再发 `Completed`（中间不得夹 Message/Tool）；`ToolStart`/`ToolEnd` 同 id 配对；`Message` 不含 Tool 块；`MessageDelta` 只允许 Text/Reasoning（工具参数 delta 不得泄漏）。
- `Error.recoverable` 对齐 cindy：`false` ≈ `isTerminal`，`true` ≈ `willRetry`/非终态横幅；UI 不得因 recoverable error 释放 running。
- 适配器在错误时必须先发 `Error { recoverable: false }` 再发 `Completed { result: None }`，消费者应等待 `Completed` 后才做清理（比 cindy「终态 error 可单独收尾」更严，保证单一 cleanup edge）。
- `Completed.result` 是最后一条助手正文（有则填），禁止占位字符串如 `"completed"`；fatal 路径 result 应为 `None`（soft warning 否则）。
- 未知 vendor 帧：适配器静默丢弃（`[]`）+ 日志，禁止 panic（cindy forward-compat）。
- `RuntimeOptions.extra` 用于引擎特有配置透传，运行时层本身不解析。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| EngineRuntime | runtime.rs | 统一引擎 trait：execute / cancel / respond_permission / capabilities |
| RuntimeEvent | event.rs | 引擎输出事件枚举：SessionStarted、Message、MessageDelta、ToolStart、ToolEnd、PermissionRequest、Completed、Error |
| StreamContract | stream_contract.rs | 多引擎流不变量检查器 + `validate_event_stream` |
| conformance | conformance.rs | 五剧本一致性 harness（feature = `test-support`）：Normal / SpawnFailure / KilledMidRun / EofWithoutResult / OutputAfterFatal |
| RuntimeUsage | event.rs | 引擎级使用统计：token 数、费用、时长、轮次 |
| RuntimeCapabilities | event.rs | 引擎能力声明：streaming、tool_control、permission_callback、resume、cancel |
| PermissionDecision | event.rs | 权限决策类型，用于 respond_permission 回传 |
| RuntimeRequest | request.rs | 引擎无关的请求参数：prompt、session 恢复、system prompt、工作目录、运行选项 |
| RuntimeOptions | request.rs | 运行选项：streaming、max_turns、extra 透传 |
| RuntimeError | error.rs | 统一错误类型：八种错误变体，含 Unsupported 显式能力拒绝，以及 io::Error / serde_json::Error 自动转换 |
| lib.rs | lib.rs | 模块入口，重导出所有公开类型 |
