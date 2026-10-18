# ADR 0012 Go 实现与自绘桌面

- Status: Accepted
- Date: 2026-10-16
- Scope: 全栈目标实现、SDK 边界、原生桌面与迁移验收
- Supersedes: ADR-0006 中 Tauri/React 的目标技术栈选择；领域、交互与可访问性要求不变

## Context

用户决定把 Alva 的目标实现改为 Go，并明确要求 subagent 实施、主 agent 审核。
目标是减轻长期开发依赖、构建和维护负担；桌面不继续依赖 React，采用 MyGo 的 Go 自绘路线。
用户要求保留现有细分抽象，历史兼容层不进入新实现。抽象是否过细必须提出可讨论的具体
职责、消费者、收益和损失；无法形成明确结论则保留现有情况。

## Decision

### 责任边界保留

保留 wire/bus 等 L0 纯数据与协调原语、kernel 契约和可替换循环、agent 语义与 SDK 装配、
context/memory/security 能力、工具/插件实现、协议与 Provider 适配、平台宿主、BaseAgent
默认配方、EngineRuntime/adapters、Hub 产品监管、持久化和 UI 呈现的独立职责。

Go package 不必逐字对应 Rust crate 名称，但本次不合并上述职责。单个 module 中的 import
防火墙必须证明 SDK 不直接或传递引用具体 Provider、产品、host、MyGo；kernel 不引用
agent 语义或装配层。协议包不替宿主保存凭据或决定授权，UI 框架不成为业务容器。

ConversationLog 是内核的窄读写契约；查询等 SessionStore 能力仍可选。Tool 调度、只读
判断和执行上下文按实际调用绑定。Plugin 继续提供 tools/middleware/services/prompts，
装配与晚期接线分开，失败可传播；SDK 拒绝重名，默认配方通过同名插件替换默认。
不把细分抽象变成没有实际消费者的空接口，也不因 Go 的语法更简单删除已有表达能力。

### 2026-10-17：按层级与用户产品重新分类目录

用户进一步要求打开目录就能看出 L1/L2/L3 与面向用户的产品，一周后仍能恢复理解。
因此采用四个纵向域与两个横向支撑域，而不是减少 package 或把所有实现压成一条依赖链：

| 当前目录 | 层级与边界 |
|---|---|
| `go/sdk/` | L0 wire/bus/sandbox ABI；L1 kernel/abi；L2 kernel/core；L2.3 agent/abi；L2.5 agent/core；L3 Agent 能力 |
| `go/extension/` | L4 可选行为；具体 Tool/Plugin/Middleware 不进入稳定 SDK |
| `go/harness/` | L5 BaseAgent 配方与产品引用/默认/子会话政策 |
| `go/apps/` | L6 direct/Gateway、L7 Hub；cmd 只拥有显式启动/退出 |
| `go/integration/` | 模型/协议/引擎外部边界，非 SDK；engine 仅根契约为 L4.5 |
| `go/platform/` | host 执行/审批 owner、system OS 原语、storage、实际 sandbox guest/host |

目录迁移关系如下，**只作为本次重排记录，不提供旧路径 alias 或兼容层**：

| 重排前 Go 路径 | 当前 Go 路径 |
|---|---|
| `llm/wire`、`kernel`、`agent`、`sandbox/abi` | `sdk/llm/wire`、`sdk/kernel`、`sdk/agent`、`sdk/sandbox/abi` |
| `llm/codec`、`llm/provider`、`protocol`、`engine` | `integration/llm/codec`、`integration/llm/provider`、`integration/protocol`、`integration/engine` |
| `app`（不含 gateway） | `harness`（Go package 同步为 harness） |
| `app/gateway`、`cmd`、`desktop` | `apps/gateway`、`apps/cmd`、`apps/desktop` |
| `hub`、`hub/native`、`desktop/hub` | `apps/hub`、`apps/hub/bootstrap`（package bootstrap）、`apps/hub/desktop` |
| `host`、`native`、`storage`、`sandbox` 实现 | `platform/host`、`platform/system`、`platform/storage`、`platform/sandbox` |

