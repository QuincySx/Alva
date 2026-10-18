# 横向支撑 · External Integration
> 适配外部模型、协议和 Agent 引擎，不属于可移植 SDK。

## 地位

按“外部系统边界”归类，而非强行定义一个统一 L 层。纯 codec、原生协议客户端、轻 engine 根契约与依赖完整 harness 的 adapter 继续分包，不能互相冒充。

## 逻辑

LLM 路径分开自有 native ProtocolAdapter、Magpie 跨协议转换与 HTTP transport。协议只拥有 wire/连接生命周期，Plugin/产品拥有授权与耐久存储。Engine 根契约是 L4.5；Alva adapter 消费完整 harness，process adapter 用 Vendor 分派 Claude/Codex/Pi 的共享原生实现，两者均非 SDK。

## 约束

SDK 不直接或传递依赖本目录。Magpie IR 仅封装在 llm/codec/internal/magpie，DTO 不外泄至 SDK。外部工具/进程/模型仅按显式配置调用，不发现用户凭据、现有账户/浏览器或旧配置。Hub core 只消费 engine 根契约，不反向导入具体 adapter。

## 业务域清单

| 目录 | 定位 | 职责 |
|---|---|---|
| `llm/` | 模型协议边界 | 纯 codec + 明确 HTTP/auth 的 provider，无产品会话执行器 |
| `protocol/` | 外部交互协议 | Skill/MCP/ACP/AEP/CDP 的独立语义、传输和有界 I/O |
| `engine/` | L4.5 契约 + 产品桥接 | 根包只定义 EngineRuntime；alva/process 子包实现完整 harness/外部 Agent 适配 |
