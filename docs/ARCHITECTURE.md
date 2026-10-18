# Alva Architecture

Go 是唯一受维护的实现，module 根目录就是仓库根目录，module path 为 `github.com/QuincySx/Alva`。
本页描述当前职责和依赖方向；实际接口以源码/公开快照为准，能力与平台状态见 [MIGRATION](../MIGRATION.md)。

## 分层

| 层 | 路径 | 职责 |
|---|---|---|
| L0 | sdk/llm/wire、sdk/kernel/bus、sdk/sandbox/abi | 纯数据、类型化能力发现/独立 Topic、沙箱能力 ABI |
| L1 | sdk/kernel/abi | 模型、工具、窄对话日志、上下文与执行契约 |
| L2 | sdk/kernel/core | 可替换循环、中间件协调、模型/工具事实与真实收尾 |
| L2.3 | sdk/agent/abi | Task、Scope、Spawn、Analytics、Provider 等 Agent 语义 |
| L2.5 | sdk/agent/core | Plugin/Registrar、晚期接线、SDK Agent 装配 |
| L3 | sdk/agent/{context,memory,security,analytics,graph} | 可复用能力；Graph 是独立 runtime |
| L4 | extension | 可选 Tool/Plugin/Middleware |
| L4.5 | integration/engine 根契约 | Hub 驱动完整 Agent 的轻量契约 |
| L5 | harness | BaseAgent 默认配方、引用策略、历史与子会话政策 |
| L6/L7 | apps | direct、Gateway、Hub 及薄命令入口 |

integration/platform 为横向责任组，不代表所有内容都处于 SDK 之下。
LLM codec/provider 分开转换与传输；协议不拥有授权和持久化政策。
platform/host 拥有执行、审批/问题 rendezvous 和资源生命周期，system 提供底层 OS 原语，
storage 保留 native records/artifacts 权威，sandbox 实现 host/guest；纯 ABI 留 SDK。

## 两类装配与产品入口

```text
第三方 harness → sdk/agent/core Builder → sdk/kernel/core AgentLoop
我们的 direct CLI/desktop → harness BaseAgent → 同一 SDK 内核
Hub CLI/desktop/server → apps/hub/product + bootstrap
                     → engine 根契约 → Alva 完整 harness / 外部 process adapter
Gateway → provider + codec（不运行 Agent）
```

Plugin 注册同步封闭贡献，Seal 后注册集固定，Finalize 只读接线完整能力。
默认 harness 按 Plugin.Name 跳过同名默认项，不把替换能力做成零散 setter。
Tool、Plugin、Middleware 各有独立职责；AgentLoop、Graph、EngineRuntime 各有实际消费者，
不合成统一 runtime，也不为未来可能性预加 hook。

## 强制边界

- SDK 不得直接/传递依赖 integration、platform、extension、harness、apps 或具体第三方 Provider/UI。
- kernel 不依赖 agent 语义/装配；窄 ConversationLog 不强迫用户采用 SQLite 或产品会话模型。
- direct 产品不进入 EngineRuntime；Hub core 只依赖 engine 根契约，具体适配器在 bootstrap 装配。
- UI 与 Inspector 观察已提交事实，不负责恢复、自动重放、执行取消或审批旁路。
- 取消须传播并真实 join；物理未完成的资源仍由原 owner 持有，关闭重试不能重放副作用。
- 文件/媒体/大结果/MCP/历史引用优先；直接父授权和 child 自有事实/日志不因共享 Bus 而放宽。

`architecture` 按真实 tagged imports 强制依赖图、公开面和分形文档。
Bus 与 Registrar 的治理见 [BUS-RULES](./BUS-RULES.md)、[REGISTRAR-RULES](./REGISTRAR-RULES.md)。

## 技术栈与历史

桌面为固定 MyGo 自绘，无 React/WebView/npm；普通 Go 构建嵌入固定 QuickJS-NG WASM，
只有维护者显式重建该资产需要独立 WASI C 工具链。协议转换吸收固定 MIT Magpie 切片，
不吸收其完整应用、凭据发现或工具执行；SDK 不依赖其 DTO。

