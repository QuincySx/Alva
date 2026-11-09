# Alva 使用与验证

所有命令从仓库根目录执行。项目分层与入口见 [README](../README.md)，实际验收状态见 [MIGRATION](../MIGRATION.md)。

## 可选项目规范兼容插件

`extension/compatibility` 默认不安装。它把外部文档归一为现有 Plugin 的提示贡献、渐进 Skill 工具和子代理模板；SDK 内核、授权、默认工具与执行模型不变。配置限定在明确 workspace 内，不读取 HOME、workspace 之外的祖先或其他产品配置。

先在项目内准备 `.alva/compatibility.json`，只选择需要的来源。仅开启 AGENTS.md 的最小配置：

```json
{"version":1,"rules":["agents"]}
```

完整选择示例（所选 Skill/Agent 目录须已存在；删去不需要的项）：

```json
{
  "version": 1,
  "scope": "src",
  "rules": ["agents", "claude", "gemini"],
  "skills": [
    {"path": ".agents/skills", "format": "agent-skills"},
    {"path": ".alva/skills", "format": "alva"}
  ],
  "agents": [".claude/agents"]
}
```

`scope` 默认为 `.`，必须是项目内已有目录。本轮仅加载 workspace → scope 祖先链上的 AGENTS.md / CLAUDE.md / GEMINI.md；越近的目录优先，同目录按 rules 数组后项优先。缺失规则文件跳过，其他读取失败明确报错。不会把其他子目录规则混进上下文，也不会随着模型访问其他路径自动加载规则。

