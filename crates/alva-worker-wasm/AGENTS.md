# alva-worker-wasm

> 在 WASIp1 guest 内运行 SDK agent loop，以阻塞式结构化 ABI 让宿主只代理 LLM。

## 地位

`alva-worker-wasm` 是 WASIp1 command agent worker，不是宿主 runner。它加入主 workspace，让 CI 与开发者能直接用 `-p alva-worker-wasm` 编译；非 WASI 目标只保留一个可编译的提示入口。

## 逻辑

1. guest 从 WASI args 读取 workspace、任务、授权 guest path 与结果通道，并以 `futures::executor::block_on` 驱动 SDK `Agent`。
2. `CorePlugin` 的文件工具在 WASI 下使用 `WasiFs`，相对路径以注入的 workspace 为根并受宿主 preopen 圈禁。
3. `RunScriptTool` 在 fresh QuickJS runtime 执行无模块脚本；同步文件绑定复用 `WasiFs`，同步 `fetch` 经宿主 import 执行且策略错误可被 JS 捕获，QuickJS interrupt/heap limit 只终止当前脚本。
4. `ProxyModel::stream()` 用 `alva-llm-wire` 的版本化 DTO 序列化 messages、工具定义和 `ModelConfig`，再调用 `alva:host/llm::llm_complete(req_ptr, req_len)`。
5. 宿主收集真实模型的 `Vec<StreamEvent>`，调用 guest 导出的 `alloc` 写回线性内存；guest 校验版本/大小后原样重放事件。
6. fetch 请求用独立 `alva-sandbox-abi` DTO 调 `alva:host/http::fetch`；白名单永不进入 guest，和 LLM 授权互相独立。
7. observation middleware 对每个顶层 Tool 完成事件调用 `alva:host/log::append`，fetch proxy 也逐次上报；事件不含宿主时间戳或落盘路径。
8. agent 完成工具循环后，guest 把最后一条 assistant 文本写入参数指定文件或 stdout。

## 约束

- 本 crate 不包含 provider 或 CLI 参数接线；这些属于宿主 / app 层。
- guest 不包含 API key、provider 配置或任何宿主 secret。
- ABI version = 1，请求上限 4 MiB、响应上限 16 MiB；超限/版本不符会响亮失败。
- `NoopSleeper` 明确关闭工具墙钟超时，避免在 `block_on` 下引入 tokio runtime/time 依赖。
- `run_script` 自带 QuickJS interrupt/heap limit；宿主 `RunLimits` 是整个 worker 的兜底，不替代 tool 可读错误。
- `run_script` 不得注册 loader、`require`、Node API 或绕开 `WasiFs` 的文件入口。
- `fetch` 保持同步，不伪装 Promise；guest 只负责 wire 编解码，域名/重定向授权只能由宿主执行。
- log import 只上报有界能力事件，不得接收或推导宿主 job 路径。
- workspace、任务与结果路径不得回退为 guest 内写死路径。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| crate 配置 | `Cargo.toml` | 声明 SDK agent、WASI core tools、runtime-neutral futures 与 serde 依赖。 |
| guest 源码 | `src/` | 实现真 agent loop、ProxyModel、WASI args/result 通道及 LLM/fetch/log imports。 |
| 脚本契约 | `RUN-SCRIPT.md` | 固定无模块 QuickJS 边界、资源限制与全部文件/输出绑定签名。 |
