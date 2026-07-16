# alva-worker-wasm guest source

> WASIp1 agent worker 的文件入口、SDK 装配与 blocking LLM/fetch/log 代理 ABI。

## 地位

本目录承载 WASIp1 worker：真 agent loop、本地文件工具与有界 QuickJS 脚本在 guest 内运行，宿主只代理模型 stream 并执行最终资源兜底。

## 逻辑

`main.rs` 在 WASI 下解析 host 注入的 workspace/task/result/grant args，用 `AgentBuilder` 装配 `ProxyModel`、`CorePlugin`、`RequestEscalationTool`、audit middleware 与 `RunScriptTool`；`escalation_proxy.rs` 把 guest cwd + command 送到宿主并将执行/拒绝结果还原为 tool result；`run_script.rs` 为每次调用新建 QuickJS runtime。模型、HTTP、升级与 audit event 分别走独立版本化 JSON import，agent 最终文本写入指定文件或 stdout。

## 约束

- `alloc` 必须保持 C ABI 和未改名导出，供宿主从实例中查找。
- LLM/fetch import 模块名与函数名必须和 `alva-sandbox-wasm` 注册形状完全一致。
- audit log import 必须只上报能力事件，不得接收或推导宿主 job 路径。
- escalation import 不得携带 PermissionMode 或 host path；相对 cwd 只能以 guest workspace 为根，最终授权由宿主 grants 翻译决定。
- 从宿主返回的 buffer 必须由 guest 重新接管并释放。
- 文件工具必须以 `--workspace` 值为根，由 WASI preopen 而不是 guest 自行路径过滤来执行圈禁。
- async loop 由 `futures::executor::block_on` 驱动，不得引入 tokio runtime 或 tokio time sleeper。
- QuickJS 必须保持无 loader/module/CommonJS 能力，文件 binding 只能调用 `WasiFs` adapter。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| WASIp1 command | `main.rs` | SDK agent loop、WASI 文件工具、args/result 协议、ProxyModel、`alloc` export 与 `llm_complete` import。 |
| Host escalation guest proxy | `escalation_proxy.rs` | `EscalationExecutor` 的 ptr/len host-import 实现与可恢复拒绝映射。 |
| HTTP guest proxy | `http_proxy.rs` | fetch DTO 编解码、host import 调用与可恢复错误信封。 |
| Audit guest proxy | `job_log.rs` | Tool middleware 与 fetch 事件的有界 guest→host import。 |
| QuickJS tool | `run_script.rs` | `run_script` Tool、interrupt/heap 限制、同步 WasiFs/fetch bindings 与结果格式。 |