| 格式 | 当前支持 | 边界 |
|---|---|---|
| [Agent Skills](https://agentskills.io/specification) | name/description/license/compatibility、字符串 metadata、空格分隔 allowed-tools；SKILL.md 正文与资源逐级读取 | 声明不授权；不执行 scripts，不接受厂商执行扩展字段 |
| Alva Skill | 原有数组 allowed-tools、invocation auto/explicit | 显式选择 `format: alva`；不改变原生解析规则 |
| [AGENTS.md](https://agents.md/) | 指定 scope 的祖先 Markdown 与近层优先 | 不自动扫描全仓库、HOME 或选定 workspace 之外的父目录 |
| [CLAUDE.md](https://code.claude.com/docs/en/memory) / [GEMINI.md](https://geminicli.com/docs/cli/gemini-md/) | 相同指定范围内的 Markdown 正文 | @import/include 原样保留，不展开；条件 frontmatter 报错；不宣称完整厂商规则/设置兼容 |
| [Claude Agent Markdown](https://code.claude.com/docs/en/sub-agents) | name/description、逗号分隔精确 tools、`model: inherit` 或省略、正文模板 | Read/Write/Edit/Grep 对应现有文件工具；省略 tools 表示无宿主工具。其他名称须有实际父能力/binder；不模拟 Glob/Bash；Hook、permissionMode、其他 model 等字段报错 |

文件限定 UTF-8 regular，拒绝 symlink/逃逸；配置32KiB、单规则64KiB、规则提示总256KiB；最多1024 Skills/规范化文档32MiB、64 Agent 模板/单 prompt16KiB，来源回执1MiB。Skill 声明不增加权限；子代理仍需独立 spawn/写入审批，不能读取父/兄弟会话。

去重按 **类型 + name**，路径是来源信息，正文哈希不是身份。同路径/同格式的重复配置只加载一次；原生数组格式与标准字符串格式先归一化。相同 Skill 的完整元数据、调用方式、工具名集合及正文一致，并且资源路径/媒体类型/字节 SHA 都相同，才只注册一个目录项和一个 Tool；Agent 模板同名且归一化定义一致时合并。元数据、正文或资源有差异时明确报告冲突的双方来源，由配置选择保留哪个，不自动覆盖或合并工具权限。不同 name 即使正文一样仍分别保留。

等价 Skill 副本选择已装配的 native 仓库作为主来源，其余按显式来源顺序取首项。发生同名时宿主才读取资源做有界校验（每个合并阶段总64MiB），不执行脚本、不把这些字节自动送模型。本轮资源按主来源当前数据读取，下一轮重查副本一致性。规则文件按目录作用域/来源加载，跨目录相同文字保留原有覆盖顺序，不做正文哈希全局去重。

CLI 冷检查仅输出来源路径、指令指纹和目录，不创建 Agent、连接 Provider 或执行脚本：

```sh
go run -mod=readonly ./cmd/alva --workspace /absolute/project --inspect-compat .alva/compatibility.json
go run -mod=readonly ./cmd/alva --demo --workspace /absolute/project --compat-config .alva/compatibility.json --prompt '你好'
```

live 在原有参数上加 `--compat-config`。不传该参数即关闭；新 REPL turn 重新读取。包含 Agent 定义时 CLI 另需 `--data-dir`，以便创建实际独立持久子会话。整个 CLI 在 WASIp1 guest 执行的 `--sandbox wasm` 暂不支持此宿主文件插件，组合会报错；可移植 SDK 本身不受影响。

Hub Desktop 的 Skills 面板可填写同一项目相对 JSON 路径，点击 **Apply compatibility next turn**；清空后点击即可关闭。Agent 定义另需 Session settings 的 native SubAgent opt-in。受管 `.alva/skills` 与兼容 Skill 合为一个工具；同名副本按上述规则去重。目录所选格式必须匹配文档，兼容插件不会自动改写文件或改变原生解析器的格式。

认证 HTTP：`PUT /v1/sessions/SESSION/settings/compatibility`，正文 `{"file":".alva/compatibility.json"}`；`{"file":""}` 关闭。只能是已保存 workspace 内的相对文件，不接受绝对宿主路径。保存/启用均不运行 Agent，running/waiting 时拒绝修改，其他插件批准不受影响。

规则、Skill 文档和 Agent 模板本轮冻结，指纹绑定所选配置、规则/模板字节及规范化 Skill Document；按需资源仍作为当前数据读取，不纳入指令指纹，也不执行。CLI/Hub durable 等待恢复须匹配同一指纹，变更指令会在模型/外部插件执行前拒绝继续。此时恢复原指令，或审阅并放弃旧等待后开始新任务；不会自动重放。

程序接入使用 `compatibility.Open/OpenFile`，将 `snapshot.Plugin(sessionID, references, optionalNativeSkillRepository)` 交给现有 Builder；需要模板时把 `snapshot.Templates()` 传给现有 `subagent.Config.Templates`。宿主在 Agent 和真实子任务 join 后负责 `snapshot.Close`，无需新的内核接口。

## 运行

使用 Go 1.27.2；本目录的 `.tool-versions` 固定本地工具版本。go.mod 位于仓库根目录，module path 为 `github.com/QuincySx/Alva`；以下命令直接在仓库根目录执行。

```sh
go run -mod=readonly ./cmd/alva --demo --prompt '你好，Alva'
go run -mod=readonly -tags desktop ./cmd/alva-desktop --demo
go run -mod=readonly -tags desktop ./cmd/alva-hub-desktop --demo --data-dir /absolute/private/alva-go
```

demo 是真实走模型接口 → echo 工具 → 模型接口的本地确定性 fixture，不调用模型账户。
桌面启动不发送请求；明确发送才执行，Enter 发送、Shift+Enter 换行（输入法组字中的 Enter 只选候选），Stop 等待运行方确认。Hub 工作台还支持下文说明的 ⌘/Ctrl+Enter 发送。
消息使用虚拟列表，阅读旧消息时不强制拉到底部，可以点击 Jump to latest。

live 必须显式指定完整 endpoint 和 model，不发现环境变量或旧配置。
可选 `--api-key-stdin` 从标准输入读取密钥，不能把密钥放进命令参数。

```sh
go run -mod=readonly ./cmd/alva --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --api-key-stdin --prompt '你好'
go run -mod=readonly -tags desktop ./cmd/alva-desktop --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --api-key-stdin
go run -mod=readonly -tags desktop ./cmd/alva-hub-desktop --data-dir /absolute/private/alva-go --live --provider-kind openai_responses --endpoint https://example.invalid/v1/responses --model MODEL --api-key-stdin
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
go run -mod=readonly ./cmd/alva-hub-server --new-token
go run -mod=readonly ./cmd/alva-hub-server --demo --data-dir /absolute/private/alva-go-server --secrets-stdin
```

第二条从 stdin pipe/显式重定向文件读取 `{"token":"第一条生成的token"}` 后 EOF；不支持不可取消的交互终端。
live 可在同一对象给可选 `api_key`，
并显式指定 `--live --provider-kind ... --endpoint ... --model ...`。token 不放参数、URL或启动输出。
默认只监听 `127.0.0.1` 的随机端口，输出实际地址；请求必须有 `Authorization: Bearer TOKEN`，
Host 与 Origin 必须是该精确本地地址。不接受公网/wildcard/DNS bind、跨站或query token。
API 路由与限制见 [server/AGENTS.md](../apps/hub/server/AGENTS.md)；SSE 使用 authenticated fetch，
按 committed Sequence/Last-Event-ID 续读。观察断线不会取消已认领Turn。

只读 Devtools MCP 可以连接上述已运行的 Hub HTTP，复用实际产品归属校验：

```sh
go run -mod=readonly ./cmd/alva-devtools-mcp --hub http://127.0.0.1:PORT --token-fd 3 3</absolute/private/hub-token
```

`hub-token` 为单独、显式提供的原始 bearer 文件（不是 server 的 secrets JSON）；token 不进 argv、URL、环境或 MCP stdout。
stdin/stdout 专用于 MCP stdio。`hub_inspect` 只读会话小索引、历史小页、typed child Inspector 和显式正文范围，
不暴露设置/凭据、不恢复 Agent、不提供执行/写入动作。启动和断线都不重放会话。
详见 [Devtools 约束](../apps/hub/devtools/AGENTS.md)。

两个自绘桌面也可显式启用独立 UI 调试服务，默认关闭、默认只读：

```sh
go run -mod=readonly -tags desktop ./cmd/alva-hub-desktop --demo --data-dir /absolute/private/alva-go --debug-listen 127.0.0.1:0 --startup-secrets-stdin </absolute/private/startup-secrets.json
go run -mod=readonly ./cmd/alva-devtools-mcp --ui http://127.0.0.1:PORT --token-file /absolute/private/ui-token
```

startup JSON 为 `{"debug_token":"独立256-bit随机token"}`，live 可另带 `api_key`；与 raw `--api-key-stdin` 互斥。
可用已有 `alva-hub-server --new-token` 显式生成随机token，再分别保存上述JSON和仅含原始token的文件。
文件由使用者显式提供；`--token-file` 要求已有、当前owner私有、非symlink/hardlink的regular文件并只读打开，适用于Windows。
MCP提供日志/动态level读取、实际视图状态/revision、产品语义树和注册窗口PNG；不导出框架内部layout，也不截取全屏。
如需UI action，桌面增加 `--debug-controls`，MCP增加 `--ui-controls`；Send还需桌面独立 `--debug-execution`，仍走实际产品审批/计费。
action必须带当前revision和唯一operation ID；原ID重试只返回原结果。凭据编辑页拒绝截图，诊断shutdown不停止Agent。
Hub快照提供`current_route_key`与有界`available_routes`；`select_route`的参数为`{"route_key":"快照给出的64位SHA256键"}`，仅选择当前可用路由，沿模型菜单同一个handler执行，受controls门控。该键独立于持久route JSON，不暴露账号来源或连接端点。`set_draft`和`send`沿可见工作台路径处理首页首次创建、附件和业务约束；Send仍需execution门控。快照只提供草稿字节数/SHA，不返回草稿正文。
详见 [UI diagnostics](../apps/debug/AGENTS.md) 和 [命令约束](../cmd/alva-devtools-mcp/AGENTS.md)。

direct/Hub Inspector均有当前页LLM/Tools/Middleware分类和显式JSONL导出；导出包含选中native snapshot的所有record种类，UI搜索/分类不代表导出选择。Hub可额外包含验证过的typed children，文件和媒体保留引用。原生写自己的私有exports目录且不覆盖旧文件；认证HTTP `/v1/sessions/ID/inspector-bundle?branch=BRANCH` 只下载bytes，不写宿主路径。8MiB/2048 records/32 sessions/8hop任一超限或hash错误均明确失败，不返回半份成功。

Hub 原生面板或认证 HTTP `/v1/gateway` 还可显式启动/停止内嵌模型 API；启用需单独确认模型费用，
只使用本进程已配置的 live route。状态读取不泄露 bearer；每次启动返回新 token，停止会等待实际请求取消/收尾。
原生多窗口共享同一个执行owner及同一逻辑会话的已提交草稿；各自保留光标、视图和未保存编辑，CAS冲突保留本窗内容。关闭旁观窗口不等于Stop。
可选工具设置已接 Shell/Web/Planning/Notebook/Interaction/Utility 的真实装配，编辑与独立权限确认只影响下一 Turn。
媒体可显式生成≤60秒 mono16kHz PCM WAV，原生 Play/Pause/seek 控制本地音频；上传/索引不自动播放或投递模型。

独立模型 Gateway 与 Hub/Agent runtime 分离，共享同一个provider传输与codec：

```sh
go run -mod=readonly ./cmd/alva-gateway --new-token
go run -mod=readonly ./cmd/alva-gateway --secrets-stdin
```

第二条通过pipe或显式重定向文件读以下JSON并以EOF结束；示例地址是占位符，key只在内存：

```json
{"token":"生成的token","routes":{"local-alias":{"kind":"anthropic","endpoint":"https://example.invalid/v1/messages","model":"actual-model","api_key":"YOUR_KEY","max_output_tokens":4096}}}
```

默认只监听随机numeric loopback端口；请求带Bearer。`GET /v1/models`只列配置alias，不调用模型。
`POST /v1/chat/completions`、`/v1/responses`、`/v1/messages`及Gemini
`/v1beta/models/local-alias:generateContent`/`:streamGenerateContent?alt=sse`可显式调用该route。
Gateway不是会话上传入口，不自动执行工具、解析host文件、抓取媒体URL或恢复历史。
支持≤16个route/8并发，body及总流各≤8MiB，不重试付费请求；细节见[Gateway约束](../apps/gateway/AGENTS.md)。
签名/加密续传不能保真时跨协议明确失败，原生同协议保留；媒体转换需要本地resolver时明确失败而不偷偷下载。

媒体工具需在会话设置显式启用；上传仍只进入小引用。PNG/JPEG/GIF可生成有损JPEG，
视频/WebP需独立批准的显式ffmpeg/ffprobe配置。选图另外要求operator对当前live route指定
`--image-capable`；未知支持不猜。只选中派生图进入一个请求副本，原件与后续历史不自动重放。
处理/真实帧时间/哈希/损失与准备、调用意图、响应观察分别记录，不把它们当作理解或网络接受证明。

## 构建和验证

```sh
go test -mod=readonly -race -p 1 ./...
go test -mod=readonly -race -p 1 -tags desktop ./...
go vet -mod=readonly ./...
go vet -mod=readonly -tags desktop ./...
go mod verify
CGO_ENABLED=0 go build -mod=readonly -o build/alva ./cmd/alva
CGO_ENABLED=0 go build -mod=readonly -o build/alva-gateway ./cmd/alva-gateway
CGO_ENABLED=0 go build -mod=readonly -o build/alva-hub-server ./cmd/alva-hub-server
CGO_ENABLED=0 go build -mod=readonly -o build/alva-devtools-mcp ./cmd/alva-devtools-mcp
CGO_ENABLED=0 go build -mod=readonly -o build/alva-hub ./cmd/alva-hub
CGO_ENABLED=0 go build -mod=readonly -tags desktop -o build/alva-desktop ./cmd/alva-desktop
CGO_ENABLED=0 go build -mod=readonly -tags desktop -o build/alva-hub-desktop ./cmd/alva-hub-desktop
```

普通测试不启动窗口；带 `desktop` 的测试使用 MyGo Tester，不调用真实模型。
SDK 同时执行跨目标编译；WASIp1 worker已有真实fresh-host执行回归，完整平台分发仍在迁移。

MyGo v0.7.4 的 [`ui.NewTester`](https://mygo.egoist.dev/docs/ui/testing) 在内存中渲染真实控件，可操作 Click/Press/Move/Release/Key/Type/Scroll、切换尺寸和明暗主题，并读取 Image/Frame；不需要启动 Alva 窗口。优先用这一入口测试布局、浮层位置、焦点、审批、发送/停止和 SQLite 重开；测试中的本地 HTTP/helper 不代表真实账户通过。`Advance` 推进 UI 动画时钟，不代替等待实际数据库/子进程收尾。

```sh
go test -mod=readonly -tags desktop -race -p 1 -timeout 6m ./apps/hub/desktop
```

需要检查安装后的真实窗口时，`desktop e2e` 构建才导入 MyGo 官方 `plugins/automation`；额外显式设置 `MYGO_AUTOMATION` 为私有目录内的 Unix socket 路径，未设置则不监听。插件使用官方 JSONL 的 windows/tree/act/input/menu/capture/idle，可检查实际元素树与画面；它仍连接真实运行窗口，不能算无窗口验收。普通 `desktop` 发布构建不包含插件。只替换唯一安装入口进行这类验收，结束后恢复普通构建，不另外启动多个客户端。框架 Inspector 的窗口树/盒模型/性能面板见[官方说明](https://mygo.egoist.dev/docs/ui/inspector)，与无窗口测试及产品 Inspector 各有不同职责。

```sh
CGO_ENABLED=0 GOOS=js GOARCH=wasm go build -mod=readonly ./sdk/... ./integration/llm/codec
CGO_ENABLED=0 GOOS=wasip1 GOARCH=wasm go build -mod=readonly ./sdk/... ./integration/llm/codec
CGO_ENABLED=0 GOOS=wasip1 GOARCH=wasm go build -mod=readonly -o build/alva-worker.wasm ./cmd/alva-worker-wasm
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
引擎出自固定C源码，可用[显式maintainer构建脚本](../platform/sandbox/host/scriptwasm/build.py)重建；普通Go编译无需C工具链；引擎重建使用显式WASI SDK clang。

依赖首次下载后，可给这些命令设置 `GOPROXY=off GOTOOLCHAIN=local` 验证离线构建。
MyGo 固定 v0.7.4，Go 工具链固定 1.27.3，checksum 校验保持开启；普通原生程序无需 Node/Bun/npm，显式选择的外部 Agent CLI 自行拥有其运行环境。
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
逐项状态见[MIGRATION.md](../MIGRATION.md)。公开面、分形文档和依赖边界包含在architecture测试中。

Hub CLI与direct `alva`分开，复用Hub desktop/server的同一个bootstrap/product权威（不能并发打开同一私有DB）：

```sh
go run -mod=readonly ./cmd/alva-hub --data-dir /absolute/new-go-hub --prompt '你好'
go run -mod=readonly ./cmd/alva-hub --data-dir /absolute/new-go-hub --sessions
go run -mod=readonly ./cmd/alva-hub --data-dir /absolute/new-go-hub --session PRODUCT_ID --repl
```

REPL支持`/session`、`/sessions`、`/engine alva|claude|codex|pi`、`/history [after]`、`/quit`。
外部引擎须显式exe及`--external-host-approved`，默认使用私有CLI配置目录。三个 Hub 入口可另传`--codex-config-dir /absolute/selected-codex-profile`、`--pi-config-dir /absolute/selected-pi-profile`或`--claude-config-dir /absolute/selected-claude-profile`，必须配对应 vendor 的`--*-command`；目录由使用者明确选择，Alva只验证目录并交给CLI，不读取、复制或链接认证文件。此方式复用完整CLI配置，CLI可以自行使用或刷新其账号；不能理解为只共享登录。所选配置目录的canonical路径SHA进入route账号来源，切换目录后旧route不能借新配置执行，旧CLI会话不自动导入。

Codex app-server按用户指定的[Cindy原生环境方式](https://github.com/makecindy/cindy/blob/5f9a219d8d789ac96de779e17c61b8126845f1bd/packages/maker-core/src/agents/codex/env-builder.ts)继承宿主环境，保留HOME、工具链与原生hook所需上下文，再设置独立CODEX_HOME；未明确选择配置时该目录仍由产品管理。补缺的Python/纯文本终端默认值，并合并NO_PROXY的本机地址；不写入mise信任或CLI配置。原生hook和信任检查继续生效。Claude、Pi与显式Codex exec继续使用私有HOME/显式环境。日志与binding均保存在产品私有目录；已知敏感环境值和人类秘密答案分别经过有界脱敏，不将宿主环境清单写入日志。
路径SHA只标识所选目录，不证明目录内当前登录者；Alva不检查目录内部的账号变化。手动切换保留branch/native身份，冷读不执行。
指定 `--codex-command` 后默认 `--codex-transport app-server`，支持实际双向审批、人类问题及同thread恢复。厂商策略显式使用 `--codex-sandbox read-only|workspace-write`（默认workspace-write，网络关闭）和 `--codex-approval-policy on-request|untrusted|never`（默认on-request），与Alva的Ask/Auto/Full不是同义映射。`--codex-transport exec`仅显式使用旧只读transport，不支持typed文件/Skill或回调，也不在协议失败时自动退回。Claude保持既有safe-mode/dontAsk，仅使用fixture验实现，不调用真实账户。

指定 `--pi-command` 后默认 `--pi-transport rpc`；应用只注入固定内容哈希的私有扩展，核对native session及ready后才发送。read/grep/find/ls保持读取，bash/edit/write每次由实际产品审批后执行，`ask_user`支持单个自由文本问答；不把Alva权限profile当Pi沙箱，也不启用全局extension/MCP/Skill发现。`--pi-transport exec`保留明确只读模式，失败不回退。两种模式的CLI完整配置仅由Pi读取。大型图片留在原件/native session，UI事件队列仅保留媒体投影并限制8MiB/64条；完成正文整体脱敏，长安全回复经诚实Delta与Completed保留全文。
桌面问题与审批分开；秘密答案之后的raw日志/流式片段不会保存，完整结果投影先脱敏。CLI默认deny，`--permission ask --repl`按不透明请求handle明确批准；CLI遇native问题会清楚报unsupported并取消/join，回答需桌面。重启/旁观/关闭面板不重放副作用。所选完整CLI配置可能包含MCP、hooks或Shell环境；用户的工具/信任错误如实返回，不自动trust全局配置或继承另一套授权。
stdout只在实际owner成功收尾后输出最终答复；Alva从exact committed native record恢复完整正文，不拿60KiB产品预览冒充完整答案。
process外部adapter当前没有Inspector，journal引用本身不保证UI/CLI可读；其长安全正文优先经持久Delta聚合，不能声称已有与Alva Repository相同的原始记录读取能力。
默认`--permission deny`；`--permission ask --repl`须输入actual request ID逐次批准，没有auto-allow/bypass。
live的API key用独立`--api-key-fd 3`（显式regular/pipe FD3..10），不占REPL stdin、不进argv/environment。

Direct原生CLI也可显式`--repl`，仍直接消费harness（不经Hub/EngineRuntime）：

```sh
go run -mod=readonly ./cmd/alva --demo --repl --data-dir /absolute/new-direct-go-history
```

同会话多轮，`/session /help /quit`本地处理；live配置可用单独`--api-key-fd 3`，REPL不与`--api-key-stdin`混用。
Ask按actual request ID只批准当前调用，非交互Ask继续拒绝写入；不会因选择workspace授予shell/network。
两个CLI显示层共用C0/C1转义，原始facts/工具参数不改写；明文stdin key读取也支持真实pipe取消。

Hub引擎的执行能力与检查能力分别声明：认证`GET /v1/engines`只返回实际装配能力，不探测账户或程序。Inspector元数据/2KiB原文/JSONL均消费独立EngineInspector，`/v1/sessions/ID/inspector-events?branch=BRANCH`以native durable Sequence（`Last-Event-ID`）观察新metadata，断线不Stop。外部engine当前检查能力明确unsupported；Alva已绑定实际harness的MaxTurns/SystemPrompt，Claude已接原生append/max-turns参数；Codex/Pi不降级system role为user文字，未接线的选项与fast-mode不会显示为支持。

可选作者插件复用同一个 AEP v0.1 协议、Plugin/Tool/Middleware、审批和会话引用：

| 写法 | 作者入口 | 执行方式 |
|---|---|---|
| Go | [公开 SDK 示例](../examples/sdk-agent/README.md) | 编译进程序的原生 Plugin，也可将整个 SDK 消费者编译到 Node/WASIp1 |
| JavaScript | [作者 SDK、声明与完整 bundle 示例](../integration/protocol/aep/javascript/README.md) | 显式 Node20+ 原生子进程；无运行时npm依赖，代码具有宿主访问权 |
| WASIp1 | [独立模块构建示例](../examples/aep-wasm/README.md) | `runtime="wasm"`、`.wasm` entry，由Go内嵌wazero加载，无目录/环境/网络 |
| Python | [现有作者 SDK](../integration/protocol/aep/python/README.md) | 显式解释器的原生子进程 |

JS/Python填写绝对interpreter路径；WASM的interpreter/args/env全部留空。Hub Desktop的Session settings → AEP Plugins先检查保存disabled pin，再明确批准下一Turn执行；pin改变后拒绝运行，不fallback到原生解释器。
编程接入使用`extension/loader`，上层程序提供会话存储、Guard和launch-risk回调；返回的非nil Plugin拥有bundle，即使同返未完成收尾错误也须保留owner并继续Close。
普通Go构建不需要Node/Python或外部WASM命令。QuickJS-NG仍用于现有受限脚本能力，与Node作者插件独立；浏览器`js/wasm`不冒充WASIp1插件ABI。检查、批准、构建与实际运行验收分别记录。

## Hub 原生工作台

`alva-hub-desktop`使用21/22工作台结构：侧栏、任务搜索、真实会话列表、顶部项目/会话位置与限宽输入区。窗口标题为Alva，MyGo v0.7.4保留原生系统按钮，macOS使用`TitleBarHiddenInset`，其他平台使用隐藏标题栏；36px页头按实时`TitleBar`安全区避让，与侧栏顶部同线。侧栏收起后完全隐藏，页头提供展开入口，“更多→新任务”仍可使用；原宽度偏好与草稿保留。空白处支持原生拖动/双击，关闭仍沿用原保存和收尾守卫，系统圆角、阴影与缩放边缘保留；跨平台实机验收分别记录。本机仅维护`/Users/smallraw/Applications/Alva.app`，沿用同一显式数据目录，不逐轮另开客户端。窗口壳实现与全部产品页面的视觉/业务验收分别见MIGRATION。

安装后的 `.app` 无参数启动只读取固定的 `Contents/Resources/alva-launch.json`（version 1）；安装者在其中明确给出 arguments 与可选 external_environment 的每引擎 PATH。它不搜索 HOME、旧配置或凭据；未知/重复字段、超限内容和 stdin 秘密请求均拒绝。普通终端显式传参时不合并此文件，仍需 `--data-dir`。本机安装配置可同时注册明确选择的 Codex/Pi，以及离线演示 Alva；选中保存的模型连接才通过 Alva 的真实 provider 路由执行，演示模式不代表真实推理。
首页“新任务”只回到编辑页，不创建空会话；建议按钮只填草稿。点击顶部工作区可显式选择项目文件夹；未选目录则使用新的私有工作区。文件夹选择不授予工具权限，也不改变已有会话的目录。
首条Send先保存捕获草稿，再以同一SQLite事务创建会话并绑定home PendingSessionID；同一产品准入冻结路由、正文、typed文件/Skill，原始用户正文与模型小索引分别保存；输入/Enter不发送。Message 中 Enter（以及 macOS ⌘+Enter、其他平台 Ctrl+Enter）沿同一 Send 路径提交，Shift+Enter 换行；菜单打开时发送键不选择、不发送，输入法组字中的 Enter 只选候选，IME/录音/提交中/运行/等待或会话未就绪也不能绕过准入（被挡住的 Enter 仍是换行，空白草稿的 Enter 什么都不做）。导入、创建或准入失败保留草稿、已创建身份和已成功引用；重试只导入失败及剩余文件。提交期间可继续编辑，旧接收回执不清除更新后的草稿，初次/同会话重载期间禁发。运行显示Stop并等待实际owner确认，切到其他会话再回来仍保留未完成停止；下一条草稿不自动提交。
“+”允许首页及已有会话先选任意位置/类型的本机文件，立即流式捕获原件与SHA到私有附属目录，随后与正文/引用一起保存草稿；重开不依赖原路径仍存在。捕获、解析与模型实际可用分别显示；失败可重试/移除，不默默丢弃后Send。模型菜单列程序目标与已启用应用连接，方向键/Enter选择、长列表滚动；失效路由或切分支后不兼容附件提前禁发并保留稿。原生权限菜单可选择每次询问、自动审批与完全访问；外部引擎继续其真实固定政策，不借原生授权。macOS系统本地语音见下方说明，其他平台明确不支持。
会话设置、等待、Inspector、协作、自动化和“路由与故障切换”从页头更多菜单进入；已批准fallback仍可查看与明确禁用。播放资源未收尾时保留暂停/清理入口，切页或关闭观察窗口不等于停止Agent。

输入 `/` 或打开“+ → 命令与 Skills…”可在首发前选择内置plan/review和明确项目中的 `.alva/skills`。方向键移动、Enter只选取、Escape关闭，仍须明确Send；选择不运行脚本或进程。项目Skill固定workspace/名称/原文SHA，创建后绑定实际会话/分支，Send重验。未全局启用Skills时只暴露本轮所选来源，不启用兄弟目录；来源改变或引擎不支持会拒绝保稿。不会隐式扫描HOME或导入compatibility目录。

首页和会话均可输入 `@` 搜索项目/最近文件，或用明确绝对路径、`~/`路径、目录导航和原生选择器选电脑上的文件；有界候选不建立全盘正文索引。选择保存不可变原件，移除chip不删除原件。单轮至多8个所选文件，捕获技术上限256MiB/文件；UTF-8短文本至多4KiB/文件和16KiB/请求，并受实际上下文准入限制，长文本经既有context_read/search按需读取。
Office是有损文本派生；macOS PDF使用独立PDFKit处理器，超限/扫描PDF/锁定/不支持如实报告，无OCR或版面还原。PNG/JPEG/GIF只在明确视觉路由上通过既有media派生与receipt一次投递，不自动启用media Tool、推断视觉能力或重放历史像素。捕获文件由原scope及home→session实际采纳grant守护，不能靠DTO把另一会话的原件拿来使用。同一会话后续仍可读取已选文本引用；新一轮不自动附加旧图片。
旧HTTP/Worker的session/branch暂存引用继续使用下面明确DraftID/SHA协议及既有8条/40MiB限制；新的桌面捕获文件不是该HTTP字段，不将共享服务等同于HTTP已曝光。

HTTP上传回执提供`draft_id`与`info.sha256`。`POST /v1/sessions/{id}/turns`和Worker新任务的JSON按本次选择传`references:[{"kind":"attachment","id":"回执draft_id","sha256":"回执SHA256"}]`；省略或空数组均无附件，未知、重复、跨分支或版本不匹配拒绝且不创建Turn。Worker任务不再接受`include_draft_attachments`。`DELETE /v1/sessions/{id}/attachments/{draft_id}`仅移除该草稿引用，保留原件。

“+ → 计划模式”首页只保留本窗意图，首条Send创建会话后才保存；已有会话只在空闲且无未放弃checkpoint时更改下一Turn。只读计划允许读取，写入/进程/网络/未知效果直接拒绝，本轮不装配Hooks、诊断、MCP/AEP、Shell/Web、Browser、Media、SubAgents。退出恢复逐次询问且不自动发送；运行或等待中的配方不变。Plan不支持外部引擎，也不能通过fallback转到外部引擎。`/plan`选择本身不改变设置，明确Send才应用计划意图。

每次询问允许读取，其他操作逐次询问。自动审批仅自动批准原生工作区文件写入；命令、网络、MCP和未知效果继续询问，尚不提供“安全命令”自动分类。完全访问需在同一菜单确认范围，只在现有Guard工作区与独立host批准范围内自动批准，不启用插件或放宽敏感路径限制。首页选择仅属于当前workspace/route，换范围恢复Ask；首发后保存到实际会话，不沿用给新任务。选择权限显式退出Plan；切入Plan重置Ask，退出不会恢复旧Full。运行/waiting/未放弃checkpoint期间不能更改，选择本身不执行任务。

## 伙伴、资源标签与系统语音

侧栏“伙伴”无需先创建普通会话。先保存伙伴名称、角色、模型/引擎和明确项目，可从已存项目选固定Skill；保存不执行、不批准账号。私聊/群发送前确认所选账号和计费，配置变更撤销旧确认。群选择成员、全部回复或明确@、有限回复数；私聊与群×成员各用独立会话，改成员/项目后保留旧范围历史。
群的“查看执行/审批”进入实际成员会话，只用于真实审批、问题、原Turn等待与Stop；新消息从群页面发。等待先在原面板解决请求，再明确“继续本群回合”，保持原Round/Turn，只补未交付结果及未开始成员。未知效果需要先审阅原checkpoint并Abandon，再确认终止；重开和查看记录不会执行或自动重试。

会话页头的文件入口打开资源标签，可关闭、通过菜单重排、专注或在同一app窗口弹出。文件/原件/任务/Inspector读取既有事实，保存查询、分页与滚动位置；关闭标签/观察窗口不停止Agent，重开不重放浏览或进程。拖拽和拖动改宽尚未提供。

点击输入区麦克风才请求系统许可。macOS要求系统本地识别可用，默认zh-CN/最长60秒，无远程fallback或音频文件。结束录音与任务Stop独立；录音/转写时先结束并核对文字后再Send。草稿未变化时结果追加，修改或导航后留作原会话的待采纳文字，可采纳/丢弃；离开聊天页停止采集，首页首次创建时文字归属跟随该草稿。取消或关窗会取消并等待实际设备/回调收尾。真实系统许可、设备和识别质量需实际点击验收。
开发者打包macOS app必须在Info.plist提供`NSMicrophoneUsageDescription`与`NSSpeechRecognitionUsageDescription`并重新签名；缺失时显示原因，不能绕过系统许可。普通go run或无用途声明的测试包不会冒充录音可用。

## Hub 模型连接

设置的“模型与连接”提供来源预设、手填完整请求端点/模型ID、添加/编辑/启停与二次删除。保存成功后，启用的模型进入输入区选择器；默认仍由程序明确配置，页面不会自动切换当前账号。使用动态连接的真实请求不标为演示；HTTP健康字段`demo`表示程序默认配方，不表示所有已保存连接均为演示。

API Key和自定义请求头值只在本窗掩码草稿和私有SQLite凭据分表保存，列表/普通DTO/错误不回显；不是加密或系统钥匙串。编辑时Key留空保留原值，显式清除或改为无认证才删除。普通导航保留本窗草稿，Cancel明确放弃；保存失败/版本冲突留稿，刷新后重新编辑。启停/编辑/删除影响之后的选择与准入，已有会话须重新选择新revision路由；不会中断当前执行或静默切换账号。

保存、打开页面和添加手填模型都不调用网络。明确点击测试/获取目录才访问同源目录，支持四协议及有界分页；目录测试不能证明真实推理成功，本机Agent账户认证仍未提供。远程端点要求HTTPS，本地HTTP只允许loopback。产品库通过v4→v5事务升级，保留已有会话/连接/目标/模型偏好与独立native库；失败不会重置。不要并发运行两个进程打开同一数据目录。

模型选择器支持真实目录聚合、搜索/连接筛选/收藏及参数范围。当前会话参数按所选账户/模型独立Save/Reload/Clear；模型默认用于未覆盖的新Turn。运行中修改只影响下一新Turn，Waiting继续保留原Attempt参数；Clear恢复已提交默认。失败/冲突保留草稿并要求Reload，不改当前模型或执行。Context无可信声明时只读未声明。

## Hub 通用外观设置

Hub原生侧栏 **General settings** 无需创建会话。应用设置使用独立分类导航，左上“返回工作台”回到进入前的会话和视图；分类切换不覆盖返回来源，未保存偏好和工作台草稿保持，返回不保存、不停止任务。媒体正文离开时仍释放/真实清理播放，返回只恢复索引位置。主题支持跟随系统/浅色/深色，正文字号支持14/16。语言/密度暂固定并隐藏入口；密度保留已存值，HTTP可保存旧字段但不代表布局已改变。
编辑只改变当前窗口草稿；明确Save后所有窗口使用已提交的外观。Reload重新读取并替换草稿；另一窗口或HTTP先保存时，旧revision保存失败，保留当前草稿并提示重新加载。
减弱动效为同一CAS保存的 `reduced_motion` 应用开关，默认false跟随系统；实际消费者使用系统要求OR已提交应用开关。开启后框架动画及共享Loader停转并停止持续请求帧，静态忙碌状态保留；系统要求不能被应用关闭覆盖。

认证Hub HTTP提供`GET /v1/preferences`与`PATCH /v1/preferences`，使用现有Bearer、Host/Origin和严格JSON校验。例如GET得到revision1后提交：

```json
{"expected_revision":1,"theme":"dark","font_size":16,"reduced_motion":true}
```

允许只提供部分设置；省略或null的字段不变，至少提供一个非null字段。成功返回完整已提交快照及新revision；输入无效400、版本冲突409、存储故障500，均不替换为默认值。
同一个Application的原生窗口与HTTP共享同一个PreferencesService；没有自动启动模型、创建聊天或恢复Turn。
外观位于`product/product.sqlite`的独立`hub_preferences`单行表，由既有Go产品库v1事务升级至v2，native记录库保持独立。
不并发运行两个进程打开同一私有数据目录。页面/服务/存储及剩余页面映射见[架构说明](./ARCHITECTURE.md#hub-页面与业务落地)。

## 第三方 SDK 与存储组件验收

[`examples/sdk-agent`](../examples/sdk-agent/README.md) 有自己的 go.mod，只导入公开 SDK。程序选择模型、会话与服务，Plugin 贡献一个 Tool 和一个 BeforeModel hook。同一源码可在 native、Node 和 WASIp1 运行：

```sh
go -C examples/sdk-agent run -mod=readonly . run
go -C examples/sdk-agent run -mod=readonly . cancel
```

run 经过真实默认循环的 model→tool→model，输出来自已提交消息；cancel 在实际工具进入后取消，再检查已结算的 cancelled outcome。模型/服务是确定性 fixture，不需账户。独立 go.mod 的相对 replace 指向本 checkout；复制到外部应用时移除 replace 并固定真实提交或已发布版本。

替换存储组件时，以下公共测试入口接收调用方的 fresh factory；factory 在每个子测试创建空组件并注册 Cleanup，测试等待并发操作完成后才交还资源：

| 组件插口 | 可复用入口 | 验收范围 |
|---|---|---|
| 纯 SDK 日志 | `sdk/kernel/abi/sessiontest.Run` | 原子记录、真实身份、匹配outcome、buffer所有权、投影、取消 |
| 应用事实 Repository | `platform/storage/storagetest.RunRepository` | 复用SDK套件；owner/authority、竞争CAS+原子交付、高水位/有界历史、引用范围、claim及父子因果 |
| 身份绑定组件 | `platform/storage/storagetest.RunBindings` | create-or-verify、重复幂等、owner/workspace冲突、竞争创建与预取消 |
| 不可变字节组件 | `platform/storage/blob/blobtest.Run` | 同值幂等、异值冲突、竞争完整发布、有界读、buffer所有权与预取消；fixture至少4对象/64KiB |

这些包仅供测试导入，生产消费者继续使用原插口。后端还须独立证明声明的持久性、容量、损坏/中途故障、跨进程锁和 Close 重试；通过内存测试不证明云服务有日志事务。当前 SQLite/opaque wrapper、bindings 和内存/目录 blob 均运行同一公共套件。

```sh
go test -mod=readonly -race ./tests/storagecontract ./platform/storage/blob/...
go test -mod=readonly ./tests/extensibility -run TestExternalModuleSDKConsumer
```

## 开发者选项：测试与评估

开发者能力在产品中：Hub侧栏 **Developer options**，以及同一个CLI的 `alva developer test` / `alva developer eval`。
共享逻辑位于[`apps/developer`](../apps/developer/AGENTS.md)，单响应测量复用SDK外的[`integration/llm/eval`](../integration/llm/eval/AGENTS.md)。

| 功能 | 目标 | 输出 |
|---|---|---|
| Integration tests / test | 接入和工具契约是否正确；目前检查单选、多选、自由文字、无需工具、选另一个工具 | passed/failed/error，不输出模型质量分 |
| Model evaluation / eval | 在独立版本化任务集上比较多次表现 | 加权任务分、全部trial通过的任务数、覆盖/错误、耗时/首输出及已报告usage |

模型评分不是普通实现测试的通过数量。协议/CLI/UI回归仍使用本地fixture，不等于GPT/Claude/Gemini真实成绩。
当前两条开发者流程都是单响应：不执行广告工具、不读取聊天历史，不证明多轮Agent、Other回填、真实权限/持久恢复或视觉能力。

### Hub UI

无需创建聊天会话，点击侧栏Developer options。Integration tests与Model evaluation分别选择；模型来自本进程明确装配的route，
live默认提供当前配置的模型，程序也可通过bootstrap.Config.DeveloperRoutes注入额外模型。外部Agent CLI订阅不自动转换成模型API权限。

选择模型、trial/总调用/token/超时预算；测试可设置工具名称变体，评估可编辑独立任务集JSON。点击Preview developer plan只校验，
确认调用数量后明确Run才请求模型；修改预览内容必须重新预览。Stop会取消并等待实际请求；多个窗口共享同一owner和最新结果，
最后窗口有开发者任务时须先Stop。打开/切页/观察不启动任务，评测也不创建聊天session或执行其插件。
报告JSON可复制；最新报告只保留在当前进程，尚无持久评测历史。每次实际Run使用所选模型账户并可能产生推理费用。

### CLI接入测试

```json
{
  "version": 1,
  "routes": [
    {"id":"route-a","protocol":"openai_chat","endpoint":"https://example.invalid/v1/chat/completions","model":"EXPLICIT_MODEL"},
    {"id":"route-b","protocol":"anthropic","endpoint":"https://example.invalid/v1/messages","model":"EXPLICIT_MODEL"}
  ],
  "tool_names": ["ask_human","AskUserQuestion","request_user_input"],
  "repeats": 2,
  "max_output_tokens": 2048
}
```

协议可选openai_chat/openai_responses/anthropic/gemini。名称对照保持相同prompt/description/真实Alva问答Schema，单轮只广告一个问答名称与weather干扰工具。
名称使用保守子集：以ASCII字母或下划线开头，后续仅字母/数字/下划线/连字符，最多64字节；不能占用get_weather。
这种对照只测当前定义下的调用可靠性，不证明训练过该工具，也不等价于厂商完整工具Schema。
此例2 routes×3名称×5检查×2 trials=60次请求，超过默认32次预算，须明确提高：

```sh
go run -mod=readonly ./cmd/alva developer test --config /absolute/tests.json --max-calls 60
# 上一条只是冷计划；--run才实际请求模型：
go run -mod=readonly ./cmd/alva developer test --config /absolute/tests.json --max-calls 60 --run --secrets-stdin < /absolute/explicit-secrets.json > tests-report.json
```

### CLI模型评估

评估不用tool_names或测试用例；配置中的dataset可省略，使用可审阅的developer-basics v1五任务集，默认每任务3次。
以下示例替换为自己的固定任务和客观评分标准：

```json
{
  "version": 1,
  "routes": [{"id":"route-a","protocol":"openai_chat","endpoint":"https://example.invalid/v1/chat/completions","model":"EXPLICIT_MODEL"}],
  "repeats": 3,
  "dataset": {
    "id": "project-tasks", "version": "v1",
    "tasks": [
      {"id":"invoice","prompt":"3 items at 49 each plus 2 at 18 each, minus a single discount of 15. Reply only with the integer total.","expected_text":"168","weight":2},
      {"id":"extract","prompt":"Lin ordered three tickets. Reply only with JSON containing exactly name and tickets; tickets is a number.","expected_json":{"name":"Lin","tickets":3},"weight":1}
    ]
  }
}
```

```sh
go run -mod=readonly ./cmd/alva developer eval --config /absolute/evals.json
go run -mod=readonly ./cmd/alva developer eval --config /absolute/evals.json --run --secrets-stdin < /absolute/explicit-secrets.json > eval-report.json
```

expected_text去除外侧空白后逐字节比较；expected_json忽略对象键顺序，严格比较字段、类型、数组顺序及数字表示，不接受Markdown fence。
每任务只能设置一个grader；权重1..100。加权分是已完成且可评分trial的通过权重/全部评分权重×100；执行错误不进分母，
所以必须同时看planned/attempted/graded/errors，不能把只完成少数任务的高分当作完整结果。consistent_task_passes仅统计所有计划trial都通过的任务。
完整数据集（含期望/权重）SHA与请求SHA用于区分版本；小任务集分数只描述该任务集，不是通用模型排行榜。
计时为本地模型接口的端到端时间/首个规范化内容事件，不是服务商纯推理时间；平均耗时含所有已尝试调用，首输出只统计实际有内容的调用。
接入测试失败返回非零；评估低分正常返回报告，API/流/取消等执行故障仍返回非零。不会自动重试或改路由。

### 共用执行边界

秘密JSON仅为route ID到API key映射，如`{"route-a":"KEY_A","route-b":"KEY_B"}`，不放进routes、argv或环境。
没有鉴权的明确本地端点可省略--secrets-stdin；不发现旧配置/账户。完整矩阵、全部route和凭据在任何请求前校验。
配置32KiB、数据集64KiB/64任务、8 routes/8名称；秘密64KiB；最多20 trial/任务、全矩阵默认32/最多256调用；
--per-call-timeout默认30s、最多2m；输出token默认2048/最多16384。模型需遵守ctx并真实join，不用后台goroutine假装硬超时。
报告不输出密钥、URL、完整prompt/响应/工具参数或思考签名；API错误与行为不合格分开，取消返回部分结果，未执行不算成功。
配置模型名不证明网关实际后端身份，usage不是费用结算。真实模型需显式运行，目前没有默认真实模型成绩。

## CLI 选项、Other 与权限

`--repl`显式开启终端交互；模型调用现有`ask_human`时，在同一轮停下来等有效回答，不先结束这一轮。仅输出一句普通文字问题不会自动变成工具等待。

```sh
go run -mod=readonly ./cmd/alva --repl --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --workspace /absolute/project
```

模型提供2..4个选项，终端额外显示Other。例如：

```text
Pick storage
1) Memory
2) Cloud
3) Local
4) Other — type a custom answer
Answer (one option number, or Other):
```

单选输入`2`，多选按提示输入`1,3`。输入`4`或`Other`后再输入一行自定义文字；Other是自由文本答案，不是模型的第四个选项，不与其他选择混合提交。没有选项时直接输入自由文本。无预选/默认回答，空输入、越界、重复选择或单选时选多项均继续等输入；EOF/取消不虚构答案。沿用32KiB/行与1024行/1MiB输入预算，Ctrl-C取消并等待真实收尾。

权限审批使用另一种提示：`allow REQUEST_ID`只批准当前实际调用一次，`deny REQUEST_ID`拒绝；其他输入、数字、Other或旧ID同样拒绝。问题与审批串行共用stdin，Other的第二行不会被审批抢走。回答“可以写入”或“允许所有操作”仍只是问答文字，不授予权限。`--permission-mode ask/accept_edits/plan`仍控制原有执行政策；Plan不因问答变成可写，accept_edits不放行未知效果。

有`--data-dir`时复用现有native问答审计/同会话引用，普通问答沿用默认5分钟等待期限；未给时使用普通问答工具与进程内引用，遵循运行context及已装配工具超时。模型先得到小回执，再按需`context_read`读取答案。未开启`--repl`或`--durable`的headless模式不广告问答工具。整Agent的`--sandbox wasm`仍不支持此REPL；Windows目前支持明确file/pipe输入，交互console/TTY仍未实现。

## CLI 插件与持久等待

插件配置是显式 JSON 数组，与 Hub 使用同一 AEP loader，例如：

```json
[{"id":"echo","source_directory":"/absolute/my-plugin","interpreter":"/absolute/bin/node"}]
```

WASIp1 配置只填 `id`、`source_directory`，不填 interpreter/args/env。源目录须包含 `alva.toml` 和入口/作者 SDK；参见上面的作者示例。JS/Python 是有宿主权限的原生代码，批准 pin 包含完整 bundle 和显式解释器/参数/环境配置；不是 OS 沙箱。环境/参数只留在用户选择的配置文件，历史保留 hash 和安全 pin。

```sh
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --inspect-plugins /absolute/plugins.json
# 记下输出的 session ID 与该插件的 review_sha256，再明确批准：
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --enable-plugin echo --approve-plugin-pin REVIEW_SHA256
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --plugins
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --demo --prompt '验证已批准的插件'
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --disable-plugin echo
```

检查、列表、批准、禁用都不启动代码或模型；重新检查清除全部启用状态。每轮复核配置字节和 bundle，变动后须重新检查/批准。空 REPL 不启动插件；每个实际 turn 使用新的插件实例，收尾后删除其私有快照。AEP工具调用仍单独审批，批准加载不批准任意工具。整套 Agent 的 `--sandbox wasm` 不会忽略会话中已启用的原生宿主管理插件，而是要求先显式禁用。

添加 `--durable` 后，审批和`ask_human`问答改为可持久化的等待；模型也可调用 `await_external` 提交外部结果请求。此工具只存请求，不替你启动外部任务。挂起输出 `status:"waiting"`、请求 ID/binding 并退出，REPL不自动读取下一条输入。

```sh
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --durable --prompt '执行需要审批或外部结果的任务'
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --waits
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --resolve-wait REQUEST_ID --wait-binding BINDING_SHA256 --wait-result-file /absolute/result.json
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --resume-wait
```

审批结果是 `{"allow":true}` 或 `{"allow":false}`；问答单选`{"selected":[2]}`、多选`{"selected":[1,3]}`，Other/自由文本`{"text":"我的自定义回答"}`，text和selected互斥，选项必须是原问题中的1-based编号。external结果是调用者显式提供的JSON，均最多16KiB。Resolve只交付结果，不执行模型/工具。Resume不给新prompt/附件/REPL，须保持模型端点/型号、canonical工作区、权限、memory开关与启用插件配置；密钥继续由显式秘密输入提供，不进recipe。等待未解决时不启动插件，过期明确失败。

审批绑定实际会话、工具、原参数与效果，只消费一次。恢复将结果原子提交后让模型继续，宿主不重跑旧工具。进程在 running/interrupted 期间退出，结果未知；普通运行也不能绕过。先检查 `--waits` 和 native历史，再用 `--abandon-wait --wait-revision N` 明确解除该版本状态。解除不撤销副作用，也不证明外部工作取消。已批准的插件会在恢复的新turn重新启动，启动本身可能有宿主副作用。启动意图与物理收尾分别记录；崩溃留下未收尾启动时，`--waits`显示`plugin_startup_unknown`，普通运行/恢复均拒绝，须检查后按当前revision明确abandon（初始revision0也必须显式填写）。Hub的独立等待UI/HTTP见下节。

实际跨进程恢复和JS/WASIp1执行验收：

```sh
go test -mod=readonly ./cmd/alva -run 'TestCLIPluginCold|TestCLIDurable|TestCLIWASIp1|TestCLIRetains'
```

## Hub 等待与恢复

Hub的Alva引擎在 **Waiting tasks → Enable durable waits next Turn** 显式启用。需要所选facts与artifact组件都声明persistent；Claude/Codex/Pi等未声明`durable_waits`的引擎不显示为支持。此产品控制当前只绑定Alva原生日志；其他引擎自己的等待控制需要对应适配，不能借用Alva存储。

挂起后，Agent及本次插件资源已退出，产品Turn保持`waiting`，原生检查点仍在同一日志。新输入、route及下一轮工具/插件设置暂时冻结。面板展示完整有界请求及binding：审批用单次Allow/Deny；问题直接单选、多选或输入文本，点击 **Save clarification without executing** 保存，不必手写JSON。问题不预选答案，选择与文本互斥，只有明确提交才保存；回答不授予文件、shell或网络权限。external仍用显式JSON和 **Save bound result without executing**。全部请求解决且仍有效、原配方可核验后，**Continue this waiting Turn**才用同一个native会话和Turn建立新Attempt。原prompt、旧工具和草稿附件不重新提交。

等待面板打开时会观察其他窗口保存的结果及继续/放弃后的状态，无须反复点击Refresh。Refresh/Continue固定在面板顶部，完整请求、已保存结果与放弃审阅在下方滚动，Turn ID可从只读字段复制。每个问题的输入和草稿独立；未提交回答仅在原branch/Turn、原请求及binding仍未解决时保留，其他请求消失不移动剩余输入的焦点。checkpoint revision变化会清除旧的放弃确认，须重新审阅。读取失败时相关操作禁用，读成功后恢复；观察与恢复显示都不会自动执行任务。

HTTP与原生UI调用同一服务，统一沿用Bearer/Host/Origin验证。以下路径前缀均为`/v1/sessions/ID`，客户端只给产品ID，不给native ID或host path：

| 方法与路径 | 请求/行为 |
|---|---|
| `GET /waits` | 冷读取`supported/enabled/branch_id/turn_id/snapshot/can_resume`，不启动Agent；`snapshot.Revision`是必须审阅的原生CAS版本，`snapshot.State.requests`包含请求ID、binding、payload、期限与已存结果 |
| `PUT /waits-enabled` | `{"enabled":true}`，idle-only持久开关，不执行 |
| `POST /wait-resolve` | `{"target":{"branch_id":"BRANCH","turn_id":"TURN","revision":123},"request_id":"WAIT","binding":"SHA256","result":{"value":"external result"}}`；只保存结果，不执行，成功后重新GET取得新revision |
| `POST /wait-resume` | `{"branch_id":"BRANCH","turn_id":"TURN","revision":124}`；全部有效结果及原配方通过后返回202及原`turn_id`；不接受新prompt |
| `POST /wait-abandon` | 同上目标/revision；检查Inspector/history后明确重置检查点。原生UI另须输入完整Turn ID；此操作不撤销副作用或取消外部任务 |

陈旧revision或跨branch/Turn的决定拒绝。等待最长24小时，结果最多16KiB（包括JSON转义后的实际字节），完整检查点最多16请求/256KiB。原生问答控件会禁用空选择或超额回答的保存，过期请求也不再接受回答/审批；已保存回答重开后只读展示，不自动执行。HTTP问题结果继续使用`{"selected":[2]}`或`{"text":"明确回答"}`，二者互斥。Resolve后保持暂停，不自动resume。续跑失败不自动fallback；程序在running期间退出时，重开保留`interrupted`/未知执行，须检查事实后明确Abandon，不重跑旧输入/副作用。已挂起Worker保留原Task/FIFO，计划保留原RunningTurnID，不定时重放或消费下一项。

继续沿用当前显式provider/model/endpoint、canonical workspace、已保存工具/插件设置与过程配方；这些发生变化会拒绝恢复。密钥不会因为等待而复制到模型上下文。启用的native插件在新的Attempt会重新启动，仍有其明确批准的宿主权限；`await_external`本身只记录数据，不替用户启动外部工作。

完整实际HTTP、MyGo无窗口控件、跨重启、审批/问答、杀死续跑进程与过期回归：

```sh
go test -mod=readonly -tags desktop -race -p 1 ./apps/hub/... ./integration/engine/...
```

## 可选持久等待 API

[`harness/durable`](../harness/durable/AGENTS.md) 是显式安装的 harness 能力，direct CLI可通过`--durable`显式接入，Hub的Alva配方通过Waiting tasks/认证HTTP显式接入，默认均不启用。
它复用普通 SDK Agent；Backend 的四个方法是 Log、Claim、Load、Commit，存储可替换。
NewMemoryBackend 是进程内参考实现；journal.Open 绑定现有私有 repository 的同一会话和事实数据库。

装配流程：

```go
backend, err := journal.Open(ctx, repository, owner, nativeID)
if err != nil { return err }
// 实际 Agent 使用同一个 backend.Log()。
waits, err := durable.New(durable.Config{Backend: backend, TTL: time.Hour})
if err != nil { return err }
views, err := harness.SessionViews(ctx, repository, owner, nativeID)
if err != nil { return err }
builder := core.NewBuilder(model, backend.Log()).Plugin(waits)
for _, view := range views {
    builder.Plugin(view)
}
agent, err := builder.Build(ctx)
if err != nil { return err }
// 以下为运行生命周期，outcome/err 均交还宿主。
outcome, err := waits.Start(ctx, agent, input, emit)
```

上述core为sdk/agent/core，durable为harness/durable，journal为harness/durable/journal。
`outcome.Suspended` 表示真实执行及收尾已经退出并保存waiting检查点，没有成功/失败终态事件。
宿主应展示`outcome.Requests`；新进程重建相同装配后可用`waits.Snapshot(ctx)`读取，不必保留等待goroutine。

宿主确认结果后调用`waits.Resolve(ctx, durable.Resolution{RequestID: id, Binding: binding, Result: rawJSON})`，
再`waits.Resume(ctx, agent, emit)`。Binding必须来自当前真实会话的待办；它只用于核对所审内容，不是独立访问凭据。
审批结果为`{"allow":true|false}`，问答结果为interaction.Answer，外部结果为有界JSON。
Controller实现普通security.ApprovalHandler与interaction.Handler，宿主可以显式注入原有Guard/问答工具；
审批匹配会话、工具、原参数哈希与效果，消费一次且受期限约束。`await_external`仅提交数据，不启动外部任务。

原生大结果先保存同会话引用，模型按需用context_read/search读取；结果交付和running状态同事务CAS，不能重复投递。
未知版本、过期、陈旧revision或running/interrupted不自动重放；检查原始事实后可明确Abandon，
这不代表外部任务已停止或已回滚。Controller拥有执行claim，原生装配使用SessionViews，
不重复安装SessionPlugins的claim。实际外部job的启动、取消和收尾仍由宿主owner负责。

实际退出/重开与副作用后崩溃不重放验收：`go test -mod=readonly ./harness/durable/...`。

## 统一存储插口

共享契约在[`platform/storage`](../platform/storage/AGENTS.md)，SDK继续只认识自己的log/reference/memory接口。
产品、harness、工具与Inspector接收同一组能力端口；Create/OpenSession、引用和分页返回共享接口/DTO，
消费者不能要求具体SQL类型或在失败后重新选择后端。

选择发生在程序装配：

```go
// selected 是本程序组装出的组件；所有下层继续消费它的共享契约。
application, err := bootstrap.Open(ctx, bootstrap.Config{
    DataDirectory: privateDirectory,
    Demo: true,
    OpenStores: selected.Stores,
    OpenMemory: selected.Memory,
    OpenBindings: selected.Bindings,
})
```

`selected.Stores`类型是bootstrap.StoresFactory，返回ProductStorage和storage.Repository。
`selected.Memory`与`selected.Bindings`分别是可选MemoryFactory/BindingsFactory；启用相应能力时必需。
现有Hub命令入口明确选择bootstrap.LocalStores/LocalMemory/LocalBindings。
Direct应用必传`direct.Config.Storage`；已知依赖由构造参数交给组件，不注册为全局存储service locator。

OpenStores缺失在IO前失败；所选工厂报错不会重试或打开默认库。
Application接管工厂返回的实际资源，包括部分成功的结果；Close先join执行，再关闭存储。
构造清理尚未完成时Open会返回非nil Application及error，调用方保留该句柄重试Close。
Direct与process.Adapter只在构造成功后接管传入组件，失败时供应方继续拥有它。

`harness.SessionPlugins`组合共享会话能力；已有外部claim owner用SessionViews，避免重复claim。
`harness/durable/journal`同样只看共享事实插口，事实与字节都persistent才声明可重启等待。
普通SDK内存日志/引用、durable.MemoryBackend与任意合约实现都不需要具体原生库类型。

事实、memory、Hub产品事实及不可变字节的职责保持独立。
如果选择现有native session实现，其OpenWithBlobs可以再装配独立blob.Store；这是该实现的组合方式，
不要求上层工具或harness知道它用了哪个数据库/云盘。目录、云服务和临时组件必须如实满足各自原子性/retention契约。
现有数据不自动搬迁，不增加云账号、扫描挂载点或产品CLI存储设置。

验收：`go test -mod=readonly -race ./platform/storage/... ./apps/hub/bootstrap ./apps/direct ./tests/architecture`。
opaque测试装饰器只保留共享接口，实际流程据此检查有没有绕过所选组件。

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