[ADR 0012](./adr/0012-go-native-stack.md) 记录技术栈和边界决策。
Rust 及旧 UI/构建工具已存于 `archive/rust-final-2026-10-18`；旧架构/产品规范可用
`git show archive/rust-final-2026-10-18:docs/ARCHITECTURE.md` 等命令查阅。
旧声明不自动成为当前实现或验收证据。

## 2026-10-18 全仓目录审查：整理提案，尚未迁移

本轮核对全部 728 个 Go 文件的 package/import 声明、跨 build tag 的 84 个包目录及 114 份
受版本管理的 AGENTS.md，结合真实消费者检查职责归属。84 个目录包含两个仅测试包，不计
testdata 中的独立探针；本机 desktop 配置列出 82 个包，另外两个是 Windows 专属原语。
这是全仓布局与依赖审查，不是所有实现逐行审计或所有平台重新验收。

结论：保留 SDK、默认 harness、外部适配与宿主实现的职责划分；整理重点是命令入口、
共享 UI、跨层验证的位置，以及 builtin 中被其他能力消费的文件系统实现。目录数量本身
不构成合并理由。以下目标路径都是提案，当前路径仍以上文与逐级地图为准。

### 全部包目录的处置

下表使用组内相对路径，覆盖当前全部 84 个包目录；“保留”指本轮结构提案，不保证永远不改接口。

| 当前组（包数） | 包目录 | 处置与理由 |
|---|---|---|
| sdk（14） | llm/wire；sandbox/abi；kernel/bus、abi、abi/sessiontest、core；agent/abi、core、analytics、context、context/scope、graph、memory、security | 保留。纯 wire/ABI、循环机制、Agent 语义、装配和能力库有不同消费者；sessiontest 是第三方存储后端可复用的合规套件，不移入私有测试目录。Graph 是独立执行模型，不并入 AgentLoop。 |
| extension（19） | browser、builtin、hooks、interaction、loader、mcp、media、memory、modelretry、notebook、planning、shell、skill、subagent、tooling、toolvalidation、utility、utility/timer、web | 按能力分包继续保留；优先拆清 builtin 的宿主文件系统、模型工具与 security wrapper。memory 复用文件工具有业务依据，不禁止一切扩展间依赖。tooling 的实际职责是诊断，后续可改为 diagnostics；utility 不继续接收无关功能，timer 保持独立可移植叶子。 |
| harness（1） | harness | 保留独立包。它拥有默认配方、同名替换和产品策略，不并入 SDK builder，也不放进 apps 的某个产品。 |
| integration（11） | engine、engine/alva、engine/process；llm/codec、llm/codec/internal/magpie、llm/provider；protocol/acp、aep、browser、mcp、skill | 保留。engine 根契约与完整产品 adapter 分开；Claude/Codex/Pi 共享 process 实现，不为目录对称制造三个包。codec/provider 分别拥有转换/传输，Magpie 继续私有封装。协议连接与上层 Plugin 的授权/注入不是重复实现。 |
| platform（16） | host、host/browser、host/command；sandbox/guest、sandbox/host；storage/memory、storage/session、storage/sqlite；system/httpjson、lineinput、loopback、media、privatefs、process、secretinput、terminaltext | 保留 host/system、storage 三种事实责任和 sandbox host/guest。httpjson 实际是严格 JSON 校验，建议后续改为 system/strictjson，地图中的“OS 原语”改为更准确的底层宿主原语。loopback 是本地 HTTP 边界，不是执行 owner。 |
| apps（21） | cmd/alva、cmd/alva-desktop、cmd/alva-devtools-mcp、cmd/alva-gateway、cmd/alva-hub、cmd/alva-hub-desktop、cmd/alva-hub-server、cmd/alva-worker-wasm；debug；desktop、desktop/modules；direct；gateway；hub、hub/bootstrap、hub/desktop、hub/devtools、hub/product、hub/server、hub/sqlite、hub/worktree | 八个 main 包移到根 cmd；desktop 拆出共享呈现与 direct 页面。debug、direct、gateway 和 Hub 各子包保留实际生命周期/事实边界；不因都叫 Service 或都用 SQLite 合并。 |
| architecture（2） | architecture、architecture/extensibility | 静态闸与真实 SDK 消费者实验分别迁入 tests/architecture、tests/extensibility。手工窗口探针迁到 tools/qa，普通单元测试继续留在所属包。 |

