# chat
> GPUI Entity 绑定层，将 alva-core 的异步 Agent 事件桥接到 GPUI 同步 UI 线程。

## 地位
位于 `alva-app` 应用层，是 alva-core Agent 与 GPUI 渲染框架之间的唯一桥梁。上游依赖 `alva-app-core`（Agent、AgentHooks、AgentMessage 等）和 `alva-kernel-abi`（Message、StreamEvent 等），下游被 views 层的 chat_panel 消费。

## 逻辑
1. `GpuiChat` 是核心 Entity，包装 alva-core 的 Agent，通过 `tokio::sync::mpsc` 将异步 agent 事件转发到 GPUI 的 `EventEmitter` 机制。
2. `GpuiChatConfig` 封装聊天配置（模型选择、工具集等），`GpuiChatEvent` 定义 UI 层可订阅的事件枚举。
3. `SharedRuntime` 提供 tokio 运行时的共享句柄，供 GPUI Entity 在同步上下文中派发异步任务。
4. `gpui_chat_state` 当前为空占位模块，聊天状态已内聚到 `GpuiChat` / alva-core 的 AgentState 中。

## 约束
- 所有 Agent 异步回调必须通过 channel 投递到 GPUI 线程，禁止在 UI 线程直接 await。
- 不要在此层添加业务逻辑；业务逻辑属于 `alva-app-core`，此层只做事件转发与状态映射。
- `gpui_chat_state.rs` 保留用于模块结构完整性，不应再向其中添加状态定义。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| GpuiChat Entity | gpui_chat.rs | 包装 Agent，桥接异步事件到 GPUI EventEmitter |
| Chat State (占位) | gpui_chat_state.rs | 模块结构占位，状态已迁移至 GpuiChat 内部 |
| Barrel 导出 | mod.rs | 聚合并 re-export GpuiChat、GpuiChatConfig、GpuiChatEvent、SharedRuntime |
