# L0–L3 · Portable SDK
> 第三方可直接构建 harness 的稳定 SDK，不携带我们的产品配方或 OS 实现。

## 地位

独立于 extension、harness、apps、integration 与 platform。L0–L3 是概念层级而非额外进程；各原有 package 与细分职责完整保留。

## 逻辑

按 L0 wire/bus/sandbox ABI → L1 kernel/abi → L2 kernel/core → L2.3 agent/abi → L2.5 agent/core → L3 Agent 能力库的顺序阅读。这是职责导航，不是每包必经的完整依赖链：agent/graph 是与 AgentLoop 平行的状态图 runtime，不并入 loop；能力的实际调用由显式 Plugin/consumer 决定。

## 约束

仅标准库与本 SDK 的允许方向；不加载凭据、配置、MyGo、具体 Provider、Magpie、SQLite 或宿主进程。kernel 不依赖 agent；wire 与纯 ABI 不反向依赖执行层。边界由 architecture 解析实际 import 图持续验证，不用“同在一个 module”放松隔离。

## 业务域清单

| 目录 | 层级 | 职责 |
|---|---|---|
| `llm/` | L0 | 自有消息线格式、媒体/来源与 stream 累加；不含 provider/codec |
| `kernel/` | L0/L1/L2 | bus 原语、最小契约与可替换通用循环 |
| `agent/` | L2.3/L2.5/L3 | Agent 语义、Plugin 装配与 context/memory/security/graph/analytics 能力 |
| `sandbox/` | L0 | 有界版本化 guest/host 共享 ABI；实际执行在 platform/sandbox |
