# Alva 使用与验证

所有命令从仓库根目录执行。项目分层与入口见 [README](../README.md)，实际验收状态见 [MIGRATION](../MIGRATION.md)。

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

可选 Python 插件作者 SDK 位于 [integration/protocol/aep/python](../integration/protocol/aep/python/README.md)，
通过真实 Go AEP loader 互操作测试。它是插件侧客户端，不是第二套 Agent 实现；普通 Go 构建/运行
不要求 Python，执行 Python 插件时才显式选择解释器和宿主批准。

## 可选持久等待 API

[`harness/durable`](../harness/durable/AGENTS.md) 是显式安装的 harness 能力，默认 CLI/Hub 尚未自动启用。
它复用普通 SDK Agent；Backend 的四个方法是 Log、Claim、Load、Commit，存储可替换。
NewMemoryBackend 是进程内参考实现；native.Open 绑定现有私有 repository 的同一会话和事实数据库。

装配流程：

```go
backend, err := durableNative.Open(ctx, repository, owner, nativeID)
if err != nil { return err }
// 实际 Agent 使用同一个 backend.Log()。
waits, err := durable.New(durable.Config{Backend: backend, TTL: time.Hour})
if err != nil { return err }
views, err := harness.NativeSessionViews(ctx, repository, owner, nativeID)
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

上述core为sdk/agent/core，durable为harness/durable，durableNative为harness/durable/native。
`outcome.Suspended` 表示真实执行及收尾已经退出并保存waiting检查点，没有成功/失败终态事件。
宿主应展示`outcome.Requests`；新进程重建相同装配后可用`waits.Snapshot(ctx)`读取，不必保留等待goroutine。

宿主确认结果后调用`waits.Resolve(ctx, durable.Resolution{RequestID: id, Binding: binding, Result: rawJSON})`，
再`waits.Resume(ctx, agent, emit)`。Binding必须来自当前真实会话的待办；它只用于核对所审内容，不是独立访问凭据。
审批结果为`{"allow":true|false}`，问答结果为interaction.Answer，外部结果为有界JSON。
Controller实现普通security.ApprovalHandler与interaction.Handler，宿主可以显式注入原有Guard/问答工具；
审批匹配会话、工具、原参数哈希与效果，消费一次且受期限约束。`await_external`仅提交数据，不启动外部任务。

原生大结果先保存同会话引用，模型按需用context_read/search读取；结果交付和running状态同事务CAS，不能重复投递。
未知版本、过期、陈旧revision或running/interrupted不自动重放；检查原始事实后可明确Abandon，
这不代表外部任务已停止或已回滚。Controller拥有执行claim，原生装配使用NativeSessionViews，
不重复安装NativeSessionPlugins的claim。实际外部job的启动、取消和收尾仍由宿主owner负责。

实际退出/重开与副作用后崩溃不重放验收：`go test -mod=readonly ./harness/durable/...`。

## 可插拔存储 API

内核日志由 `abi.ConversationLog` 注入；SDK `core.MemorySession` 与
`context.MemoryReferences` 不需要本地目录。可选等待的 `durable.Backend` 独立要求原子CAS/实际执行claim。
原生产品默认仍使用私有SQLite；新artifact字节可以显式替换后端，事实JSON和授权metadata保留在原事务：

```go
// blob 为 platform/storage/blob；blobDirectory 为其 directory 子包。
objects, err := blobDirectory.Open("/absolute/host-owned/object-directory")
if err != nil { return err }
defer objects.Close() // host 拥有adapter；须在所有使用者结束后关闭
repository, err := session.OpenWithBlobs(ctx, "/absolute/private/native-data", session.BlobConfig{
    ID: "workspace-objects-v1", // 非秘密部署namespace，不能放凭据或URL
    Retention: blob.Persistent,
    Store: objects,
})
if err != nil { return err }
defer repository.Close()
// 继续使用同一Repository创建session、NativeSessionPlugins及具体原生能力。
```

临时对象可以用 `blob/memory.New(memory.Limits{Bytes: 64 << 20, Objects: 1024})`，
并明确设置 `Retention: blob.Volatile`；引用会标记进程生命周期，进程结束后字节不会恢复。
全内存运行可直接使用SDK日志/引用及durable.MemoryBackend，不必为了临时字节打开SQLite。

云盘/网盘适配器只需实现两方法 `Put(ctx,key,data)` 和 `Read(ctx,key,offset,limit)`；
必须创建完整不可变对象、同key异内容报冲突、写后可读、返回自有buffer并遵守预算/取消。
directory adapter要求hard-link原子发布和file/directory Sync，不支持的挂载必须明确报错或使用专用API adapter。
真实云账号/provider、产品CLI设置和后台迁移任务尚未装配，不隐式扫描挂载点或读取云凭据。
已有inline数据不迁移；外部存储的namespace ID/retention绑定后，缺失或替换adapter会拒绝打开。
错误上传可能留下孤立对象，不自动重试/删除；外部字节不具备与数据库的分布式事务，也不授予文件执行权限。

共用后端验收：`go test -mod=readonly -race ./platform/storage/blob/... ./platform/storage/session`。

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
