# 02 — 升级通道：升级请求 = 工具调用走 PermissionMode

**What to build:** worker 遇到能力缺口（如需要跑测试/构建）时，发出一个"升级请求"工具调用，请求宿主在沙箱外执行指定命令。该请求作为普通工具调用进入现有 PermissionMode 审批流：Interactive 弹给用户逐条批、Auto 走分类器判安全性、Bypass 直接执行。执行结果（stdout/exit code）作为工具结果喂回 worker。不发明任何专用审批机制。

**Blocked by:** 01 — 修子 agent HITL 绕过洞（审批体系必须先不可绕，升级通道才有兜底意义）。

**Status:** ready-for-agent

- [ ] 升级请求工具注册进 worker 工具集，参数含命令与工作目录声明
- [ ] 三种 PermissionMode 下行为各一条 golden：Interactive 挂起等批（拒绝→任务带原因失败）、Auto 分类器放行安全命令/拦截危险命令、Bypass 直行
- [ ] 执行结果完整喂回 worker 上下文，worker 可基于失败输出继续迭代
- [ ] 脚本化 mock worker 端到端演示："改文件→升级请求跑测试→按结果修复"闭环
