# L0–L2 · Go Kernel
> 不依赖 Agent 产品语义的稳定循环与契约。

## 地位

SDK 下层，不能直接或传递依赖 sdk/agent、harness、platform、apps、integration 或具体 Provider。

## 逻辑

`abi` 定义循环所需最小词汇，`core` 执行，`bus` 提供独立能力注册原语。

## 约束

可选功能留给 Plugin/Middleware；工具通过明确ConcurrentSafe及资源claim进入有界无冲突wave，保持模型顺序结算。
当前能力与平台验收边界见各子目录和 `../../MIGRATION.md`。
Bus的实际能力消费者、registry生命周期与尚未采用的SDK公开面，见bus/AGENTS.md；不能以存在声明或测试替代生产调用证据。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| ABI | `abi/` | 最小模型/工具/会话、调用上下文、事件、版本化记录与可复用sessiontest后端验收 |
| Bus | `bus/` | typed能力与读写句柄分离、显式独立bounded Topic；callback测试验证锁外复制 |
| Core | `core/` | 可替换loop、invocation绑定/准入/continuation、意图/结果结算、实际log值身份安全与有界事件通道 |