没有第一方生产 import 不等于废弃：analytics、graph、ACP、modelretry 等是独立可选能力；
sessiontest 由测试及外部后端作者使用。WASIp1 guest 和 Windows 专属包也不能用单一本机
go list 的结果判断死活。地图数量不等于包数，容器、资产和非 Go 作者 SDK 也各有地图。

### 优先落地的结构

```text
alva-agent/
├── go.mod / go.sum
├── cmd/                         # 当前 apps/cmd 的八个进程入口
├── sdk/                         # 保留当前 L0–L3 与公开路径
├── extension/                   # 按能力组织，不按 Tool/Plugin/Middleware 分树
├── harness/                     # 默认配方
├── integration/                 # llm / protocol / engine
├── platform/                    # host / system / storage / sandbox
├── apps/
│   ├── direct/
│   │   └── desktop/             # 当前 desktop 中的 direct 页面与窗口装配
│   ├── ui/
│   │   ├── chat/                # direct 和 Hub 确实共用的呈现状态/控件
│   │   └── modules/             # 版本化可信消息 renderer
│   ├── debug/                   # 默认关闭的观察与诊断
│   ├── gateway/                 # 模型 HTTP/SSE 产品
│   └── hub/                     # 保留根调度契约与 product/bootstrap/sqlite/server/desktop 等子包
├── tests/
│   ├── architecture/            # 依赖、布局、分形、公开面、转换来源闸
│   └── extensibility/           # SDK 外部消费者式实验
├── tools/qa/                    # 显式手工工具，不被测试隐式启动
└── docs/                        # 架构、治理、ADR、使用与验收记录
```

