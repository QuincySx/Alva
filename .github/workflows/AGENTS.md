# Workflows
> Independent source validation gates.

## 地位
CI declarations only; no production execution or deployment.
## 逻辑
Go runs Linux/macOS race（package -p1，避免metered SDK/CLI cold compile互相争夺真实wall-clock预算，包内concurrency/race不变）, vet, formatting, portable SDK/codec and cgo-free cmd executables. A separate Windows job runs actual DACL/LockFileEx/atomic JobList/inherited stdin tests, diagnostic HTTP/private-file MCP stdio and SDK/architecture/native builds. The repository root is the module root; apps/hub, harness, integration and platform keep separate import gates. The retired Rust workflow is preserved only in the Git archive.
## 约束
No shared/global app configuration, model calls, native UI interaction, credentials or installed daemons in fixtures. MyGo tests use headless Tester; compilation never proves real OS visual acceptance. Declaring the Windows job does not mean it has executed locally; Windows is cgo-free without race, while Linux/macOS keep full race gates. Windows read-only seekable media stdin/delete-on-close与实际WASI SDK文件回路也纳入native gates；Unix fd3 ABI不冒充Windows handle，交互Windows console和pinned shell仍明确拒绝。 SDK WASIp1/fuel/QuickJS regressions execute in the full suite. Windows声明实际QuickJS WASM绑定/资源回归，本机不冒充Windows已运行。 All test/vet/build commands use readonly module mode; seven native entrypoints include the independent Hub CLI and Devtools MCP.
## 业务域清单
| 文件 | 职责 |
|---|---|
| go.yml | Maintained Go source, SDK and native build gates |
