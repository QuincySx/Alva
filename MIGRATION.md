# Alva 实施与验收

Go 是当前唯一受维护实现；Rust 与旧前端已按用户授权存入 Git 标签 archive/rust-final-2026-10-18。
技术栈决策见 [ADR 0012](./docs/adr/0012-go-native-stack.md)，当前结构见 [ARCHITECTURE](./docs/ARCHITECTURE.md)，运行说明见 [USAGE](./docs/USAGE.md)。
目录与源码验收不等于全部功能对等、全部平台或外部账户验收。

## 已确认约束

- 每个抽象先保留，包或接口合并须给出独立职责、真实消费者、收益、损失供用户讨论。
- Go 不依赖 Rust/Tauri/React 的运行桥，不隐式读旧配置、历史或凭据。
- 保留 SDK 与默认 harness 两入口、单 Agent loop 与多 engine Hub 两套不同 runtime。
- 保留原始事实/投影区别、窄会话契约、同名默认 Plugin 替换、版本化扩展记录和权限边界。
- 先有可执行和反例，再报告完成；不拿界面出现当作整个产品迁移完成。
- 用户确认开源后，Go实现采用AGPL-3.0-only，许可/归属见LICENSE/NOTICE；当前吸收的Magpie协议代码只由integration/llm/codec使用，不进入SDK事实类型/权限/存储。

## 2026-10-19 普通工具与原生插件政策拆分

用户确认本轮插件瘦身。planning、interaction、utility 根包现为可直接安装到 SDK 的普通能力；
每类的 native/ 子包保留具体产品接线，Hub 显式导入对应适配，不保留旧原生 New/Config 路径 alias。

- Planning 保留原 Service/Task DTO 契约，NewTools 只需要业务 Service 与完整详情引用 writer。
  七个工具的 schema/参数/业务操作只有一份实现；原生 wrapper 继续验证实际 Tool、同一 session
  authority、run/workspace 与 Guard，默认 DAG/CAS/冷恢复和原生详情 evidence 不下沉到普通工具。
- Interaction 的 Question/Answer/Handler 留在原根包；NewTool 只需要 Handler 与引用后端。
  解析、真正同步等待、取消晚答拒绝和引用回执与原生适配共享；原生仍负责并发4、配置期限、
  Guard、脱敏 artifact 和 question_intent/question_outcome，不把人类回答当权限审批。
- Utility 将 PreferenceStore/ApplyPreferences、普通 config/tool_search 与已有 timer 分开消费。
  普通工具不安装 middleware；偏好由一个明确的 BeforeModel hook 消费，目录每次读取 live registry。
  原生 Plugin 继续拥有会话 snapshot/CAS、Guard、已选 schema 脱敏 refs 和现有下一请求行为。
  即便用户替换后端，普通工具/投影也拒绝不支持的 presentation key/value。
- 三个根包的直接/传递依赖仅限标准库、SDK 与已有纯 strictjson helper；静态闸拒绝回引 native/、
  host、SQLite、provider 和 helper 偷带重依赖。CI 的 js/wasm、wasip1/wasm 构建增加这三类能力和 timer。
- SDK 全部源码/六份公开快照、go.mod/go.sum 与固定 QuickJS 资产未修改，没有新增外部依赖，
  Plugin.Name、工具名和已有记录 namespace 不变。13 个原生基线测试全部迁移保留。

本轮 macOS arm64 / Go 1.27.2 验证如下，无付费模型或外部 engine 账户调用：

| 检查 | 本轮结果与范围 |
|---|---|
| 整树 desktop-tagged race | 1792 项通过（含子测试）/85 个有测试包，0 失败，715.93 秒；durable helper 无参数入口按设计 skip，实际独立进程恢复实验通过 |
| 最终相关包 race | 三类能力及各自 native/、timer、架构闸：222 项通过；普通问答实际遵循宿主 ToolTimeout，native 保留自管期限；同 ID 外部 log 和 Guard 拒绝仍不能借委托绕过 |
| JS/WASM 实际运行 | Node 执行三个普通能力与 timer：12 项通过，0 skip |
| WASIp1 实际运行 | 同四包由 wazero 实际执行：12 项通过，0 skip |
| 可移植构建 | 完整 SDK、codec 与三个普通能力/timer 在 js/wasm、wasip1/wasm 均通过 |
| 原生命令构建 | CGO_ENABLED=0，tagged cmd 在 macOS arm64、Linux amd64、Windows amd64/arm64 均通过；跨编译不代表对应 OS 实机验收 |
| 静态与依赖完整性 | tagged vet、go mod verify、gofmt、空白/当前文档链接检查通过；实际 go list 证明普通能力没有 native/SQLite/host/外部依赖，静态反例防回归 |
| 原实现保留 | 原生 Planning Service 的 12 个函数在去除类型限定名后与原字节格式化结果一致；Question/Answer/Handler 契约文件原字节保留；13 个原生基线测试全部保留 |

