# 横向支撑 · Go sandbox
> WASIp1 guest/host 的显式能力边界，不是原生 OS 进程沙箱。

## 地位

共享纯 ABI 位于 `sdk/sandbox/abi`；本目录的 `guest` 复用 SDK 循环，`host` 为 native wazero 执行器，不依赖 harness、具体 provider 或凭据发现。

## 逻辑

每 job 新 runtime、实例、目录句柄与能力预算；宿主通过版本化有界 exchange/read 下发 context 和模型结果。
guest 的文件调用走 WASI，网络及 unsandboxed escalation 只能走独立宿主闸门；完整 IO/audit 交外层 observer，绝不自动送主 AI。

## 约束

- 不兼容旧 Rust ptr/len ABI，不自动识别旧版本。
- 不继承环境、stdio、文件系统或代理；目录使用 os.Root 约束，不使用会逃逸的默认 DirMount。
- 取消只有实际 guest 调用和同步宿主 callback 退出后才能返回；callback 必须响应 ctx，不用 goroutine 伪造终止。
- deadline/memory/IO 为实际限额；Core V2显式instruction fuel已有计量/实际耗尽回归，QuickJS-NG使用独立fresh core WASM，沿用同job授权/燃料/整体linear-memory预算，普通script错误可恢复，不宣称 Rust 全量对等。

## 业务域清单

| 目录 | 职责 |
|---|---|
| `host` | native runner、Unix/Windows目录能力（Windows原生未验收）、逐跳 fetch、升级授权与观察；job冷编译预算与QuickJS独立执行期限分开 |
| `guest` | WASIp1 import proxy、真 SDK agent 与 WASI 文件工具 |
