# alva-sandbox-wasm

> Native-only WASIp1 宿主叶子 crate，以 job 级 preopen 能力运行任意 wasm 模块。

## 地位

`alva-sandbox-wasm` 位于宿主边界，不属于 wasm-clean guest 集，也不是 `alva-host-wasm`。本 crate 在 Ticket 04 阶段不接 agent loop，只提供无全局状态的模块执行边界。

## 逻辑

1. 调用者把 wasm 模块字节、guest 参数和宿主目录到 guest 挂载点的授权映射交给 `run_module`。
2. `run_module` 为本次调用新建 wasmtime engine、WASIp1 context、store 与 linker，以读写权限 preopen 每个授权目录。
3. guest 的 stdout/stderr 写入内存管道；正常返回与 WASI `proc_exit` 都转换为 `RunOutcome`，其余 trap 返回 `SandboxError`。
4. 集成测试按需把独立 fixture crate 编译到 `wasm32-wasip1`，只从公共 runner 边界观察输出和宿主文件系统效果。

## 约束

- production runner 不使用 static、进程级缓存或跨调用共享状态；授权随 `RunRequest` 生灭。
- 不继承宿主 stdio、环境、参数、网络或文件系统；只有显式 `Grant` 形成 guest 可见目录。
- 本 crate 是 native-only wasmtime 宿主，不得加入 `scripts/ci-check-deps.sh` 的 `WASM_CLEAN_CRATES`。
- Ticket 04 不接 kernel、agent、app 或 host crate，不实现 QuickJS、网络、LLM 代理或 worker 编排。

## Public Surface

- `Access`：授权目录的访问级别（`ReadOnly` / `ReadWrite`，默认 `ReadWrite`）；只读挂载禁止 guest 创建/删除/改写。
- `Grant`：一个宿主目录到 guest 挂载点的授权映射（含 `access`）；构造器 `read_write` / `read_only`。
- `RunRequest`：模块字节、授权列表与 guest 参数。
- `RunOutcome`：退出码及捕获的 stdout/stderr。
- `SandboxError`：模块加载、WASI 接线、授权挂载、执行与输出解码错误。
- `SandboxRunner`：持有共享 `Engine` 的可复用 runner；`run` 每次新建 Store（隔离），跨 job 复用编译缓存。
- `run_module`：一次性同步 WASIp1 模块 runner（内部新建一次性 `SandboxRunner`）。

## Dependency Policy

- 运行依赖仅限 wasmtime、wasmtime-wasi 与错误封装库，不依赖任何 Alva workspace crate。
- 测试可使用 `tempfile` 与本机 Rust 工具链按需构建 fixture。
- 文件访问必须保持在 WASI preopen 接口之后，不在 runner 中另写路径检查器。

## Module Map

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| runner 公共边界 | `src/lib.rs` | 配置 WASIp1、preopen 授权、执行 `_start` 并捕获结果。 |
| 缝 2 集成测试 | `tests/runner.rs` | 用真 wasmtime + 临时目录断言 CRUD 落盘、输出、退出码与越狱阻断。 |
| fixture guest | `tests/fixtures/fs-guest/` | 独立 wasip1 二进制 crate，执行文件操作和两类越狱尝试。 |