host 不因放入 platform 就变成最低层：Controller 消费 harness Runner，拥有执行、审批/问题
和 command/browser 资源；system 才是更低层 OS 原语。storage 与 Hub 产品 store 权威仍分开。
Alva engine adapter 是消费完整 harness 的产品桥接；外部 process adapter 当前仍按 Vendor
分派 Claude/Codex/Pi 的共享实现，本次不伪称完成三个独立 adapter 包的一对一迁移。
Hub core 仅依赖 engine 根契约，direct CLI/desktop 仍不依赖 EngineRuntime。

原有细分实现、边界防火墙和单一 Go module 保留，不为层号新增空接口、空包或进程。
目录分组不是依赖数量、安装体积或功能完成证明。总图与阅读顺序以 go/README.md 为入口，
每个容器和子域维护分形 AGENTS.md；旧 Rust 参考源码和历史 crate 路径保持原样。

### 无历史兼容桥

Go 实现在 `go/` 独立运行，不调用旧 Rust 二进制、Tauri IPC 或 React，不写双格式记录，
不增加旧名 API 别名或隐式配置/凭据发现。不自动导入旧 session 或旧配置。
Rust tree 暂作行为和验收参考，直至 Go 对等验收完成；暂时并存不是运行兼容层。
删除旧源码、依赖、缓存或用户会话不是本决策授权的隐式步骤。

### 自绘桌面

MyGo 固定版本，以 `WindowOptions.Content` 的 Go view 创建窗口，不使用 WebView、
HTML/JavaScript/npm 前端。保留 Token → Controls → Regions/Surfaces → Domain → Pages
的职责方向，不为尚无实现的层创建空壳。MyGo 类型不能进入 Agent/Hub 业务契约。

执行属于 runtime，窗口/订阅属于观察者。关闭或更换订阅不自动等同于停止任务；Stop
和应用退出必须通过明确的运行所有者。后台结果经 UI 主线程更新；消息使用稳定身份、
虚拟列表和正确的阅读位置/跟随语义。demo 与 live 身份清楚，不在启动时发起模型请求。

### 领域与数据语义不变

HubSession 不等于 engine session；Engine 不等于 Model/Provider。Turn 内 fallback Attempt
保持串行，Hub Worker 可以独立并行；SDK 子 Agent、Hub Worker 和 MCP I/O worker 不混用。
native 记录与 Hub 产品事实分别拥有权威，Inspector/实时 UI/telemetry 不能成为第二份
恢复历史。持久化意图先于外部副作用，提交、错误、取消与重启不自动重放。

ADR-0011 的引用优先、独立 child 日志、直接父授权、媒体处理与实际投递回执必须保留。
大结果读取错误、超预算或无授权要明确失败，不静默丢原文、自动回灌或伪装成功。

### 开源与协议转换依赖

用户最初在确认 RelayKit 继承 AGPLv3 后选择开源。Go 实现采用 AGPL-3.0-only，许可证与归属保存在 go/LICENSE、go/NOTICE。

**2026-10-17 整顿决定：用户明确要求停用 RelayKit，直接吸收 Magpie。** 跨协议转换改用
`yetone/magpie` commit `e085d6e9262786815c0f6c5ff65c5a585916cc9e` 的 MIT 源码切片，
保存在 `go/integration/llm/codec/internal/magpie`；实际抽取请求 parser/builders、消息 IR、
response renderers/encoders 与纯 helpers，保留完整 MIT 原文、原文件 SHA、声明与本地修改记录。
移除 RelayKit 的 module/sum 依赖与全局 hook，不引入完整 magpie 应用或其 internal 产品包。
这项选择替代前述 RelayKit 运行依赖决策，不自动变更 Alva 已有源码的 AGPL 声明。

同协议仍保留原生薄适配与原始 JSON/opaque 续传，不强制经过 Chat 或第三方 IR；跨协议
Exchange 使用真实 Magpie parse → IR → build，并记录实际两阶段转换证据。响应复用 Alva
唯一 native decoder 的权威内容/顺序/用量后，投影到 Magpie renderer/encoder。
wire/kernel/Agent SDK 不认识 Magpie IR，Provider 继续拥有 endpoint/auth/HTTP/取消，
产品和 Plugin 继续拥有权限、引用、媒体处理与实际投递。未批准合并这些抽象。

