# Alva Go · L0–L7 目标实现
> 稳定 SDK、可选插件与独立产品；目录按职责归类，不合并已有抽象。

## 地位

2026-10-16 用户授权的目标实现，2026-10-17 按层级与用户产品重排。单一 module 为 `github.com/QuincySx/Alva/go`；目录容器不另建 module、进程或兼容入口。Rust tree 仅作行为与验收参考，不是 Go 运行依赖，也不在本轮删除。

## 逻辑

**纵向读 L0 → L7，横向查 integration/platform。** L0 是纯值与协调原语，L1/L2 是通用内核，L2.3/L2.5/L3 是可独立消费的 Agent SDK，L4 是可选扩展，L5 是我们的默认 harness，L6/L7 才是面向用户的产品。具体路径、职责和阅读顺序见 [README.md](./README.md)。

`integration/engine` 根契约单独保留 L4.5；其 Alva/外部进程适配器是产品桥接而非 SDK。横向容器只是按“适配外部系统”或“提供平台实现”归类，不意味着整包都比 harness 更底层。

细分抽象默认保留。包或接口合并须先说明独立职责、真实消费者、收益和损失；没有明确结论就不合并。目录移动不是功能迁移完成或减弱依赖防火墙的理由。

## 约束

- `sdk` 可移植，不直接或传递依赖 integration、platform、extension、harness、apps、MyGo 或具体 Provider；kernel 不反向依赖 agent 语义。
- 扩展通过 Tool/Plugin/Middleware 提供；同名 Plugin 替换 harness 默认项，不增加 ad-hoc setter 或旧路径 alias。
- direct CLI/desktop 直连 harness 与执行 owner，不通过 EngineRuntime；Hub 通过 engine 根契约监管多个完整 Agent。
- `platform/host` 拥有 Agent 执行、审批/问题 rendezvous、command/browser 资源；`platform/system` 是进程/媒体/HTTP 等底层 OS 原语，两者不混成产品容器。
- 文件/媒体、大工具结果、MCP 与历史引用优先；未移植能力明确失败。UI 退订不是 Stop，只有执行 owner 取消并等待真实收尾。
- 无历史配置/数据/API 兼容桥、Rust FFI、双写或隐式凭据发现；不删除旧源码、依赖、缓存或用户历史。
- MyGo 固定版本、自绘，无 React/WebView/npm；demo 不冒充真实模型，live 必须显式配置。窗口出现不等于完整 OS/产品验收。
- Go 实现采用 AGPL-3.0-only，见 LICENSE/NOTICE；吸收的 Magpie IR 仅可由 integration/llm/codec 使用，Magpie 依赖已移除，第三方 DTO 不进入 SDK。
- 遵守根 FRACTAL-DOCS：源码三行头、逐级 AGENTS.md。完成/未完成状态独立见 [MIGRATION.md](./MIGRATION.md)，不以目录齐全或一次旧 checkpoint 宣称全量迁完。

## 业务域清单

| 目录 | 层级/定位 | 职责 |
|---|---|---|
| `sdk/` | L0–L3，可移植 SDK | wire/bus/沙箱 ABI、kernel 契约与循环、Agent 语义/装配/能力；第三方 harness 可直接使用 |
| `extension/` | L4，可选能力 | builtin、文件/记忆、MCP/Skill/AEP、子 Agent、浏览器/媒体/命令等真实 Plugin/Tool |
| `harness/` | L5，默认配方 | BaseAgent 本次配方冻结、默认按名替换、actual native run claim与引用/历史/Pending/原生子会话接线 |
| `apps/` | L6/L7，用户产品 | direct UI、独立模型 Gateway、Hub 产品及所有薄命令入口 |
| `integration/` | 横向外部适配 | LLM codec/provider、Skill/MCP/ACP/AEP/CDP 协议、L4.5 engine 根契约与具体产品适配器 |
| `platform/` | 横向宿主实现 | host 执行所有权、system OS 原语、storage 耐久权威、sandbox 宿主/guest |
| `architecture/` | 验证闸与消费者实验 | tagged import 防火墙、公开面快照、分形文档检查与反例；独立扩展性实验验证公开SDK的复用范围与限制 |
