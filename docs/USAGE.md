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
桌面启动不发送请求；明确点击 Send 才执行，Enter 换行，Stop 等待运行方确认。
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
详见 [UI diagnostics](../apps/debug/AGENTS.md) 和 [命令约束](../cmd/alva-devtools-mcp/AGENTS.md)。

direct/Hub Inspector均有当前页LLM/Tools/Middleware分类和显式JSONL导出；导出包含选中native snapshot的所有record种类，UI搜索/分类不代表导出选择。Hub可额外包含验证过的typed children，文件和媒体保留引用。原生写自己的私有exports目录且不覆盖旧文件；认证HTTP `/v1/sessions/ID/inspector-bundle?branch=BRANCH` 只下载bytes，不写宿主路径。8MiB/2048 records/32 sessions/8hop任一超限或hash错误均明确失败，不返回半份成功。

Hub 原生面板或认证 HTTP `/v1/gateway` 还可显式启动/停止内嵌模型 API；启用需单独确认模型费用，
只使用本进程已配置的 live route。状态读取不泄露 bearer；每次启动返回新 token，停止会等待实际请求取消/收尾。
原生的多窗口共享同一个执行 owner，各自保留草稿与视图；关闭旁观窗口不等于 Stop。
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
逐项状态见[MIGRATION.md](../MIGRATION.md)。公开面、分形文档和依赖边界包含在architecture测试中。

Hub CLI与direct `alva`分开，复用Hub desktop/server的同一个bootstrap/product权威（不能并发打开同一私有DB）：

```sh
go run -mod=readonly ./cmd/alva-hub --data-dir /absolute/new-go-hub --prompt '你好'
go run -mod=readonly ./cmd/alva-hub --data-dir /absolute/new-go-hub --sessions
go run -mod=readonly ./cmd/alva-hub --data-dir /absolute/new-go-hub --session PRODUCT_ID --repl
```

REPL支持`/session`、`/sessions`、`/engine alva|claude|codex|pi`、`/history [after]`、`/quit`。
外部引擎须显式exe及`--external-host-approved`，不发现既有账户；手动切换保留branch/native身份，冷读不执行。
stdout只在实际owner成功收尾后输出最终答复；Alva从exact committed native record恢复完整正文，不拿60KiB产品预览冒充完整答案。
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

添加 `--durable` 后，审批改为可持久化的等待；模型也可调用 `await_external` 提交外部结果请求。此工具只存请求，不替你启动外部任务。挂起输出 `status:"waiting"`、请求 ID/binding 并退出，REPL不自动读取下一条输入。

```sh
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --durable --prompt '执行需要审批或外部结果的任务'
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --waits
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --resolve-wait REQUEST_ID --wait-binding BINDING_SHA256 --wait-result-file /absolute/result.json
go run -mod=readonly ./cmd/alva --data-dir /absolute/private/cli --session ID --live --endpoint https://example.invalid/v1/chat/completions --model MODEL --resume-wait
```

审批结果是 `{"allow":true}` 或 `{"allow":false}`，external结果是调用者显式提供的JSON，均最多16KiB。Resolve只交付结果，不执行模型/工具。Resume不给新prompt/附件/REPL，须保持模型端点/型号、canonical工作区、权限、memory开关与启用插件配置；密钥继续由显式秘密输入提供，不进recipe。等待未解决时不启动插件，过期明确失败。

审批绑定实际会话、工具、原参数与效果，只消费一次。恢复将结果原子提交后让模型继续，宿主不重跑旧工具。进程在 running/interrupted 期间退出，结果未知；普通运行也不能绕过。先检查 `--waits` 和 native历史，再用 `--abandon-wait --wait-revision N` 明确解除该版本状态。解除不撤销副作用，也不证明外部工作取消。已批准的插件会在恢复的新turn重新启动，启动本身可能有宿主副作用。启动意图与物理收尾分别记录；崩溃留下未收尾启动时，`--waits`显示`plugin_startup_unknown`，普通运行/恢复均拒绝，须检查后按当前revision明确abandon（初始revision0也必须显式填写）。Hub的等待展示与HTTP控制未因CLI接入而完成。

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
