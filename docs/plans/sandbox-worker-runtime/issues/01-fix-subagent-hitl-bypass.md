# 01 — 修子 agent HITL 绕过洞

**What to build:** 子 agent（进程内 SubAgent 派活）的每一次工具调用都必须经过审批中间件。今天子 agent 以空 middleware 运行，HITL 被静默绕过——用户在 Interactive 模式下批准的只是主 agent 的行为，子 agent 干什么没人看见。修复后，任何深度的子 agent 在 Interactive 模式下的工具调用都会浮出为审批请求。

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] 子 agent 继承（或按策略收窄）父会话的审批 middleware，不存在空 middleware 路径
- [ ] CLI golden（recording-mock）：Interactive 模式下子 agent 的工具调用产生审批事件，拒绝后工具不执行
- [ ] Auto / Bypass 模式下子 agent 行为与主 agent 语义一致，各一条 golden
- [ ] workspace 测试全绿，无既有断言改动（行为保持型除外，需逐条说明）
