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
实际依赖由 [架构防火墙](./architecture/boundaries_test.go) 检查，不靠目录说明自行保证。

目录先分“SDK / 扩展 / 默认配方 / 用户产品”，再把外部集成与平台实现单独归类。
这是原有职责的重新摆放，不合并抽象、不增加旧路径别名，也不表示剩余功能已迁完。

```text
sdk/          L0–L3   第三方可直接使用的可移植 SDK
extension/    L4      可选 Tool / Plugin / Middleware
harness/      L5      我们的 BaseAgent 默认配方
apps/         L6/L7   direct 产品、Gateway、Hub 与可执行程序入口
integration/  横向    外部模型 / 协议 / 引擎适配
platform/     横向    执行 owner / OS 原语 / 存储 / WASM 实现
architecture/ 验证    import 防火墙、公开面与分形文档闸
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
| L5 | [harness](./harness/AGENTS.md) | 我们如何选择默认 Plugin、引用策略、历史/Pending 和子会话配方 |
| L6 | [apps/direct](./apps/direct/AGENTS.md)、[apps/desktop](./apps/desktop/AGENTS.md)、[apps/gateway](./apps/gateway/AGENTS.md)、direct 程序入口 | 用户直接聊天或显式调用模型 API；Gateway 不运行 Agent |
| L7 | [apps/hub](./apps/hub/AGENTS.md) 与 Hub 程序入口 | 多引擎会话、branch/Turn/Attempt、Team、自动化和产品服务/UI |

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

程序入口已迁至根 cmd；其余结构整理的原始评估见 [结构审查](./docs/ARCHITECTURE.md#优先落地的结构)。

### 横向支撑与依赖边界

横向目录不是一条“全部在 SDK 之下”的严格层级：

| 横向目录 | 独立职责与边界 |
|---|---|
| `integration/llm/{codec,provider}` | native 薄适配/真实跨协议转换与唯一 HTTP/auth；L0 wire 仍在 SDK |
| `integration/protocol` | Skill/MCP/ACP/AEP/CDP；协议不决定产品授权或持久化政策 |
| `integration/engine/{alva,process}` | Alva 消费完整 L5 harness；process 按 Vendor 共享实现 Claude/Codex/Pi，不是三个独立 adapter 包，也不是 SDK |
| `platform/host` | Agent 执行/取消/订阅、审批与问题 broker、command/browser 资源所有权；可消费 harness Runner |
| `platform/system` | 更底层的 owned process、媒体派生/播放、loopback、严格 JSON 与可取消秘密输入 |
| `platform/storage` | 私有 SQLite、原始 typed records/artifacts、可选持久 memory；不合并 Hub 产品事实 |
| `platform/sandbox` | native wazero 宿主与 WASIp1 SDK guest；共享纯 ABI 单独留在 `sdk/sandbox/abi` |

依赖方向由实际 import 防火墙验证：SDK 不能依赖 integration/platform/extension/harness/apps；
kernel 不能依赖 agent。上层可以显式消费下层或横向实现，不允许反向偷带产品政策。
direct CLI/desktop 直连 harness 与 host，不经 EngineRuntime；Hub core 只看 engine 根契约，
具体 Alva/外部适配器在 `apps/hub/bootstrap` 装配。基础 desktop 不反向依赖 Hub。

扩展可按能力直接安装普通Tool：独立SDK使用`Tool(timer.NewTool())`（[`extension/utility/timer`](./extension/utility/timer/AGENTS.md)）即可等待，不需要SQLite或原生会话；Utility Plugin复用同一实现。原生planning可通过`Config.Service`替换业务后端，Tool/getter/bus使用同一实例；实际native执行审计和详情引用authority仍保留，外部后端的持久化由提供者负责。

一周后回来建议这样读：

1. 先看本页地图，再看 [根目录清单](./AGENTS.md)，确认改的是机制、能力、配方还是产品。
2. 改 SDK 从 [sdk/AGENTS.md](./sdk/AGENTS.md) 进入；工具/政策从 [extension](./extension/AGENTS.md) 或 [harness](./harness/AGENTS.md) 进入。
3. 改用户功能从 [apps](./apps/AGENTS.md) 选择 direct/Gateway/Hub；Hub 的事实、服务、装配、UI/HTTP 仍分开。
4. 涉及外部系统或资源生命周期，再读 [integration](./integration/AGENTS.md) / [platform](./platform/AGENTS.md) 对应子域，最后核对 [迁移状态](./MIGRATION.md)。

Go 实现采用 [GNU AGPL-3.0-only](./LICENSE) 开源，第三方声明见 [NOTICE](./NOTICE)。
协议转换吸收 yetone/magpie 固定 MIT 源码切片，不引入其完整应用或 new-api 服务。native协议直连薄适配，
跨协议才调用Magpie；有损工具/推理续传明确拒绝，不以EOF补成功。第三方 DTO 不进入 wire/kernel/Agent SDK。

## 运行

使用 Go 1.27.2；本目录的 `.tool-versions` 固定本地工具版本。go.mod 位于仓库根目录，module path 为 `github.com/QuincySx/Alva`；以下命令直接在仓库根目录执行。

```sh
go run ./cmd/alva --demo --prompt '你好，Alva'
go run -tags desktop ./cmd/alva-desktop --demo
go run -tags desktop ./cmd/alva-hub-desktop --demo --data-dir /absolute/private/alva-go
```

demo 是真实走模型接口 → echo 工具 → 模型接口的本地确定性 fixture，不调用模型账户。
桌面启动不发送请求；明确点击 Send 才执行，Enter 换行，Stop 等待运行方确认。
消息使用虚拟列表，阅读旧消息时不强制拉到底部，可以点击 Jump to latest。

live 必须显式指定完整 endpoint 和 model，不发现环境变量或旧配置。
可选 `--api-key-stdin` 从标准输入读取密钥，不能把密钥放进命令参数。

```sh
go run ./cmd/alva --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --api-key-stdin --prompt '你好'
go run -tags desktop ./cmd/alva-desktop --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --api-key-stdin
go run -tags desktop ./cmd/alva-hub-desktop --data-dir /absolute/private/alva-go --live --provider-kind openai_responses --endpoint https://example.invalid/v1/responses --model MODEL --api-key-stdin
```

示例地址是占位符，需替换为自己的服务；提供密钥后以 EOF 结束输入。live 会产生实际服务费用。
HTTPS 或 loopback HTTP 可用，不跟随重定向。Hub 的 `--provider-kind` 支持
`openai_chat`、`openai_responses`、`anthropic`、`gemini`，endpoint 为对应完整操作地址。
CLI 的显式 `--workspace` 可装配受管文件工具；默认非交互写授权失败关闭。
Hub 有真实审批 broker，项目文件写入与未知外部工具走 HITL。
MCP 仅接受本会话保存并独立确认启用的配置，调用时才启动选中服务；不继承现有全局配置或凭据。
MCP/AEP gateway绑定实际Tool、call与native会话；外部执行owner关闭时取消并等待审批/工具回调、事实写入和真实进程清理。Hub将MCP同样纳入attempt资源重试，清理未完成不释放会话存储。

直接 `alva-desktop` 不链接 EngineRuntime。无数据目录时保留独立内存演示；显式 `--data-dir /absolute/new-direct` 启用耐久 direct 会话列表、文件引用、逐调用审批和 typed Inspector。`--session ID` 只选现有会话，`--workspace PATH` 只绑定新会话项目；恢复保留原 workspace/route 指纹，四 provider 共用 canonical factory。选择、冷历史与 Inspector 都不自动执行。
`alva-hub-desktop` 是独立的耐久多引擎产品入口，有首页、会话/branch、附件、设置、
Skills 与只读 Inspector；未完成项仍见迁移矩阵，不以基础控件可用宣称 Hub V1 完成。

同源 headless 入口是 `alva-hub-server`，不嵌入旧 React 页面：

```sh
go run ./cmd/alva-hub-server --new-token
go run ./cmd/alva-hub-server --demo --data-dir /absolute/private/alva-go-server --secrets-stdin
```

第二条从 stdin pipe/显式重定向文件读取 `{"token":"第一条生成的token"}` 后 EOF；不支持不可取消的交互终端。
live 可在同一对象给可选 `api_key`，
并显式指定 `--live --provider-kind ... --endpoint ... --model ...`。token 不放参数、URL或启动输出。
默认只监听 `127.0.0.1` 的随机端口，输出实际地址；请求必须有 `Authorization: Bearer TOKEN`，
Host 与 Origin 必须是该精确本地地址。不接受公网/wildcard/DNS bind、跨站或query token。
API 路由与限制见 [server/AGENTS.md](./apps/hub/server/AGENTS.md)；SSE 使用 authenticated fetch，
按 committed Sequence/Last-Event-ID 续读。观察断线不会取消已认领Turn。

只读 Devtools MCP 可以连接上述已运行的 Hub HTTP，复用实际产品归属校验：

```sh
go run ./cmd/alva-devtools-mcp --hub http://127.0.0.1:PORT --token-fd 3 3</absolute/private/hub-token
```

`hub-token` 为单独、显式提供的原始 bearer 文件（不是 server 的 secrets JSON）；token 不进 argv、URL、环境或 MCP stdout。
stdin/stdout 专用于 MCP stdio。`hub_inspect` 只读会话小索引、历史小页、typed child Inspector 和显式正文范围，
不暴露设置/凭据、不恢复 Agent、不提供执行/写入动作。启动和断线都不重放会话。
详见 [Devtools 约束](./apps/hub/devtools/AGENTS.md)。

两个自绘桌面也可显式启用独立 UI 调试服务，默认关闭、默认只读：

```sh
go run -tags desktop ./cmd/alva-hub-desktop --demo --data-dir /absolute/private/alva-go --debug-listen 127.0.0.1:0 --startup-secrets-stdin </absolute/private/startup-secrets.json
go run ./cmd/alva-devtools-mcp --ui http://127.0.0.1:PORT --token-file /absolute/private/ui-token
```

startup JSON 为 `{"debug_token":"独立256-bit随机token"}`，live 可另带 `api_key`；与 raw `--api-key-stdin` 互斥。
可用已有 `alva-hub-server --new-token` 显式生成随机token，再分别保存上述JSON和仅含原始token的文件。
文件由使用者显式提供；`--token-file` 要求已有、当前owner私有、非symlink/hardlink的regular文件并只读打开，适用于Windows。
MCP提供日志/动态level读取、实际视图状态/revision、产品语义树和注册窗口PNG；不导出框架内部layout，也不截取全屏。
如需UI action，桌面增加 `--debug-controls`，MCP增加 `--ui-controls`；Send还需桌面独立 `--debug-execution`，仍走实际产品审批/计费。
action必须带当前revision和唯一operation ID；原ID重试只返回原结果。凭据编辑页拒绝截图，诊断shutdown不停止Agent。
详见 [UI diagnostics](./apps/debug/AGENTS.md) 和 [命令约束](./cmd/alva-devtools-mcp/AGENTS.md)。

direct/Hub Inspector均有当前页LLM/Tools/Middleware分类和显式JSONL导出；导出包含选中native snapshot的所有record种类，UI搜索/分类不代表导出选择。Hub可额外包含验证过的typed children，文件和媒体保留引用。原生写自己的私有exports目录且不覆盖旧文件；认证HTTP `/v1/sessions/ID/inspector-bundle?branch=BRANCH` 只下载bytes，不写宿主路径。8MiB/2048 records/32 sessions/8hop任一超限或hash错误均明确失败，不返回半份成功。

Hub 原生面板或认证 HTTP `/v1/gateway` 还可显式启动/停止内嵌模型 API；启用需单独确认模型费用，
只使用本进程已配置的 live route。状态读取不泄露 bearer；每次启动返回新 token，停止会等待实际请求取消/收尾。
原生的多窗口共享同一个执行 owner，各自保留草稿与视图；关闭旁观窗口不等于 Stop。
可选工具设置已接 Shell/Web/Planning/Notebook/Interaction/Utility 的真实装配，编辑与独立权限确认只影响下一 Turn。
媒体可显式生成≤60秒 mono16kHz PCM WAV，原生 Play/Pause/seek 控制本地音频；上传/索引不自动播放或投递模型。

独立模型 Gateway 与 Hub/Agent runtime 分离，共享同一个provider传输与codec：

```sh
go run ./cmd/alva-gateway --new-token
go run ./cmd/alva-gateway --secrets-stdin
```

第二条通过pipe或显式重定向文件读以下JSON并以EOF结束；示例地址是占位符，key只在内存：

```json
{"token":"生成的token","routes":{"local-alias":{"kind":"anthropic","endpoint":"https://example.invalid/v1/messages","model":"actual-model","api_key":"YOUR_KEY","max_output_tokens":4096}}}
```

默认只监听随机numeric loopback端口；请求带Bearer。`GET /v1/models`只列配置alias，不调用模型。
`POST /v1/chat/completions`、`/v1/responses`、`/v1/messages`及Gemini
`/v1beta/models/local-alias:generateContent`/`:streamGenerateContent?alt=sse`可显式调用该route。
Gateway不是会话上传入口，不自动执行工具、解析host文件、抓取媒体URL或恢复历史。
支持≤16个route/8并发，body及总流各≤8MiB，不重试付费请求；细节见[Gateway约束](./apps/gateway/AGENTS.md)。
签名/加密续传不能保真时跨协议明确失败，原生同协议保留；媒体转换需要本地resolver时明确失败而不偷偷下载。

媒体工具需在会话设置显式启用；上传仍只进入小引用。PNG/JPEG/GIF可生成有损JPEG，
视频/WebP需独立批准的显式ffmpeg/ffprobe配置。选图另外要求operator对当前live route指定
`--image-capable`；未知支持不猜。只选中派生图进入一个请求副本，原件与后续历史不自动重放。
处理/真实帧时间/哈希/损失与准备、调用意图、响应观察分别记录，不把它们当作理解或网络接受证明。

## 构建和验证

```sh
go test -mod=readonly -race -p 1 ./...
go test -mod=readonly -race -p 1 -tags desktop ./...
go vet ./...
go vet -tags desktop ./...
go mod verify
CGO_ENABLED=0 go build -o build/alva ./cmd/alva
CGO_ENABLED=0 go build -o build/alva-gateway ./cmd/alva-gateway
CGO_ENABLED=0 go build -o build/alva-hub-server ./cmd/alva-hub-server
CGO_ENABLED=0 go build -o build/alva-devtools-mcp ./cmd/alva-devtools-mcp
CGO_ENABLED=0 go build -o build/alva-hub ./cmd/alva-hub
CGO_ENABLED=0 go build -tags desktop -o build/alva-desktop ./cmd/alva-desktop
CGO_ENABLED=0 go build -tags desktop -o build/alva-hub-desktop ./cmd/alva-hub-desktop
```

普通测试不启动窗口；带 `desktop` 的测试使用 MyGo Tester，不调用真实模型。
SDK 同时执行跨目标编译；WASIp1 worker已有真实fresh-host执行回归，完整平台分发仍在迁移。

```sh
CGO_ENABLED=0 GOOS=js GOARCH=wasm go build ./sdk/... ./integration/llm/codec
CGO_ENABLED=0 GOOS=wasip1 GOARCH=wasm go build ./sdk/... ./integration/llm/codec
CGO_ENABLED=0 GOOS=wasip1 GOARCH=wasm go build -o build/alva-worker.wasm ./cmd/alva-worker-wasm
```

脚本能力使用固定QuickJS-NG v0.17.0 core WASM，936,180字节（约0.89MiB），每次调用独立实例，由Go宿主执行。
普通CLI构建直接嵌入固定资产，无需C/Node工具链；只有维护者重建引擎时需要WASI SDK。

WASM CLI 可显式开启指令预算和独立脚本能力：

```sh
./build/alva --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --api-key-stdin --workspace /absolute/project --sandbox wasm --wasm-module build/alva-worker.wasm --grant /absolute/project=/work --wasm-fuel 1000000000 --wasm-scripts --prompt '检查所选工作区'
```

`--wasm-fuel`使用版本1 Core V2基本块预扣，0表示仅使用deadline/memory/IO限制；host callback等待和动态bulk-memory字节不计为指令。
`--wasm-scripts`默认关闭，每次调用新的QuickJS-NG core WASM实例，不含std/os模块、loader、Node或shell。
Script有编译完成后开始的10秒独立初始化/执行deadline、32MiB跟踪heap和512KiB软C栈；冷编译仍受job总deadline约束，64MiB实例reservation包含在job整体linear-memory预算内，
正数fuel也覆盖脚本引擎指令。Ask/Plan下原有host grant仍只读；脚本不会增加目录或fetch domain授权。
普通脚本错误保留可能的部分写入与输出引用，Agent可以继续；整体fuel耗尽/用户取消停止job。脚本正文与结果不会在启动或重启时重放。
引擎出自固定C源码，可用[显式maintainer构建脚本](./platform/sandbox/host/scriptwasm/build.py)重建；普通Go编译无需C工具链；引擎重建使用显式WASI SDK clang。

依赖首次下载后，可给这些命令设置 `GOPROXY=off GOTOOLCHAIN=local` 验证离线构建。
MyGo 固定 v0.5.5，checksum 校验保持开启；无需安装 Node/Bun/npm。
当前只在 macOS arm64 环境编译和测试，真实 OS 窗口、其他平台另行验收。

## 数据与生命周期

- SDK 内存 session 保存原始 typed facts；模型请求的大工具文本先投影为引用，按需有界读取。
- 工具返回的图片/音频/视频也先保留完整DTO引用，不因一个插件返回媒体就自动送模型；
  单次显式选图与普通历史投影分开，带真实处理来源。
- CLI 显式 `--data-dir` 使用新的私有 native SQLite；`--session ID` 仅恢复已存在会话，
  `--sessions` / `--history` / `--search` 为冷读取，不启动模型或恢复 Agent。
- Hub 的 product SQLite 与 native typed records/artifacts 分权。相同记录正文只保存一份，
  冷 Inspector 读取小索引/固定高水位分页，需要正文时显式按会话读取。
- `--attach PATH`（可重复）仅存原始字节，20 MiB/文件、40 MiB/最多 8 文件/轮，
  模型只收到来源/处理说明和小引用；不会因上传就自动投递图片/视频/文本。
- SDK 默认 memory backend 和无 data-dir 的 direct desktop 仍易失；direct native 会话显式持久化，持久 memory 仍是独立 opt-in Plugin。
- 无任何旧数据/配置导入、双写、FFI 或兼容格式；旧源码已归档至 `archive/rust-final-2026-10-18` 并退出工作树，用户数据不清理。
- runtime 拥有执行，UI 仅订阅；退订不是 Stop，慢观察者用快照对账，不重放任务。
- 当前聊天视口最多保留 64 条展示行，不等于删除 SDK 事实；耐久记录按需分页/搜索，
  完整产品历史与 Inspector 大规模性能仍持续验收。
- UI、journal 和传输有显式预算，超限报错，不宣称无限上下文。

工具产品设置/问答、worktree HTTP/独立确认丢弃、只读Devtools MCP、内嵌Gateway、LLM缓存审计、
音频、多窗口与WASM fuel/QuickJS已接入实际回归。Hub后台Shell由应用内Sessions跨正常Turn监管，
SDK与product workspace租约持到reap；原生工具pane与认证`/v1/sessions/{id}/shell-jobs`提供冷查、Stop和idle reset。
Worker附件只将本会话metadata/refs入队并固定原branch；停驻分支任务不改道或自动投递原件。
Rust本身未实现的完整LSP/网络embedding/自动LLM memory提取和worktree rebase/证据归档归新增能力，
不将deprecated stub当作迁移实现。Windows DACL/LockFileEx/创建时Job树、只读seekable媒体stdin/handle、WASI读写/atomic NOFOLLOW和CLI file/pipe已补；Windows pinned host shell/交互console/worktree权限证据仍有迁移缺口，Windows原生运行、真实OS窗口/AX/声学与完整原fixture对照仍未验收；
逐项状态见[MIGRATION.md](./MIGRATION.md)。公开面、分形文档和依赖边界包含在architecture测试中。

Hub CLI与direct `alva`分开，复用Hub desktop/server的同一个bootstrap/product权威（不能并发打开同一私有DB）：

```sh
go run ./cmd/alva-hub --data-dir /absolute/new-go-hub --prompt '你好'
go run ./cmd/alva-hub --data-dir /absolute/new-go-hub --sessions
go run ./cmd/alva-hub --data-dir /absolute/new-go-hub --session PRODUCT_ID --repl
```

REPL支持`/session`、`/sessions`、`/engine alva|claude|codex|pi`、`/history [after]`、`/quit`。
外部引擎须显式exe及`--external-host-approved`，不发现既有账户；手动切换保留branch/native身份，冷读不执行。
stdout只在实际owner成功收尾后输出最终答复；Alva从exact committed native record恢复完整正文，不拿60KiB产品预览冒充完整答案。
默认`--permission deny`；`--permission ask --repl`须输入actual request ID逐次批准，没有auto-allow/bypass。
live的API key用独立`--api-key-fd 3`（显式regular/pipe FD3..10），不占REPL stdin、不进argv/environment。

Direct原生CLI也可显式`--repl`，仍直接消费harness（不经Hub/EngineRuntime）：

```sh
go run ./cmd/alva --demo --repl --data-dir /absolute/new-direct-go-history
```

同会话多轮，`/session /help /quit`本地处理；live配置可用单独`--api-key-fd 3`，REPL不与`--api-key-stdin`混用。
Ask按actual request ID只批准当前调用，非交互Ask继续拒绝写入；不会因选择workspace授予shell/network。
两个CLI显示层共用C0/C1转义，原始facts/工具参数不改写；明文stdin key读取也支持真实pipe取消。

Hub引擎的执行能力与检查能力分别声明：认证`GET /v1/engines`只返回实际装配能力，不探测账户或程序。Inspector元数据/2KiB原文/JSONL均消费独立EngineInspector，`/v1/sessions/ID/inspector-events?branch=BRANCH`以native durable Sequence（`Last-Event-ID`）观察新metadata，断线不Stop。外部engine当前检查能力明确unsupported；Alva已绑定实际harness的MaxTurns/SystemPrompt，Claude已接原生append/max-turns参数；Codex/Pi不降级system role为user文字，未接线的选项与fast-mode不会显示为支持。

可选 Python 插件作者 SDK 位于 [integration/protocol/aep/python](./integration/protocol/aep/python/README.md)，
通过真实 Go AEP loader 互操作测试。它是插件侧客户端，不是第二套 Agent 实现；普通 Go 构建/运行
不要求 Python，执行 Python 插件时才显式选择解释器和宿主批准。

## 仓库布局与历史

项目名为 Alva，单一 module 使用仓库地址 `github.com/QuincySx/Alva`，没有外层 go/ 容器。
这遵循 [Go 官方的单模块仓库指南](https://go.dev/doc/modules/managing-source)。
SDK、扩展、harness、产品与横向适配的职责不因 module 提升而合并；入口继续位于 cmd。

最终 Rust 完整快照为 `archive/rust-final-2026-10-18`（`1d675f730223`），含源码、锁文件、
旧前端和审查资料。旧代码仅在 Git 历史中，查阅可用：

```sh
git show archive/rust-final-2026-10-18:Cargo.toml
git log archive/rust-final-2026-10-18 -- crates/
```

历史实现保留其原有许可；当前许可与第三方归属见 LICENSE、NOTICE。
