# Alva

Alva 唯一受维护的 Go 实现，桌面使用 MyGo 自绘，不使用 React、WebView 或 npm。
SDK、direct harness 与 Hub 保留独立职责。技术栈已正式切换；能力与平台的独立验收边界见
[MIGRATION.md](./MIGRATION.md)，决策见 [ADR 0012](./docs/adr/0012-go-native-stack.md)。

## 先看地图：L0 → L7

**看分层从这里开始。** 下表的目录链接进入各层 AGENTS.md，那里写当前职责、接口/文件清单和依赖边界。
完整架构定义见 [ARCHITECTURE](./docs/ARCHITECTURE.md#分层)。L0–L7 是架构职责编号，
运行时通过具体接口与依赖关系组合能力；这些编号不表示请求执行的先后阶段。
例如 L1 的 [contracts.go](./sdk/kernel/abi/contracts.go) 定义接口，L2 的
[loop.go](./sdk/kernel/core/loop.go) 实现循环，文件前三行的 POS 也分别标明其层级职责。
实际依赖由 [架构防火墙](./tests/architecture/boundaries_test.go) 检查，不靠目录说明自行保证。

目录先分“SDK / 扩展 / 默认配方 / 用户产品”，再把外部集成与平台实现单独归类。
这是原有职责的重新摆放，不合并抽象、不增加旧路径别名，也不表示剩余功能已迁完。

```text
sdk/          L0–L3   第三方可直接使用的可移植 SDK
extension/    L4      可选 Tool / Plugin / Middleware
harness/      L5      我们的 BaseAgent 默认配方
apps/         L6/L7   direct 产品、Gateway、Hub 与共享呈现
cmd/          入口    CLI、桌面、HTTP、MCP 桥与 WASIp1 Worker
integration/  横向    外部模型 / 协议 / 引擎适配
platform/     横向    执行 owner / OS 原语 / 存储 / WASM 实现
tests/        验证    import 防火墙、公开面、分形闸与 SDK 消费者实验
tools/        手工    显式维护与本机验收工具
```

| 层级 | 实际目录 | 这一层回答什么问题 |
|---|---|---|
| L0 | [sdk/llm/wire](./sdk/llm/wire/AGENTS.md)、[sdk/kernel/bus](./sdk/kernel/bus/AGENTS.md)、[sdk/sandbox/abi](./sdk/sandbox/abi/AGENTS.md) | 消息/媒体/用量如何表达，能力如何协调，跨沙箱 ABI 是什么；没有 I/O 或授权政策 |
| L1 | [sdk/kernel/abi](./sdk/kernel/abi/AGENTS.md) | 循环只需要哪些模型、工具、会话与中间件契约 |
| L2 | [sdk/kernel/core](./sdk/kernel/core/AGENTS.md) | 如何运行循环、结算模型/工具事实、执行 hook 与处理取消 |
| L2.3 | [sdk/agent/abi](./sdk/agent/abi/AGENTS.md) | Task/Scope/Spawn/Phase 等 Agent 语义是什么；内核不反向依赖 |
| L2.5 | [sdk/agent/core](./sdk/agent/core/AGENTS.md) | 如何注册 Plugin、晚期接线、构建 SDK Agent |
| L3 | [sdk/agent 下的 context/memory/security/analytics/graph](./sdk/agent/AGENTS.md) | 可复用能力；graph 与 AgentLoop 是平行 runtime |
| L4 | [extension](./extension/AGENTS.md) | 哪些具体工具与可选行为可以安装；不是给内核加功能 |
| L4.5 | [integration/engine](./integration/engine/AGENTS.md) **仅根契约包** | Hub 如何统一驱动完整 Agent；不是 Model/Provider |
| L5 | [harness](./harness/AGENTS.md) | 默认Plugin、引用/历史/子会话政策；[可选持久等待](./harness/durable/AGENTS.md) |
| L6 | [apps/direct](./apps/direct/AGENTS.md)、[apps/direct/desktop](./apps/direct/desktop/AGENTS.md)、[apps/gateway](./apps/gateway/AGENTS.md)、direct 程序入口 | 用户直接聊天或显式调用模型 API；Gateway 不运行 Agent |
| L7 | [apps/hub](./apps/hub/AGENTS.md) 与 Hub 程序入口 | 多引擎会话、branch/Turn/Attempt、Team、自动化和产品服务/UI |

纯SDK默认不安装诊断保存政策；默认harness的telemetry Plugin可按名替换，诊断失败报告后继续，执行事实失败仍中断。可选durable Controller支持外部结果、审批和问题跨进程等待/继续；native适配复用原会话存储，尚未自动接入默认CLI/Hub交互。

### 程序入口：cmd 与 CLI

当前八个入口在 [cmd](./cmd/AGENTS.md)。cmd 表示可执行程序的启动代码，CLI 表示终端交互形态。
各目录都是独立 main 包；使用相应产品时才启动对应程序。

| 交互/运行形态 | 程序入口 |
|---|---|
| 终端 CLI | [alva](./cmd/alva/AGENTS.md)、[alva-hub](./cmd/alva-hub/AGENTS.md) |
| 桌面窗口 | [alva-desktop](./cmd/alva-desktop/AGENTS.md)、[alva-hub-desktop](./cmd/alva-hub-desktop/AGENTS.md) |
| HTTP 服务 | [alva-gateway](./cmd/alva-gateway/AGENTS.md)、[alva-hub-server](./cmd/alva-hub-server/AGENTS.md) |
| MCP 调试桥 | [alva-devtools-mcp](./cmd/alva-devtools-mcp/AGENTS.md) |
| WASIp1 沙箱 Worker | [alva-worker-wasm](./cmd/alva-worker-wasm/AGENTS.md) |

程序入口位于根 cmd；当前目录与边界见 [ARCHITECTURE](./docs/ARCHITECTURE.md#当前目录与入口)。

### 横向支撑与依赖边界

横向目录不是一条“全部在 SDK 之下”的严格层级：

| 横向目录 | 独立职责与边界 |
|---|---|
| `integration/llm/{codec,provider}` | native 薄适配/真实跨协议转换与唯一 HTTP/auth；L0 wire 仍在 SDK |
| `integration/protocol` | Skill/MCP/ACP/AEP/CDP；协议不决定产品授权或持久化政策 |
| `integration/engine/{alva,process}` | Alva 消费完整 L5 harness；process 按 Vendor 共享实现 Claude/Codex/Pi，不是三个独立 adapter 包，也不是 SDK |
| `platform/host` | Agent 执行/取消/订阅、审批与问题 broker、command/browser 资源所有权；可消费 harness Runner |
| `platform/system` | 更底层的 owned process、媒体派生/播放、loopback、严格 JSON 与可取消秘密输入 |
| `platform/storage` | 原始typed facts/catalog、[可插拔blob](./platform/storage/blob/AGENTS.md)（目录/临时内存）、私有SQLite与可选持久memory；不合并Hub事实 |
| `platform/sandbox` | native wazero 宿主与 WASIp1 SDK guest；共享纯 ABI 单独留在 `sdk/sandbox/abi` |

依赖方向由实际 import 防火墙验证：SDK 不能依赖 integration/platform/extension/harness/apps；
kernel 不能依赖 agent。上层可以显式消费下层或横向实现，不允许反向偷带产品政策。
direct CLI/desktop 直连 harness 与 host，不经 EngineRuntime；Hub core 只看 engine 根契约，
具体 Alva/外部适配器在 `apps/hub/bootstrap` 装配。共享呈现在 `apps/ui`，不依赖 direct/Hub 或执行 owner。

扩展可直接作为普通 Tool 安装，无需原生 Plugin、SQLite 或工作目录：

| 能力 | 普通工具入口 | 宿主提供 |
|---|---|---|
| 任务 | [`planning.NewTaskTools`](./extension/planning/AGENTS.md) | TaskService；详情引用 writer 可选 |
| 独立计划 | [`planning.NewPlanTool`](./extension/planning/AGENTS.md) | 一个计划写入函数；不要求任务服务或引用存储 |
| 人类问答 | [`interaction.NewTool`](./extension/interaction/AGENTS.md) | ToolConfig.Handler；引用后端可选 |
| 偏好/工具目录 | [`utility.NewConfigTool / NewSearchTool`](./extension/utility/AGENTS.md) | PreferenceStore 或 SearchConfig.Snapshot；Schema writer 可选 |
| 等待 | [`timer.NewTool`](./extension/utility/timer/AGENTS.md) | 无额外服务 |

这些入口直接使用 SDK `Builder.Tool`；偏好投影通过一个普通 `BeforeModel` hook 调用 `utility.ApplyPreferences`，安装工具不自动改变请求。原生 Hub 导入对应 `native/` 适配，复用相同工具实现并保留会话、Guard、审计、引用和持久化；同名默认 Plugin 替换规则不变。

通用入口没有引用后端时只返回完整且有界的结果（JSON 回执最多 16 KiB），超界明确失败，不截断冒充完整。
显式配置引用政策后保存失败不会退回全文。任务提交后的回执失败通过 `planning.CommittedError`
保留实际 call/task ID；已收到有效人类回答后的保存/呈现失败通过 `interaction.AnswerReceivedError`
保留 question ID。二者经现有内核错误路径留下恢复提示，调用方应查询事实而非自动重复操作。

一周后回来建议这样读：

1. 先看本页地图，再看 [根目录清单](./AGENTS.md)，确认改的是机制、能力、配方还是产品。
2. 改 SDK 从 [sdk/AGENTS.md](./sdk/AGENTS.md) 进入；工具/政策从 [extension](./extension/AGENTS.md) 或 [harness](./harness/AGENTS.md) 进入。
3. 改用户功能从 [apps](./apps/AGENTS.md) 选择 direct/Gateway/Hub；Hub 的事实、服务、装配、UI/HTTP 仍分开。
4. 涉及外部系统或资源生命周期，再读 [integration](./integration/AGENTS.md) / [platform](./platform/AGENTS.md) 对应子域，最后核对 [迁移状态](./MIGRATION.md)。

Go 实现采用 [GNU AGPL-3.0-only](./LICENSE) 开源，第三方声明见 [NOTICE](./NOTICE)。
协议转换吸收 yetone/magpie 固定 MIT 源码切片，不引入其完整应用或 new-api 服务。native协议直连薄适配，
跨协议才调用Magpie；有损工具/推理续传明确拒绝，不以EOF补成功。第三方 DTO 不进入 wire/kernel/Agent SDK。

## 快速开始

使用 .tool-versions 固定的 Go 工具链，在仓库根目录运行：

```sh
go run -mod=readonly ./cmd/alva --demo --prompt '你好，Alva'
go run -mod=readonly -tags desktop ./cmd/alva-desktop --demo
go run -mod=readonly -tags desktop ./cmd/alva-hub-desktop --demo --data-dir /absolute/private/alva
```

CLI demo 使用本地确定性模型/工具 fixture；桌面启动后等待显式 Send。
完整 live 配置、Gateway/Hub Server、调试、WASM、构建验证和数据生命周期见 [使用说明](./docs/USAGE.md)。
剩余平台/产品验收与本批结果见 [MIGRATION](./MIGRATION.md)。

Rust 完整快照保留在 Git 标签 archive/rust-final-2026-10-18；当前工作树仅维护 Go。
历史验收与初始结构审查保存在 [docs/history](./docs/history/AGENTS.md)，不作为新修改自动通过的证明。
