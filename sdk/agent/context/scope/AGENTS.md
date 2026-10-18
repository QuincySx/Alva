# L3 · sdk/agent/context/scope
> 纯 SDK spawn 生命周期与黑板协作；不执行子 Agent、不创建原生会话或扩大历史读取权。

## 地位
context 能力子域；只依赖 agent ABI、kernel、wire 和标准库。产品 SubAgent Tool 与 session factory 在上层。

## 逻辑
SpawnScope 管理唯一 scope/session identity、depth、角色、预算与 direct-parent 工具授权上限。
每次 SpawnChild 必须显式给出工具名列表；nil=无工具。父绑定 agent 工具不继承，递归须为 child 重建工具，
child不能重新授予自己没收到的祖先工具。Scope只声明宿主会话目标ID，实际宿主必须按该ID绑定并持久化。
root Config.SessionID可显式绑定实际已存在session，空值才生成；child始终生成新目标identity。
child timeout/max-iterations只能继承或缩小直接父预算，不能用显式正值重新扩大预算。
SessionTracker 记录有界父子树；它是观测关系，不是读取会话内容的授权能力。

## 约束
root/child持有的Tool值可能绑定会话；产品factory必须重绑，不能将父Reference/MCP日志能力复制成子权限。
完成 scope 不能再 spawn；context 取消不假称已执行子任务终止。通信是 opt-in SpawnCommunication，不藏于生命周期树。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| Lifecycle | `scope.go` | scope树、显式工具上限、depth与预算、session tracker |
| Board | `board.go` | 有界typed消息/profile、相关性、16KiB渲染、lag通知 |
| Board scoping | `board_registry.go` | scope隔离、无环父链接、真正reader-only父视图 |
| Hooks and comms | `board_hooks.go` | Bootstrap/assemble/after-turn与spawn Attach/完成callback |
| Regressions | `scope_test.go` | direct-parent工具边界、depth、只读视图、lag、实际hooks |
