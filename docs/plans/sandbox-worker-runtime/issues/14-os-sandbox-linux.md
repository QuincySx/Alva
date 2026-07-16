# 14 — OS 沙箱档 Linux（Landlock/bubblewrap）

**What to build:** 13 的 Linux 对位实现：同样的用户面（os 档 + 授权目录）、同样的语义（内核强制、shell 在边界内、子进程继承），底层换 Landlock（或 bubblewrap，spike 时定夺）。Linux CI 可直接执行这档的测试——它也是 CI 环境里唯一能自动化验证的 OS 沙箱实现。与既有 Linux 门禁（默认拒启 + 显式逃生门）的关系在此票里理顺。

**Blocked by:** 13 — OS 沙箱档 macOS 实装（沿用其接口与测试形状）。

**Status:** ready-for-agent

- [ ] Linux 下 os 档派活端到端，圈禁语义与 macOS 档一致
- [ ] cfg-gated 测试由 Linux CI 执行（沿用既有 cfg-gated golden 模式）
- [ ] Landlock 内核版本不足时响亮降级报错（不静默裸跑）
- [ ] 与 --dangerously-allow-unsandboxed 门禁的交互有明确定义并测试
