# alva-app-cli
> Native CLI harness：provider 配置、headless/interactive UX 与可选 WASIp1 派活入口。

## 地位

本 crate 位于应用层，可依赖 app/host/sandbox；不得把 CLI 决策下沉到稳定 SDK。

## 逻辑

1. `main.rs` 解析 subcommand/flag，装配 provider 与普通 BaseAgent 路径。
2. `-p --sandbox wasm` 提前验证授权目录，只构造宿主 provider，不构造 native agent。
3. wasm host policy 在 blocking 线程执行 runner，import callback 回到原 Tokio handle 调模型。
4. TUI/REPL、session、jobs/providers/tools 子命令保持原路径。

## 约束

- API key 只保留在宿主 provider，不得写入 module bytes、WASI args/env/preopen 或结果。
- wasm runner 同步调用必须放入 `spawn_blocking`。
- worker 生产物按 sidecar 交付；开发期允许 target fallback 或 `ALVA_WORKER_WASM`。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| CLI 源码 | `src/` | 主入口、agent 装配、UI/subcommand 与 wasm host policy。 |
| golden 测试 | `tests/` | 真实二进制 argv/config/stdout/stderr 与 wasm E2E 契约。 |
