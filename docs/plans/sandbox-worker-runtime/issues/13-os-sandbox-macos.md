# 13 — OS 沙箱档 macOS 实装（--sandbox os）

**What to build:** 全流程 worker 的隔离档：派活声明 os 档与授权目录，worker 进程在按 job 生成的 Seatbelt profile 圈禁下启动——全套工具含 shell 可用，但文件系统只见授权路径，由内核强制。适配需要本机工具链（测试/构建/git）的重任务。审批语义不变：Bypass（信任直干）模式的前提正是这层沙箱兜底。

**Blocked by:** 01 — HITL 洞修复（全工具 worker 的审批必须先不可绕）；12 — Seatbelt spike。

**Status:** ready-for-agent

- [ ] os 档派活端到端：worker 在圈禁下完成"改文件+跑测试"任务
- [ ] 授权外读写被内核拒绝（非工具层校验），有测试钉住
- [ ] 子进程（shell 里再起的命令）继承圈禁
- [ ] 与 jobs/审批/日志的组合语义与 wasm 档一致（同一套用户面）
