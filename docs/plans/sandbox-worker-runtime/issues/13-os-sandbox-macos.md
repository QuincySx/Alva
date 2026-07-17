# 13 — OS 沙箱档 macOS 实装（--sandbox os）

**What to build:** 全流程 worker 的 macOS 写圈禁档：派活声明 os 档与授权目录，worker 进程在按 job 生成的 Seatbelt profile 下启动——全套工具含 shell 可用，授权外**写**由内核拒绝，读取则完全不受圈禁。它适配需要本机工具链（测试/构建/git）的重任务，目标是防止 worker 改坏授权外文件，**不能防止读取或带走宿主密钥**。Bypass / AcceptShell 只在本次进程已经通过真实 Seatbelt 写拒绝探针后才可启用。

> 2026-07-17 验收裁定：票 12 补测证明 cargo 兼容的唯一可用 profile 必须包含
> `(allow file-read*)`；deny-default 配手写读 allow-list 会 SIGABRT、worker 无法启动。
> 因此原验收项“授权外读写被内核拒绝”改为“授权外写被内核拒绝”。依据见
> [`../12-seatbelt-spike-recommendation.md`](../12-seatbelt-spike-recommendation.md) 的
> “2.3 补测”节。os 档与 wasm 档“授权外路径不存在”的保证不在同一量级。

**Blocked by:** 01 — HITL 洞修复（全工具 worker 的审批必须先不可绕）；12 — Seatbelt spike。

**Status:** ready-for-agent

- [ ] os 档派活端到端：worker 在圈禁下完成"改文件+跑测试"任务
- [ ] 授权外**写**被内核拒绝（非工具层校验），有测试钉住；读取不受圈禁并在 CLI 明示
- [ ] 子进程（shell 里再起的命令）继承圈禁
- [ ] 与 jobs/审批/日志的组合语义与 wasm 档一致（同一套用户面）