1. **根 cmd：先做入口位置调整。** Go 官方在同时提供库和命令的模块中推荐使用
   [cmd 子目录](https://go.dev/doc/modules/layout)，不是要求所有 Go 项目套一棵固定模板。
   八个 main 包没有生产 import 消费者，迁移主要影响构建/测试命令、CI、路径 fixture 和文档。
   main 可以保留 CLI 私有 REPL、WASM 装配和退出政策，不为追求“薄”再造一层无复用的 API。
   不新增 pkg 容器，不拆多个 go.mod，不改变当前 module 身份。

2. **共享 UI：先拆角色，再移动。** 当前 apps/hub/desktop 复用 apps/desktop 的 Model、
   Message、Status、NativeView、Inspector 分类和 modules，同时整个包还依赖 direct/native
   会话与存储。共享 chat 仅保留真实共用的呈现模型/控件；direct 页面及窗口订阅/持久化接线
   归 apps/direct/desktop，Hub 页面留原位。现有 direct 诊断访问私有视图状态，拆包要定义
   窄的呈现/诊断接线方法，不能把字段全部导出，也不能把执行 owner 移进 UI。暂不再拆
   inspection 包，先让少量共享分类函数随 chat，避免单函数一包。

3. **验证与工具：按运行方式归位。** architecture 根是静态测试，不是生产架构实现；
   extensibility 则实际运行 SDK、双向 transport 和独立进程恢复实验；native_window_qa.py
   还会显式构建/打开临时窗口。三者分开。公开面快照和扫描器 fixtures 随静态闸保留，
   各产品/协议的 testdata 继续就近；不是把所有 *_test.go 集中到 tests。

这三步不需要修改 SDK 契约，但会修改包路径与测试定位，不可仅用文件移动成功作为验收。
当前 layout_test.go 明确禁止根 cmd，依赖闸也硬编码了 apps/cmd 和 apps/desktop。
实施时须同步更新路径角色、MyGo 准入、direct 不进入 Hub/engine 的传递边界、旧路径拒绝
以及 cmd-extra 等伪前缀反例。建议把角色路径集中在测试 helper 定义，继续独立验证实际 import
图，不删除防火墙或扩大成“整个 apps 都能引 MyGo”。

### 第二步：builtin 的职责提取

[builtin/files.go](../extension/builtin/files.go) 同时定义 os.Root 授权文件系统和 LLM FileTools，
[snapshot.go](../extension/builtin/snapshot.go) 又提供 notebook 使用的受检读写。六个实际生产
消费者是 CLI、direct、Hub bootstrap、harness、memory、notebook。建议目标为：

- platform/host/workspace：现有 pinned roots、Workspace、FileSnapshot 与受限宿主操作。
- extension/files：FileTools、参数校验、引用先行读取/备份、模型调用适配。
- extension/security：现有 SecurityPlugin 与实际 Tool 身份/effect 审批接线。

这是提取已有具体能力，不新建万能 FileSystem interface；workspace 不依赖 extension、
harness 或 host 父包。现有方法直接依赖 security.Guard 和 abi.Access，属于带授权的宿主能力，
不能当纯 OS 原语搬进 system。工具的引用与会话政策仍留 extension/files；memory 的 sync/extract
确实可继续消费 files，notebook 只依赖 workspace。拆分时不公开 os.Root 或私有路径绕过闸门，
保留 symlink/逐级 pin、审批、备份、快照冲突、权限/mode 和不确定写入结果的已有反例。
这会改变外围组件的公开类型所在包，须独立于第一步提交和说明，不冒充无成本改名。

### 公开边界、资产及文档

- **不把所有非 SDK 包塞进 internal。** Go 的 internal 是外部模块无法导入的真实边界。
  SDK、后端合规套件、可选扩展、harness、协议/provider 和可插拔存储有外部复用价值。
  Hub/Gateway 的嵌入是否承诺为公开 API 尚需单独确定；当前可导入就是实际外部可见性，
  不能把产品整个迁入 internal 当作普通整理。先明确支持面，再限制真正私有实现。
- **Hub bootstrap 保留集中装配。** 它被三个入口共用，依赖多符合集中装配组件的角色；
  不把其配置/依赖集合搬入 Bus 或内核。Hub sqlite 保存产品事实，platform/storage/session
  保存 native 原始事实，platform/storage/sqlite 提供底层开库能力，三者不因技术相同合并。
- **资产与非 Go 桥保留归属。** QuickJS-NG WASM、C 重建桥、哈希/许可继续放在 sandbox 宿主资产旁；
  Magpie 固定切片继续封装于 codec/internal；AEP Python 作者 SDK 留 protocol/aep/python。
  它们不是未删除干净的 Rust。fixture/testdata 不搬成生产库或误归档。
- **根目录保留单模块和少量入口文档。** README 保留定位、快速开始、地图和命令入口；详细
  运行说明后续放 docs。MIGRATION 保留当前验收/缺口入口，旧批次记录可完整迁到 docs/history，
  不删除证据或把历史通过改写成当前通过。AGENTS 描述当前地图，职责变更同步级联更新；
  ADR 保留已接受决策，提案未执行前不能改写当前目录表。
- **CI 与用户目录。** .github 继续只声明 Go 验证/构建；路径迁移与 fixture 一起更新。
  当前 Browser-portable SDK 步骤是交叉构建，不能据其命名声称执行过 JS/WASM；guest 实际运行
  和宿主测试继续分别记录，后续将已有 JS/WASM 执行流程纳入常态 CI 时明确单列。
  .alva/.codex/.claude 等用户配置和外部缓存不是本轮结构整理的删除对象，不纳入源码搬迁。

落地顺序：入口路径 → 共享 UI → 验证/手工工具 → builtin 宿主能力提取 → 命名与文档瘦身。
前三项也分成独立变更，保留可 bisect 的验收。每步运行相关包与架构闸；UI 拆分另验两个桌面
入口，入口迁移另构建八个命令及 WASIp1 worker。最后统一跑 tagged race、vet、模块校验和
实际 WASM 测试，跨编译结果与实机/外部账户验收继续分别报告。
