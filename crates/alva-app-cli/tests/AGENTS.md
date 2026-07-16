# alva-app-cli/tests
> 真实 `alva` binary 的 headless/subcommand/wasm-tier golden 契约。

## 地位

集成测试目录；以 argv/env/config/files/stdout/stderr/exit code 观察应用层行为。

## 逻辑

各 golden 文件按用户入口分组；wasm suite 另启动 recording provider 并检查宿主 HTTP 与 guest 文件结果。

## 约束

- secret 断言必须覆盖 stdout、stderr、preopen 文件与 worker module bytes。
- 真 provider 测试必须 ignore/env-gated，不得进入默认 CI。
- worker artifact 名必须保留 `alva-worker-wasm.wasm` 连字符。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| Print golden | `print_mode_golden.rs` | 普通 `-p` contract。 |
| WASI golden | `wasm_sandbox_golden.rs` | wasm flags、recording HTTP、JSON/file E2E、真 provider smoke。 |
| 其他 golden | `*_golden.rs` | jobs/providers/tools/recursion contract。 |
