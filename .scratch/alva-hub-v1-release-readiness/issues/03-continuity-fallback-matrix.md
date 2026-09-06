# 03 Continuity Failure and Fallback Matrix

## 目标

关闭 V1 acceptance scenario 3，并同时证明 fallback 的可见、有限、可逆属性。

## 场景矩阵

| Trigger | A | P | L 策略 |
|---|---|---|---|
| rate limit | classifier + state machine | injected terminal reason | 仅用可控测试账号/Provider；不可安全触发则记录限制 |
| auth failure | adapter mapping | acceptance-only invalid credential | 可撤销的测试凭证，不破坏用户配置 |
| missing binary | readiness + selection |隔离 PATH fixture | 临时 PATH，不移动或删除真实 CLI |
| process crash | adapter exit mapping | fixture process exit | 启动独立测试子进程，不 kill 用户进程 |
| context limit | provider/adapter mapping | injected bounded stream | 使用可控小窗口测试 route；不制造无界 token 消费 |
| watchdog stall | timer/state machine | deterministic stalled stream | 独立测试 runtime，不冻结真实 session |

## 共同断言

- 同一 Turn 内每个 EngineTarget 最多自动尝试一次；
- handoff 在 target 执行前持久化；
- fallback card 显示 reason/source/target，并可 Stop、View handoff、Switch back；
- chain exhausted 时展示所有失败原因；
- source branch 保持 parked，返回时优先 native resume，失败则原子升级为 full handoff；
- sensitive fields 不进入目标 adapter 或 evidence envelope。

## 验收

六种 trigger 均有 A + P；安全可复现者补 L。任何缺失的 L 必须作为外部限制单列，不影响对应 P 结论的真实性。

## 依赖

Ticket 01。

