# alva-worker-wasm guest source

> WASIp1 agent worker 的文件入口、SDK 装配与 core-wasm LLM 代理 ABI。

## 地位

本目录承载 Ticket 05(a) 的 WASIp1 worker：真 agent loop 与本地文件工具在 guest 内运行，宿主只代理模型 stream。

## 逻辑

`main.rs` 在 WASI 下读取任务，用 `AgentBuilder` 装配 `ProxyModel`、`CorePlugin` 和注入 `NoopSleeper` 的工具超时 middleware；模型请求以 JSON 通过阻塞式 import 往返 `Vec<StreamEvent>`，agent 最终文本落盘。在其他 target 下只输出平台提示以保持 workspace native 构建可用。

## 约束

- `alloc` 必须保持 C ABI 和未改名导出，供宿主从实例中查找。
- import 模块名与函数名必须和宿主测试接线完全一致。
- 从宿主返回的 buffer 必须由 guest 重新接管并释放。
- 文件工具必须以 `/work` 为 workspace，由 WASI preopen 而不是 guest 自行路径过滤来执行圈禁。
- async loop 由 `futures::executor::block_on` 驱动，不得引入 tokio runtime 或 tokio time sleeper。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| WASIp1 command | `main.rs` | SDK agent loop、WASI 文件工具、ProxyModel、文件输入输出、`alloc` export 与 `llm_complete` import。 |
