# REGISTRAR-RULES — Registrar 准入规则

> 由公开符号快照闸（`scripts/public-surface.sh`，CI 中的 public-surface step）执行，
> 不另写 lint。`alva-agent-core` 的公开符号快照就是这条规则的执行记录。

## 规则

`Registrar`（`alva-agent-core::extension::Registrar`）只接受**贡献**类方法：
把插件的能力**放进**装配结果的方法。

- 贡献类：`tool` / `tools` / `middleware` / `phase` / `phase_handler` /
  `provide` / `system_prompt` / `command`。
- **不再新增"读取"类方法。** 想在装配期读取运行期状态，走 `bus()` /
  `bus_writer()`——bus 本来就是跨层协调通道，读取能力已经存在。
- 现有三个只读方法（`workspace()` / `bus()` / `plugin_name()`，
  加上装配期可写的 `bus_writer()`）是**历史欠账，保留但不扩大**。

## 为什么

1. **读取方法是单向棘轮。** 每加一个读取方法，就把装配期上下文的一部分固化成
   Registrar 的公开契约；之后想收窄就是一次破坏性变更。贡献方法没有这个问题——
   装配结果的消费者是固定的（builder 自己）。
2. **读取已有正道。** 装配期可读的状态（已注册能力、workspace、事件）全部
   可以从 `bus` / `bus_writer` / `workspace()` 拿到。新增读取方法不是补能力，
   是在两个通道之间复制状态。
3. **快照闸执行成本为零。** `Registrar` 的每一个新公开方法都会出现在
   `alva-agent-core` 的符号快照 diff 里；review 时看见 `implfn Registrar::fn …`
   多了一行，就对照本文件问一句：这是贡献还是读取？

## 历史

这条规则是 2026-09 内核边界重构（ticket 01）定下的。起因是检视 Registrar 时
发现它的读取面（workspace/bus/plugin_name）没有任何调用方会因缺少更多读取方法
而受阻，而每个读取方法都会永久占据 SDK 公开表面。
