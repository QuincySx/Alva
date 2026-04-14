# alva-host-wasm

> wasm32 host装配层 for the alva agent kernel — the wasm-side counterpart of `alva-host-native`.

## 地位
`alva-host-wasm` 是 L4 装配层的 wasm 目标变体。它和 `alva-host-native` 共享同一个 `alva-kernel-core` 内核，差别只在宿主提供的运行时原语（`Sleeper` impl）、默认的 middleware 栈、以及暴露给外部代码的 facade。本 crate 是 Phase 5 的交付物之一，证明 "kernel 平台无关" 这条断言在实践上成立。

## 逻辑
1. `sleeper::WasmSleeper` 提供 `alva_kernel_abi::Sleeper` 的 wasm 实现。核心是 `spawn_local + tokio::sync::oneshot` 桥接 non-Send 的 `gloo_timers` future——让外层 async fn 捕获的只是 `Receiver<()>`（Send），而 timer 本身被推进 `spawn_local` 的单线程任务里。
2. `agent::WasmAgent` 是最小消费 facade，打包 `AgentState + AgentConfig + run_agent`。构造时自动按 `cfg(target_family = "wasm")` 选 `WasmSleeper`（wasm）或 `NoopSleeper`（native 测试路径），装进 `ToolTimeoutMiddleware`。
3. `agent::WasmAgent::run_simple` 包装 `run` + 事件 channel + `MessageEnd` 收集，返回一个 `Result<String>`——wasm 最常见"跑一句拿到一句"的用例一行解决。
4. `smoke::_wasm_smoke_probe` 是**编译期探针**：永远不执行，只为 `cargo check --target wasm32` 真的 type-check 整条 kernel API 表面（`run_agent`、`LanguageModel`、`Tool`、`AgentState`、`AgentConfig`、`MiddlewareStack`……），防止未来某个 kernel commit 偷渡 wasm-incompatible dep 进来。

## 约束
- **跨层依赖方向**：只能依赖 `alva-kernel-abi` + `alva-kernel-core`，**不得依赖** `alva-host-native` 或 `alva-agent-*` box。host-wasm 必须可以和 host-native 互不感知地共存。
- **wasm 专用 dep（`gloo-timers` / `wasm-bindgen-futures`）必须 `[target.'cfg(target_family = "wasm")'.dependencies]` 守卫**，否则 native 编译会拉进不需要的 crate。
- **所有 wasm 专用 module 必须 `#[cfg(target_family = "wasm")]` 守卫**。`agent::WasmAgent` 和 `smoke::_wasm_smoke_probe` 在两个目标都编，因为它们不依赖 wasm-only API，只是让 native 测试也能跑同一套代码。
- 新增的 consumer API 必须同时有一个 native 测试覆盖——wasm32 只跑 `cargo check`，不跑 `cargo test`，所以测试覆盖走的是 native path（同一份代码）。
- **`alva-host-wasm` 不提供 `alva-agent-tools`**。`alva-agent-tools` 在当前状态下无法编译 wasm32（约 20 个 cfg-gating 相关 error），是独立的清理任务。`WasmAgent::new` 的 `tools` 参数因此默认为空 Vec，调用方自己按需传入 wasm-friendly 的工具。

## 业务域清单
| 名称 | 文件 | 职责 |
|------|------|------|
| Crate Root | `src/lib.rs` | 声明子模块 + cfg-gated `pub use WasmSleeper` |
| WasmSleeper | `src/sleeper.rs` | `Sleeper` wasm impl via `spawn_local + oneshot` 桥接 |
| WasmAgent | `src/agent.rs` | 最小消费 facade：`new` / `run` / `run_simple` / `state` / `config_mut` |
| Smoke Probe | `src/smoke.rs` | 编译期 dead-code 探针：让 `cargo check --target wasm32` 穿透整条 kernel API |
| Cargo 配置 | `Cargo.toml` | `alva-kernel-abi` + `alva-kernel-core` + `async-trait` + `futures-core` + `tokio (sync)`；wasm-only target deps: `gloo-timers` + `wasm-bindgen-futures` |