不吸收账号发现、凭据/设置读取、外部 Agent 配置写入、自动重试/路由、磁盘签名缓存、UI
或工具执行。不能用 schema→prompt、schema union flatten、文本→toolcall 或 think-tag
识别悄悄改变调用语义；未保留的控制、约束、精确预算、签名/加密续传和参数精度明确失败。
转换器 finish 必须服从实际源终态，截断/取消/超限不补成功；工具及其后内容在完整校验并按
native index 排序后交付，普通首文本保持增量。源码吸收不代表任意供应商字段全兼容。

### 2026-10-17 历史选择：moejs 脚本引擎（被下方决定替代）

用户为简化同一套 CLI 在原生、js/wasm、wasip1/wasm 等环境的维护，明确选择用 moejs
替换 QuickJS。固定 `github.com/Calcium-Ion/moejs v0.1.0-alpha.7`，保留上游 Apache-2.0
许可；直接使用独立 Go module，不吸收 Magpie 的应用/插件宿主或 Bun。

`platform/sandbox/script` 只拥有新建执行器、严格脚本、有限结果和显式 callback，不拥有
文件/网络/进程、模块 resolver 或凭据；同一源码可在原生和 WASM 编译。`host/scriptwasm`
将其构建成每次新建的独立 Go WASM reactor，现有 host 仍拥有目录/fetch 授权、总 linear
memory reservation、取消和共享 instruction fuel，不把脚本引擎或运行政策加入稳定 SDK。

引擎预算改为 moejs 的近似累计32MiB JS分配计数与512帧递归限制，不能继续声称 QuickJS
的精确 heap 或512KiB C栈。Go WASM 引擎约12.4MiB，旧 QuickJS约0.87MiB；构建链简化
不代表产物更小。原始源码/模块 checksum、Go构建命令、完整许可与二进制哈希必须可复核。

### 2026-10-17 当前决定：继续采用轻量 QuickJS WASM 核心

用户在获知QuickJS core约0.87MiB、moejs Go reactor约12.4MiB后，明确决定继续使用QuickJS。
本决定替代上一节moejs选择。首次恢复固定QuickJS-NG v0.15.1、MIT许可和可重建C reactor，
移除moejs模块/Go执行器及Go reactor活动资产，不保留双引擎配置或旧路径alias。

首次恢复时应用正常构建/分发直接嵌入908,706字节的WASM资产，不依赖C工具链；仅显式维护者重建需要
WASI SDK。每次fresh脚本实例继续与Go Agent worker独立；文件/网络/取消/总内存与fuel归host
拥有，稳定SDK不认识具体引擎。恢复32MiB tracked heap/512KiB soft C stack，保留64MiB实例
reservation、10秒deadline和共享job fuel。构建链、体积和执行能力分别验收，不因选择C引擎
改变Go主技术栈、Magpie协议转换或harness/host/system的职责边界。

同日用户要求升级到最新正式版，当前固定QuickJS-NG v0.17.0，WASM资产936,180字节
（约0.89MiB）。源码归档checksum与二进制/bridge/bootstrap出处同步更新，使用上游WASI栈保护，
移除0.15.1所需的源码补丁；32MiB tracked heap、512KiB软栈及既有host预算/授权保持原契约。
源码重建与真实CLI/SDK guest/QuickJS回归的本批证据独立记在`go/MIGRATION.md`，旧批结果不替代新版验收。

## Verification

逐层记录运行证据，见 `go/MIGRATION.md`。第一批是标准库 SDK、Plugin 装配、明确配置的
模型协议、保留内存会话的运行控制及 native UI 可运行链路，不代表完整 Hub 已迁移。
持久化、完整权限/MCP/文件媒体、四引擎/分支/fallback/团队、graph 和 WASM 各有独立验收。

分开报告开发依赖、构建缓存、分发产物、构建时间和运行内存。当前前端依赖目录不是安装包；
小 demo 产物不能证明完整 Go 产品已达性能目标。对等功能和同一环境基线才可比较。

## Consequences

Go 的接口/函数/显式生命周期替代 Rust 特定接线形式；正确性、资源预算、race 和包依赖
约束继续用测试维护。MyGo 的平台行为独立验证，不以采用 Go 推导桌面框架已成熟。
本次不批准任何抽象合并，不保留历史兼容层，也不因追求少包而扩大单个对象的职责。
