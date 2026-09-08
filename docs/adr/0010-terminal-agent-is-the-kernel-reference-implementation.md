# ADR-0010: 终端 agent 是内核的参考实现（Rule 18 的真实理由）

- Status: Accepted
- Date: 2026-09-09
- Scope: alva-app-cli, alva-app-tauri, alva-engine-runtime, scripts/ci-check-deps.sh (Rule 18)

## Context

CI Rule 18 规定：原生产品入口（`alva-app-cli` / `alva-app-tauri`）不得（传递）
依赖任何 `alva-engine-*`。这条规则的理由写在 `ci-check-deps.sh` 的注释里：

> EngineRuntime 只服务 Hub 等多引擎产品；CLI/Tauri 继续直连 BaseAgent，
> 避免 multi-engine 合约变更同时打穿三端。

那是一个**施工期理由**——它描述的是写下这条规则那一天的合并风险。施工期
过去之后，这条注释就成了把规则当技术债删掉的邀请函：有人会说"合并风险早
过了，让 CLI 也走 EngineRuntime 统一抽象吧"。

本 ADR 记录真实理由，防止那一天。

## Decision

**终端 agent（alva-app-cli）是内核唯一一个必须自己跑通的消费者，这个角色
不可转让给 EngineRuntime。** CLI 不改走 `EngineRuntime`。

完整的理由：

1. **内核需要至少一个不能糊弄的用户。** 内核的每一处改动——循环、工具批、
   会话契约、事件流——都要先在一个真实产品上活体验证。Hub 走
   EngineRuntime 中间层，任何内核回归都可以被适配层的容错吞掉或变形；
   只有终端 agent 与内核朝夕相处、零隔离。它是内核的"活体测试"。

2. **EngineRuntime 是多引擎产品的合约，不是内核的替代面。** 引擎合约服务
   的是"多引擎路由/回退/权限协商"这类 Hub 需求。如果终端 agent 也改走
   EngineRuntime，内核将失去最后一个"必须自己跑通"的消费者：此后内核的
   每次演化都只需让某个适配器编译通过，内核与真实使用的距离会不可逆地
   拉大。

3. **一个参考实现足以约束内核，两个则互相迁就。** 内核不需要被所有产品
   直接消费才健康；它需要被**至少一个**不能糊弄的产品直接消费才健康。
   Hub 想要抽象，终端想要真实——两个都要，内核才不会漂。

## Consequences

### Positive

- 内核演化有持续的活体验证：CLI 的每次发布都是内核的端到端验收。
- Rule 18 获得一个不会过期的理由；删掉它必须先推翻本 ADR，而不是顺手清理。
- 终端 agent 保持零引擎层依赖，发布面最小。

### Negative

- CLI 无法零成本接入多引擎回退——那是有意的：回退是编排产品能力，不是
  内核机制。
- 引擎合约的变更不再"天然"惠及终端 agent；如果终端确实需要某个引擎的
  能力，正确做法是内核侧补齐机制，而不是给 CLI 接引擎层。
