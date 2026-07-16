# alva-worker-wasm guest source

> WASIp1 agent worker 的文件入口、SDK 装配与 blocking LLM/fetch 代理 ABI。

## 地位

本目录承载 WASIp1 worker：真 agent loop、本地文件工具与有界 QuickJS 脚本在 guest 内运行，宿主只代理模型 stream 并执行最终资源兜底。

## 逻辑

`main.rs` 在 WASI 下解析 host 注入的 workspace/task/result/grant args，用 `AgentBuilder` 装配 `ProxyModel`、`CorePlugin` 与 `RunScriptTool`；`run_script.rs` 为每次调用新建 QuickJS runtime，以同步 `WasiFs` 和 fetch 绑定完成批量操作并把脚本错误返回模型。模型与 HTTP 请求分别以独立版本化 JSON import 往返，agent 最终文本写入指定文件或 stdout。

## 约束

- `alloc` 必须保持 C ABI 和未改名导出，供宿主从实例中查找。
- LLM/fetch import 模块名与函数名必须和 `alva-sandbox-wasm` 注册形状完全一致。
- 从宿主返回的 buffer 必须由 guest 重新接管并释放。
- 文件工具必须以 `--workspace` 值为根，由 WASI preopen 而不是 guest 自行路径过滤来执行圈禁。
- async loop 由 `futures::executor::block_on` 驱动，不得引入 tokio runtime 或 tokio time sleeper。
- QuickJS 必须保持无 loader/module/CommonJS 能力，文件 binding 只能调用 `WasiFs` adapter。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| WASIp1 command | `main.rs` | SDK agent loop、WASI 文件工具、args/result 协议、ProxyModel、`alloc` export 与 `llm_complete` import。 |
| HTTP guest proxy | `http_proxy.rs` | fetch DTO 编解码、host import 调用与可恢复错误信封。 |
| QuickJS tool | `run_script.rs` | `run_script` Tool、interrupt/heap 限制、同步 WasiFs/fetch bindings 与结果格式。 |
