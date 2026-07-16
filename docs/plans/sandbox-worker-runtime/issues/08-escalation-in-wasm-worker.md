# 08 — 升级通道接入 wasm worker

**What to build:** 把 02 的升级请求工具接进 wasm 档 worker 的工具集：沙箱 worker 干到一半需要跑测试/构建时举手，请求经宿主进入 PermissionMode 审批，批准后宿主在沙箱外执行、结果喂回沙箱内的 worker 继续干。wasm 档由此从"只能干纯文件活"变成"文件活自己干、重活举手申请"，全流程重构任务无需 OS 沙箱即可闭环。

**Blocked by:** 02 — 升级通道走 PermissionMode；05 — CLI 接通 wasm 档。

**Status:** ready-for-agent

- [ ] 升级请求跨 wasm 边界：guest 发起 → 宿主审批 → 宿主执行 → 结果回到 guest 上下文
- [ ] Auto 模式端到端 golden：沙箱 worker 改文件→举手跑测试→按失败迭代→任务成功
- [ ] Interactive 模式下升级请求挂起等批，拒绝后 worker 收到明确拒绝并能优雅收尾
- [ ] 升级执行的命令与输出进入 job 日志（审计可见）
