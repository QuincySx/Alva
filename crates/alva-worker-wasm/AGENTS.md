# alva-worker-wasm

> 在 WASIp1 guest 内运行 SDK agent loop，以阻塞式结构化 ABI 让宿主只代理 LLM。

## 地位

`alva-worker-wasm` 是 WASIp1 command agent worker，不是宿主 runner。它加入主 workspace，让 CI 与开发者能直接用 `-p alva-worker-wasm` 编译；非 WASI 目标只保留一个可编译的提示入口。

## 逻辑

1. guest 从 preopen 的 `/work/task.txt` 读取任务，并以 `futures::executor::block_on` 驱动 SDK `Agent`。
2. `CorePlugin` 的文件工具在 WASI 下使用 `WasiFs`，相对路径以 `/work` 为根并受宿主 preopen 圈禁。
3. `ProxyModel::stream()` 把 messages、工具定义和 `ModelConfig` 序列化后调用 `alva:host/llm::llm_complete(req_ptr, req_len)`。
4. 宿主收集真实模型的 `Vec<StreamEvent>`，调用 guest 导出的 `alloc` 写回线性内存；guest 原样重放事件。
5. agent 完成工具循环后，guest 把最后一条 assistant 文本写入 `/work/result.txt`。

## 约束

- 本 crate 不包含 provider、CLI 参数接线或结构化错误协议；这些属于宿主 / app 层。
- guest 不包含 API key、provider 配置或任何宿主 secret。
- ABI 当前使用未版本化 JSON，且只接受长度可装入 `i32` 的请求/响应；错误仍由 trap 表达。
- `NoopSleeper` 明确关闭工具墙钟超时，避免在 `block_on` 下引入 tokio runtime/time 依赖。
- 生产化前必须版本化 ABI，并定义结构化错误与资源上限。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| crate 配置 | `Cargo.toml` | 声明 SDK agent、WASI core tools、runtime-neutral futures 与 serde 依赖。 |
| guest 源码 | `src/` | 实现真 agent loop、ProxyModel、`/work` 文件流和阻塞式 ptr/len LLM import。 |
