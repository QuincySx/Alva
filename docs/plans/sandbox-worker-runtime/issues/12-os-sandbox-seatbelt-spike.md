# 12 — OS 沙箱档 spike：Seatbelt profile 原型（调研票）

**What to build:** 调研票，产出物是证据与结论而非合入代码：验证 macOS Seatbelt（sandbox-exec / sandbox profile）能按 job 动态生成"只允许列出的路径读写 + 允许子进程"的 profile 并圈禁一个全工具 worker 进程。仓库里既有的 Seatbelt 死代码作为起点参考。结论要回答：profile 模板长什么样、动态路径注入怎么做、shell/git/cargo 在圈禁下是否可用、已知逃逸面有哪些——为 13 的实施手册铺路。

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] 可运行的 profile 生成原型：给定授权目录列表 → 生成 profile → 圈禁进程只见授权路径
- [ ] 实测：圈禁下 shell 可用、授权外读写被内核拒绝、子进程继承圈禁
- [ ] 已知限制与逃逸面清单（网络、临时目录、符号链接等）
- [ ] 给 13 的实施建议书（profile 模板 + 集成点分析），存 docs 或票内