整树计数对应增加最后一条超时回归之前的拆分版本；最后的问答超时归属与普通偏好说明调整后，
受影响能力/原生适配及架构已完整重跑222项，两种 WASM 也已重新编译并实际运行。
完整日志/依赖图/构建结果保存在本机 alva-portable-extensions 证据目录，指针为
/tmp/alva-portable-extensions-path。SDK 源码、六份公开快照、模块锁定与 QuickJS 固定资产未改。
前一批完整验收仍见下文，不以旧结果代替本轮验证。

## 2026-10-18 全仓结构整理落地

用户授权开始实施。第一步已将八个程序入口从 apps/cmd 迁至根 cmd，不保留旧目录或 alias；
同步 CI、动态 WASIp1 fixture、说明地图与传递依赖闸。库/产品不得依赖 cmd，入口可装配所属产品；
新增四个反例验证缺失源节点的入口、平台/产品反向依赖和旧路径。架构闸 159 项通过，
desktop-tagged 全入口构建及实际 CLI/WASIp1 附件/历史/宿主凭据隔离回归通过。
第二步将静态闸与六份原字节公开快照放入 tests/architecture，消费者实验独立为 tests/extensibility，
手工窗口探针放 tools/qa/native_window.py；集中 module 根定位并更新 CI/文档。
补生产包不得导入 tests/tools 的反例。新位置两包 189 项通过，durable helper 无参数入口按设计 skip，
实际独立进程恢复实验通过；手工工具仅验证 --help 与语法，不启动窗口或声称实机验收。
第三步完成 apps/ui/chat、apps/ui/modules 与 apps/direct/desktop 拆分；Hub 直接复用共享控件，
direct 产品说明/窗口/诊断/订阅留在所属产品。仅增加呈现保留上限构造、只读高水位及已存在运行的
观察重绑方法，不导出可变 ListState/内部序号，不修改 SDK 契约。保留 64/128 行政策、真实 Send/Stop、
诊断、虚拟列表与阅读锚点；新增共享 UI 不得取得 direct/Hub/harness/host/engine 的传递依赖反例。
共享 UI/direct/Hub/桌面入口/架构六包 desktop-tagged race 共 237 项通过；实机窗口另验。
第四步移除 extension/builtin，提取 platform/host/workspace、extension/files 与 extension/security，
所有实际消费者绑定新类型路径，不保留旧路径 alias。根句柄和锁不导出，文件变换通过宿主 Update
在串行读取/备份回调/原子写入间保留边界；工具仍负责严格参数、引用与原有 JSON 结果，security
保留 actual Tool 值身份与 effect/HITL。notebook 只依赖宿主 workspace；memory 显式复用文件工具。
原 builtin 的 12 个测试全部保留，另验证备份失败/取消不写入、无残留 staging、根路径被替换后
Read/Update/List/Search 仍只访问原 pinned root。八个消费者/能力包 race 99 项通过；后续补充反例与
静态闸的结果另随本批最终验收记录。137 个受保护 SDK/模块/固定资产文件保持原字节。
第五步将 platform/system/httpjson 改名为 strictjson，将 extension/tooling 改名为 diagnostics，
同步实际 import、调用限定名、目录地图及退休路径闸。仅调整源码归属名称；Plugin.Name("tooling")、
配置键、工具名和已有 records namespace 均保留。相关两能力/Hub 装配/架构四包 race 239 项通过。
文档已分离当前入口/使用说明/历史记录；本批结构整理与源码验收完成。
当批跨 build tag 共 87 个包目录、736 个 Go 文件；比审查时新增的三个包来自两处职责拆分，
不是新增进程或外部依赖。SDK 公开包路径和全部源码保持原字节，六份快照未修改；外围类型路径迁移不保留 alias。

