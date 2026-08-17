# Spec: Swappable Agent Loop Interface

> 来源：ADR-0007 的 follow-up research。本 spec 是「agent loop 接口化」第一步的可执行规格。
> 状态：ready-for-agent。

## Problem Statement

Alva 的 agent 循环当前是一个具体函数 `run_agent`，而非一个可替换的接口。这导致一个真实的产品限制：Hub 若想对齐 DeepSeek Harness（DSH）那样「替换 looper / 换一种驻留式 driver / 注入自己的循环策略」，没有干净的接入点——要么改 SDK 内核，要么在 harness 层重新包一层。

DSH 能做到这件事，是因为它把 loop 拆成「`Agent` 接口（零 loop 依赖）+ `AgentFactory`（换 loop 的入口）+ 具体实现 `ReactLoopAgent implements Agent`」。Alva 缺的就是这个「接口 + 默认实现」的分界。

## Solution

在 `alva-kernel-core` 引入一个**极简的、object-safe 的 loop 接口**（暂时只暴露「执行」这一个动作），让现有的 `run_agent` 成为它的默认实现。任何第三方（Hub、未来的驻留 driver、特殊循环策略）都能通过这个接口替换循环，而现有 3 个调用方（SDK `Agent`、harness `BaseAgent`、WASM facade）行为零变化。

**明确不做**：不引入 DSH 的 inbox 队列投喂、`status` 状态机、`whenIdle`/`runMaintenance`、事件否决 dispatch。那些是「驻留 driver」的更大决策，且部分与 Cordis 生命周期耦合，属于后续独立决策，不混进这一步。

## User Stories

1. 作为一个 Hub 产品开发者，我想要一个稳定的 loop 接口，以便我能在不 fork SDK 内核的前提下替换 / 包装 agent 循环来对齐 DSH 的能力。
2. 作为一个第三方 harness 搭建者，我想要一个明确的 loop 契约，以便我能实现自己的循环策略并注册进去。
3. 作为一个 SDK 内核维护者，我想要 `run_agent` 保持为默认实现，以便现有所有消费方不因接口化而改变行为。
4. 作为一个内核回归守护者，我想要 `run_agent` 的既有测试继续直接覆盖默认实现，以便接口化本身不削弱既有行为保障。
5. 作为一个 WASM 目标维护者，我想要 loop 接口在 wasm-clean 约束下成立，以便 CI 的 wasm32 防火墙不被破坏。
6. 作为一个希望渐进演进的开发者，我想要这个接口一开始只暴露 execute 一个动作，以便接口化不会被膨胀成 loop 模型的重写。
7. 作为一个希望未来引入驻留 driver 的人，我想要接口的 shape 与 `run_agent` 现有入参对齐（state/config/cancel/event channel），以便后续扩展队列投喂时不需要推翻接口底座。
8. 作为一个希望遵守边界规则的贡献者，我想要 loop 接口不违反 Rule 17（SDK 不依赖 app/host），以便 CI 依赖检查保持通过。

## Implementation Decisions

- **接口落点在 `alva-kernel-core`**：依赖方向已定（`alva-agent-core → alva-kernel-core → alva-kernel-abi`），且 kernel-core 已拥有 `AgentState` / `AgentConfig` / `AgentEvent` 这些接口入参类型。
- **接口 shape = `execute` 薄缝**：只暴露一个动作，签名形状对齐现有 `run_agent` 的入参（mutable state、immutable config、取消令牌、输入消息、事件通道），默认实现就是 `run_agent`。
- **object-safe**：接口必须能被 `dyn` 动态分发（对齐 `EngineRuntime` 已有的 object-safe 姿态），因为替换方需要通过 trait 对象注入。
- **`run_agent` 保留为 pub 默认实现**：不移除、不重命名，现有直接调用方与测试零改动。
- **不用新并行模型**：不引入 inbox 队列、状态机、事件否决这些 DSH 特产；`MiddlewareStack` 已是 Alva 等价的边界拦截机制。
- **不触碰 skill / AEP / worker-wasm**：loop 接口化与这三者正交，不混入本 spec。
- **接口命名与 home 归属在实现时定稿**：命名（如 `AgentLoop` / `RunLoop` / `Driver`）与是否有 factory 注册点，在实现阶段基于 object-safe 约束敲定，本 spec 只锁「一个 execute 缝」这个形状。

## Testing Decisions

- **好测试 = 只测外部行为**：断言「通过接口驱动一个 run 产生的 `AgentEvent` 序列，与直接调 `run_agent` 完全一致」，不测接口的内部实现细节。
- **被测模块**：`alva-kernel-core` 的 loop 接口 + 默认实现。
- **新增 seam = 一个（唯一的新 seam）**：loop 接口。现有测试（`alva-kernel-core/tests/integration.rs` 等大量直接 `run_agent(` 调用）继续作为「默认实现」的行为回归，不迁移。
- **先例**：`alva-engine-runtime` 的 `EngineRuntime` trait（object-safe trait + 多个 adapter 实现）是同一类「接口解耦」的既有范式；`alva-kernel-abi` 的 `AgentSession` / `LanguageModel` trait 也是 object-safe 契约的同类先例。
- **边界验证**：`scripts/ci-check-deps.sh` 必须保持全绿（Rule 17），`cargo check --target wasm32-unknown-unknown` 对 wasm-clean crate 集合不回归。

## Out of Scope

- DSH 的 inbox 队列投喂（`send`/`followup`/`steer`/`inject`）。
- DSH 的 `status` 状态机（idle ⇄ running）。
- `whenIdle` / `runMaintenance` 维护原语。
- 事件否决 dispatch（`MiddlewareStack` 已是等价物）。
- Hub plugin 化（这是 ADR-0007 的另一条线，本 spec 只覆盖 agent loop 接口这一步）。
- AEP 能力注入、worker-wasm 的 QuickJS `run_script`、skill 注入——均与本 spec 正交。

## Further Notes

- 本 spec 是 ADR-0007 「文档 → 调研 → 代码」三段的第三步（代码），且严格限定为「薄 execute 缝」这个最小、零行为变化的落点。若后续要引入驻留 driver / 队列投喂，那是**独立的** spec，不再复用本 spec 的范围。
- 实现时若发现「object-safe 的 execute 签名」与现有 `run_agent` 的 `&mut AgentState` 借用有冲突，须先回到 ADR-0007 记录该冲突与取舍，而不是静默放宽接口。
