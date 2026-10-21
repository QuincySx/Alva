# Workflows
> Independent source validation gates.

## 地位
CI declarations only; no production execution or deployment.
## 逻辑

JS作者SDK使用Node内置test，在Windows/Linux/macOS均显式要求Node20+而不允许Go互操作测试静默全skip；Windows另执行direct CLI插件启停/跨进程等待恢复/取消收尾及实际JS/WASIp1插件/Hub装配与WASM资源限额，其余平台整仓race包含全部路径。Node只为显式JS插件/portable验收所需，不进入普通Go构建依赖。
普通 planning/interaction/utility/timer、telemetry、可插拔 durable 等待和纯内存 blob不仅跨编译：Node 与固定依赖 wazero 实际执行测试，独立 portable-agent 消费者的 plan/question/schema 三种模式在两类 WASM 中执行。native 消费者子进程由 extensibility 测试执行。
Graph的实际BSP、取消准入、checkpoint和数值预算回归也在Node/WASIp1运行，不只构建SDK。
Go runs Linux/macOS race（package -p1，避免metered SDK/CLI cold compile互相争夺真实wall-clock预算，包内concurrency/race不变）, vet, formatting, portable SDK/codec/planning/interaction/utility/timer/telemetry/durable/blob-memory and cgo-free cmd executables. A separate Windows job runs actual DACL/LockFileEx/atomic JobList/inherited stdin tests, diagnostic HTTP/private-file MCP stdio and SDK/architecture/native builds. The repository root is the module root; apps/hub, harness, integration and platform keep separate import gates. The retired Rust workflow is preserved only in the Git archive.
Linux/macOS完整job预算45分钟，每个Go测试包6分钟；不通过删减race、取消实机断言或跳过后续portable gates处理冷构建超时。
独立examples/sdk-agent module在native验证公开依赖图、正常/工具内取消；Node/WASIp1执行同一消费者。Windows job预算25分钟，实际执行共享storage conformance与外部module验收；声明不替代远程Windows结果。
## 约束
No shared/global app configuration, model calls, native UI interaction, credentials or installed daemons in fixtures. MyGo tests use headless Tester; compilation never proves real OS visual acceptance. Declaring the Windows job does not mean it has executed locally; Windows is cgo-free without race, while Linux/macOS keep full race gates. Windows read-only seekable media stdin/delete-on-close与实际WASI SDK文件回路也纳入native gates；Unix fd3 ABI不冒充Windows handle，交互Windows console和pinned shell仍明确拒绝。 SDK WASIp1/fuel/QuickJS regressions execute in the full suite. Windows声明实际QuickJS WASM绑定/资源回归，本机不冒充Windows已运行。 All test/vet/build commands use readonly module mode; seven native entrypoints include the independent Hub CLI and Devtools MCP.
## 业务域清单
| 文件 | 职责 |
|---|---|
| go.yml | Maintained Go source, SDK and native build gates |