## 本批最终验证

macOS arm64、固定 Go 1.27.2，功能改动后独立执行以下检查；无付费模型或外部引擎账户调用。

| 检查 | 本批结果与范围 |
|---|---|
| 全树 desktop-tagged race | `go test -mod=readonly -tags desktop -race -p 1 -count=1 -timeout 6m ./...`：1777 项通过（含子测试）/82 包，0 失败，640.71 秒 |
| 无直接测试的 support | WASIp1 Worker 的 native 拒绝入口、私有 Magpie 切片、SDK sessiontest 合规套件；durable helper 无参数入口按设计 skip，真实独立进程恢复实验通过 |
| JS/WASM 实际运行 | Node 执行 bus、agent/core、context/scope 与 tests/extensibility：181 项通过，0 skip |
| WASIp1 实际运行 | 同四包通过 wazero 执行：181 项通过，0 skip；全树 native suite 另实际执行 SDK guest、fuel 与固定 QuickJS WASM |
| 完整 SDK 与 codec 的跨目标构建 | js/wasm、wasip1/wasm 均通过 |
| 程序构建 | CGO_ENABLED=0；macOS arm64、Linux amd64、Windows amd64/arm64 的 tagged 根 cmd 全部通过，实际 WASIp1 Worker 单独构建通过 |
| CLI demo | 当前原生二进制的本地 model→tool→model 闭环通过 |
| direct 与 Hub 原生窗口 | 两个独立临时 demo bundle 实际创建会话、草稿/Send、真实完成、Inspector、JSONL 导出与同 operation ID 不重放通过；当前 Build 的 Content PNG 均成功并已查看，最后回收自身进程 |
| 静态与完整性 | tagged vet、go mod verify、gofmt、99 个本地 Markdown 目标/锚点及空白检查通过；137 个受保护 SDK/模块/固定资产文件保持原字节，六份公开快照与基线一致 |

最终格式整理仅重排八个测试文件中的 import，已逐字核对函数体与其余源码不变；未用格式改动替换失败测试。
原宿主的九个根/IO helper、六个 snapshot 方法和 security wrapper 逻辑保留；原 builtin 的全部 12 个测试仍在，
新增用例证明备份/回调失败与取消不改变原件、清除 staging，路径被替换后新宿主操作仍绑定原 pinned root。
历史 checkpoint 的正文完整保留，仅相对 Markdown 链接重绑新位置。

跨编译不代替 Windows/Linux 实机；本轮原生窗口验证不扩张为 compositor/完整 AX/IME/声学验收。
CI 的 Browser-portable SDK 步骤目前仍是构建；上述 Node/WASIp1 执行是本批独立本机验收。
完整日志/JSON 与 PNG 在本机 alva-layout-apply / alva-native-window-qa 证据目录，指针为 /tmp/alva-layout-apply-path。

## 尚未完成的迁移与验收

下列条目仍是实际未完成项；不以本轮测试数量替代功能对等：

1. **Windows运行与剩余adapter**：private storage的原子DACL/LockFileEx、创建时JOB_LIST/kill-on-close/真实job清空、继承stdin取消和显式只读私有token文件已实现；amd64/arm64命令和原生test-binary交叉构建及Windows vet通过。CI已声明真实Windows反例但尚未运行，不能据此宣称实机通过。Windows只读seekable媒体stdin/handle与WASI writable/atomic NOFOLLOW已补实现和原生测试；pinned-directory host shell与交互console仍未迁完；managed worktree仍以Unix executable mode/权限快照绑定证据（构造时也检查Unix execute bit），Windows模式/ACL对应策略尚未迁移或验收，不能仅放行git.exe后冒充对等。普通Unix fd3 ABI不伪装成Windows handle。
2. **原生UI调试实机验收**：动态日志level、bounded ring、实际产品语义树/状态/revision、声明action及own-window PNG已迁入独立apps/debug，两个原生UI实际绑定与HTTP/官方MCP stdio均有回归。控件mutation/Send双重opt-in、取消排队不迟到执行、operation ID不重放，诊断shutdown不Stop Agent。actual Hub own-window当前Build的Inspector PNG已通过，本批真实系统AX的创建/Send/Inspector/导出/重启基础路径已通过；CUA compositor仍只有thumbnail，完整视觉/AX/IME未通过，两者分开记录，不以MyGo Tester替代；语义树与原Rust手工InspectNode一样由产品注册，不冒充框架内部layout树。
3. **逐项对等核对**：当前六份Go公开面快照/导入防火墙/分形文档闸约束后续变更，不能证明Rust全部公开语义和适配器fixtures已逐项对照完成。direct desktop已补独立native持久化、会话恢复/附件引用/once审批/Inspector分类与导出，仍直连harness/host、不经EngineRuntime；完整Hub产品保留独立入口。不能用六份Go快照或新测试总数替代逐项Rust语义核对。
4. **真实平台和外部服务验收**：本机macOS基础原生AX演示链路已通过；CUA compositor仅返回thumbnail，完整视觉/AX/IME仍未通过；真实音频设备/声学、各付费模型/外部engine账户以及Windows运行与平台分发验收也未通过。headless MyGo Tester、本地HTTP/helper和构建结果各自只证明其实际覆盖范围。

