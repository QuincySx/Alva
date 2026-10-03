# App harness extensions
> 产品协议与可选 Tool/Plugin/Middleware，不扩展稳定 SDK 内核。

## 地位
L5 app-core 的 harness 装配；Plugin/Registrar 合约仍来自 agent-core。

## 逻辑
注册协议/工具/能力，在 finalize 接线最终目录。`agent_spawn` 创建独立子会话、
继承共享安全/资源锁，并在有产品 context policy 时为实际会话重新绑定引用策略。

## 约束
- 递归工具 schema、模板 inherit-all 与执行选择均取直接父 Agent 的实际授权工具，
  不重新开放 root catalog；默认模型同样继承实际父模型，而非 scope 的 root 模型。
  每层 `agent` 是新实例，保持 depth/cancellation/timeout。
- `SpawnContextPolicy` 仅初始化注册，跨 crate 发现；不是通用 child builder 或运行期
  bus 注册器。绑定只贡献自己的检索工具、请求中间件、首个 assembly hook 和短 prompt。
- 自有历史 reader 即使空 host-tool whitelist 也保留，但不得授予文件/Shell/MCP、
  父会话或全局历史权限。缺失 policy 维持既有行为；权限、锁、typed causal link 不变。
- 产品 policy 先于通信 assemble，middleware 再检查 provider 请求；通信 hook 是
  可信装配代码，不宣称隔离其对 ContextHandle 原记录的访问。

## 业务域清单
| 文件/目录 | 职责 |
|---|---|
| mod.rs | SDK contract 重导出与 app extension facade |
| agent_spawn.rs | 独立子会话、直接父工具授权、递归深度/预算/安全/锁与可选引用绑定 |
| spawn_context.rs | SpawnContextPolicy/Binding：产品引用策略的窄子会话继承口 |
| agent_templates.rs | 内置子 Agent 模板 |
| mcp/ | MCP 生命周期、兼容直接目录、渐进 gateway；可选 session-bound journal/owned I/O worker，日志政策由产品提供 |
| skills/ | Skills、模板服务与协议装配 |
| hooks/ | session/tool hooks phase handler |
| evaluation/ | graph-based 评估能力 |
| lsp/ | deprecated LSP scaffold |
| analytics.rs | 埋点 middleware |
| approval.rs | HITL 审批通道 |
| blackboard_comm.rs | 可选共享 board 通信 hooks |
| pending.rs | steering/follow-up 排队与注入 |
| permission.rs | permission mode 装配与控制 |
| provider_registry.rs | 显式 Provider registry 发布 |
| spawn_comm_registry.rs | 可选通信能力注册表 |
| tool_lock_registry.rs | 共享工具资源锁 middleware |