完整逐项原语义/fixture 对照继续参考 [历史矩阵](./docs/history/2026-10-18-migration-checkpoints.md#逐层状态持续更新不代表全量迁移完成)，
并按当前代码复验。矩阵中早期 RelayKit、StateCell、旧路径或“未删除 Rust”等文字只描述对应阶段，当前状态由本页及实际源码覆盖。

**WASM 模型交付范围（2026-10-17 源码复核）**：host 的 `OpModel` 先将一次 `LanguageModel.Stream` 的全部 chunks 收入有界 `ModelResponse`，同步 exchange 返回后，guest 的 `ProxyModel.Stream` 才逐个 emit。因此目前已验证内容/终态与实际工具循环，不具备模型 token 实时穿过 WASM 边界的交付或消费侧背压；这是当前受控单job请求/响应的实现选择，不属于沙箱核心迁移缺口。沙箱的必要验收是隔离、显式授权、字节/内存/指令预算、取消和真实退出；取消不依赖token流式交付。实际入口目前为 direct CLI 显式WASM单轮，REPL拒绝该组合；Hub/direct desktop WASM模式和浏览器Agent属于独立产品范围。仅当交互式消费者需要实时文本/音频或双向通信时，再讨论有界增量通道、背压和ABI版本，不预先扩充内核，也不把接口名 `Stream` 当作实时流证明。

## 原实现与新增能力的区别

本轮直接核对Rust源码，而非把计划文档或类型声明当成已实现功能：

- `crates/alva-app-core/src/extension/lsp/mod.rs` 明确deprecated scaffold、StubLspManager，真实server I/O不在已实现范围；Go保留可替换诊断Manager及真实命令Runner，不搬空成功。
- `crates/alva-agent-memory/src/embedding.rs` 只有trait/Noop；`crates/alva-app-extension-memory/src/extract.rs` 负责显式提取文件/索引写入，没有自动LLM提取调用链。因此完整LSP、网络embedding、自动LLM记忆提取归新增需求；用户未另行要求新增能力时按现有功能迁移。
- `crates/alva-hub-desktop/src/worktree.rs` 实际有create/preview/adopt/discard，没有rebase或证据归档/人工恢复UI。Go补齐discard的受管目录功能，保留分支和证据，不复刻隐式branch删除或native历史retarget；复杂多轮merge/rebase另作增强。
- Windows private DACL/LockFileEx/创建时owned Job、WASI文件读写/NOFOLLOW、媒体stdin/handle与CLI pipe已实现并交叉构建，真实Windows运行未验收；Unix fd3 ABI和未实现的pinned shell/交互console仍明确拒绝。真实macOS视觉/AX/声学及付费账户验收也仍未完成。上述状态不能用headless测试、交叉编译或一个窗口visible日志替代。

## 历史记录与定位

- [完整迁移 checkpoint](./docs/history/2026-10-18-migration-checkpoints.md)：原批次判断、失败/修复、验收与抽象台账完整保留，未删减历史证据。
- [初始全仓结构审查](./docs/history/2026-10-18-structure-review.md)：审查时 84 个包的覆盖表、整理动机和边界。
- Rust 源码、旧 UI 和旧规范：Git 标签 archive/rust-final-2026-10-18，归档提交 1d675f730223。
- [AEP Python 作者 SDK](./integration/protocol/aep/python/README.md) 是当前 Go 宿主的可选插件侧客户端；普通 Go 构建与运行不要求 Python。
