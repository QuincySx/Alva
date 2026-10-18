# Alva Go 实施与验收

2026-10-16 用户授权开始。目标是保留架构职责、去掉历史兼容层，采用不依赖 React 的
Go 自绘桌面。决策见 [ADR 0012](./docs/adr/0012-go-native-stack.md)。

## 已确认约束

- 每个抽象先保留，包或接口合并须给出独立职责、真实消费者、收益、损失供用户讨论。
- Go 不依赖 Rust/Tauri/React 的运行桥，不隐式读旧配置、历史或凭据。
- 保留 SDK 与默认 harness 两入口、单 Agent loop 与多 engine Hub 两套不同 runtime。
- 保留原始事实/投影区别、窄会话契约、同名默认 Plugin 替换、版本化扩展记录和权限边界。
- 先有可执行和反例，再报告完成；不拿界面出现当作整个产品迁移完成。
- 用户确认开源后，Go实现采用AGPL-3.0-only，许可/归属见LICENSE/NOTICE；当前吸收的Magpie协议代码只由integration/llm/codec使用，不进入SDK事实类型/权限/存储。

## 目录分层（2026-10-17）

当前结构与阅读顺序见 [README 的 L0→L7 地图](./README.md#先看地图l0--l7)。
`sdk` 保留 L0–L3 可移植契约/机制/能力，`extension` 是 L4，`harness` 是 L5，`apps` 是 L6/L7。
`integration` 与 `platform` 是横向职责容器，不强行冒充一个统一层级；尤其 engine 仅根契约属于 L4.5，
Alva 是完整 harness 产品桥接，Claude/Codex/Pi 仍由 process package 按 Vendor 共享实现。
host 执行/审批所有权与 system 底层 OS 原语分开；纯 sandbox ABI 留 SDK，实际宿主/guest 在 platform。

该目录重排只重新分类目录、更新 imports/构建路径及文档，不批准抽象合并，不保留旧路径别名，不导入旧数据。
目录是自有源码 package 的分组，不是外部依赖数量或多进程数量。下列历史 checkpoint 的命令已按当前
等价路径标注；这些历史结果本身不替代重排后的独立构建与架构复验，也不改变剩余验收项。

## 2026-10-18 全仓结构整理落地（进行中）

用户授权开始实施。第一步已将八个程序入口从 apps/cmd 迁至根 cmd，不保留旧目录或 alias；
同步 CI、动态 WASIp1 fixture、说明地图与传递依赖闸。库/产品不得依赖 cmd，入口可装配所属产品；
新增四个反例验证缺失源节点的入口、平台/产品反向依赖和旧路径。架构闸 159 项通过，
desktop-tagged 全入口构建及实际 CLI/WASIp1 附件/历史/宿主凭据隔离回归通过。
第二步将静态闸与六份原字节公开快照放入 tests/architecture，消费者实验独立为 tests/extensibility，
手工窗口探针放 tools/qa/native_window.py；集中 module 根定位并更新 CI/文档。
补生产包不得导入 tests/tools 的反例。新位置两包 189 项通过，durable helper 无参数入口按设计 skip，
实际独立进程恢复实验通过；手工工具仅验证 --help 与语法，不启动窗口或声称实机验收。
后续继续 UI/验证工具归位、builtin 能力提取、命名/文档整理及最终整树验收。

## 2026-10-18 全仓结构复审（审查基线）

已核对跨 build tag 的 84 个包目录、728 个 Go 文件的 package/import 声明和 114 份受版本管理的
目录地图；不是全树实现逐行审计。完整覆盖表、问题证据、目标目录和分步验收见
[架构中的全仓目录审查](./docs/ARCHITECTURE.md#2026-10-18-全仓目录审查与整理方案)。
保留 SDK/扩展/harness/适配/宿主职责，建议优先根 cmd、共享 UI 与 direct 页面拆分、验证/手工工具归位，
再提取 builtin 受限文件系统。产品是否 internal 化是外部可见性决策，不随整理自动实施。
该审查轮只更新文档，生产目录、module、SDK 公开面、固定资产及运行行为未改；后续实施另见上节。
当前结构的 architecture 与 extensibility 两包共 176 项测试通过；不替代下方整树/平台验收。

## 2026-10-18 Alva 根 module 正式化

用户进一步要求去掉最外层“go”命名并核对 Go 官方规范。按单模块仓库指南
https://go.dev/doc/modules/managing-source，将 go.mod 与全部实现提升到仓库根目录，
项目名为 Alva，module path 为 `github.com/QuincySx/Alva`。命令在根目录直接执行，
apps/cmd、sdk、extension、harness、integration、platform、architecture 保留原职责，不为目录规范合并抽象。

全部仓内 import、动态构建 fixture、依赖防火墙前缀与六份公开快照同步新 module 身份。
这是外部 import path 的明确迁移，不保留旧路径 alias；方法/字段/行为契约不变。
根 README/AGENTS/NOTICE 与原 Go 目录同名文件合并，CI 从根目录运行并缓存根 go.sum；
保留完整许可、第三方来源、固定 WASM 资产和 .tool-versions。

根 module 整树首轮暴露 `TestCheckedInPythonSDKMutationsUseActualProtocol` 失败：我将当前 Go AEP
实际使用的 Python 作者 SDK 误归入旧栈一并删除。已从归档迁入 integration/protocol/aep/python，
保留现有客户端协议/行为与 MIT 声明，仅补文档头、纠正自动发现文案，并将真实 loader 测试绑定新路径。
未 skip、删除或替换该测试，不恢复 Rust 或旧路径 alias；Python 仅为显式第三方插件及互操作测试使用。
除 module 前缀外，Go 生产源码不变；唯一额外 Go 测试改动是该 SDK 的显式路径与职责注释。
失败日志与后续修复/整树复测分别保留，不能用初轮其余包通过掩盖此缺口。

下方归档阶段的 675 项与构建结果发生在旧栈删除之后、根 module 提升之前，不作为本阶段最终验收。
本阶段在根 module 完整执行 desktop-tagged race 回归，**1739 项测试（含子测试）/79 包通过，0 失败**。
三个 support 包无直接测试；无 probe 参数的 durable helper 入口 skip，真实独立进程恢复实验通过。
JS/WASM（Node）与 WASIp1 每目标 181 项实际运行，无 skip；全部 82 包 tagged 编译、全树 vet、
go mod verify 与 macOS/Linux/Windows 命令构建、WASIp1 worker、原生 CLI、本地 demo 共 6 项检查通过。
交叉构建不代替目标平台实机，Node 不代替浏览器 UI，不扩张下文已有的视觉/外部账户验收范围。
最终逐文件核对 728 个 Go 源文件：生产源码仅替换 module 前缀，另有 1 个原有测试重绑 Python SDK 路径及注释；六份快照仅变 import 身份。go.sum、固定资产、
完整许可与工具版本不变。旧栈清除、归档完整性、根 CI YAML/路径、Markdown 本地链接、gofmt 与空白检查全部通过。
历史 checkpoint 里的 go/ 路径描述其原时间；其当前等价路径去掉该前缀，不重写历史测试结论。
证据仍在本批 alva-go-cutover 目录，完整 Rust 标签与归档提交不变；最终切换单独提交，未 push。

## 2026-10-18 Go 转正与 Rust 归档（归档及删除阶段）

用户明确授权先提交所有 Rust 版本，再从工作树删除旧栈，Go 成为唯一受维护实现。
已提交完整快照 `1d675f730223`，并创建归档标签 `archive/rust-final-2026-10-18`。
快照含 Rust 源码、Cargo 锁文件、旧前端、配套脚本/CI/SDK、审查材料，以及本批之前完整 Go 源码。
旧架构/产品规范/ADR 也在该标签保留，`git show archive/rust-final-2026-10-18:路径` 可查阅；
下方旧 checkpoint 中的“不删除旧栈/尚未应用”仅描述当时批次，不能覆盖本次明确授权与已落地状态。

已移除 crates、Cargo 文件、旧 SDK/工具/内置 skills、旧 UI 与 Rust CI、旧审查目录和仓内构建产物。
根 target 原为外部共享缓存的 symlink，仅移除链接；未遍历/删除其目标，也未触及用户配置/会话。
旧调研中与 Rust 绑定的方案也统一保存在 Git 归档。根 README/AGENTS/架构/Bus/Registrar/分形协议、
许可证入口和 CI 全部面向 Go；Go module 根目录仍是 go/，import path 不变，不移动或合并现有抽象。

本批仅改文档、CI 与仓库入口；Go 生产/测试源码、六份公开快照、go.mod/go.sum 和固定资产不变。
紧邻本批之前已完成整树 race 1739 项/79 包、JS/WASM 与 WASIp1 每目标 181 项及跨平台构建。
删除旧栈后独立运行 SDK/architecture/harness/host/Alva adapter/Hub bootstrap/Gateway/direct 的
**675 项测试（含子测试）/21 包 race 通过，0 失败**；无参数 durable helper 入口 skip，独立进程恢复实验已执行。
整树 82 个 package 完成 desktop-tagged 编译（go test -run '^$'，不计为业务测试）；vet、go mod verify、
JS/WASM（Node）与 WASIp1 每目标 181 项实际运行，以及 macOS/Linux/Windows 全部命令、WASIp1 worker、
原生 CLI 和本地 demo 共 6 项构建/演示检查全部通过。交叉构建不替代目标 OS 实机，Node 不替代浏览器 UI。
本批未重复上一批已完成的整树 1739 项业务回归，不把分组重叠或纯编译计入测试总数。
Go 源码/依赖/六份快照哈希、归档完整性、旧栈清除、所有本地 Markdown 链接、CI YAML 及 diff 空白检查通过。
Go 转正不抹去下文保留的平台/视觉/外部账户验收边界，也不自动导入历史数据。

证据：`/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/alva-go-cutover-uls_u16b`，保存归档提交、删除清单、源码哈希和本批验证。归档与切换为两个独立本地提交；未 push。

## 2026-10-18 Bus 裁剪落地与回归（上一批）

用户确认上一节可审阅的最小候选后，已将同一份补丁应用到主工作区。本次是明确批准的公开 API 删除，不再只是隔离实验；不新增替代抽象或兼容别名。

- **保留** Writer/Handle、New/Provide/Resolve 与 Seal 的类型化能力注册，以及显式创建、独立持有的 Topic/Subscription。有界通知继续报告 lag，不能替代命令、模型流、执行授权或耐久事实。
- **删除** 无仓内生产消费者的 StateCell、BusPlugin/PluginRegistrar/RegisterCapability/StartPlugins，以及 TopicFor 和 registry.topics。服务生命周期由实际 owner 管理，Agent 装配仍由既有 Plugin Register→Seal→Finalize 负责。
- **公开面与成本**：删除 3 个类型、9 个公开函数/方法；Bus 生产源码 407→225 行。使用被删 API 的外部 SDK 源码需要迁移，这是已接受的源码兼容损失；不声称二进制必然缩小或性能提升。
- **保留行为**：仓内生产消费者无需修改。13 个保留函数体逐 token 相同，唯一变化的保留函数 New 只去掉 topics map 分配；其余删除函数均属于上述裁剪。对应旧 API 的测试随实现删除，保留类型身份、typed-nil、重复/封闭注册、有界 ring、lag、clone 锁边界、取消和关闭回归，不用保留空壳凑测试数量。

公开面快照与 Bus 本层/上层目录地图同步。只有 Bus 快照变化，其余五份 SDK 快照及 go.mod/go.sum 逐字节不变；SDK 外的生产源码未改动。

**已完成验证**：Bus、Agent 装配、Scope、Analytics、architecture 与 host 的 373 项测试（含子测试）/7 包 race 通过；实际运行 JS/WASM（Node）与 WASIp1，每目标 181 项、无 skip。整树 vet、go mod verify，以及 macOS/Linux/Windows 命令交叉构建、WASIp1 worker、原生 CLI 和本地 demo 共 6 项构建/演示检查通过。交叉构建不等于目标 OS 实机运行，Node 不等于浏览器 UI 验收。整树 desktop-tagged race 测试 **1739 项（含子测试）/79 包通过，0 失败**。唯一测试级 skip 是无 probe 参数的 durable helper 入口，独立子进程恢复场景已实际执行通过；另有三个无直接测试的 support 包。

证据：`/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/alva-bus-apply-_mmcz03j`，保存应用前快照、已批准补丁、实际主工作区回归、portable 运行、构建及质量检查。候选阶段证据仍见下节；本节的主工作区落地状态取代其“未应用”状态。未 stage/commit/push，未触及历史 Rust 源码或用户数据。

## 2026-10-18 Bus必要性复核与裁剪候选（提案阶段）

本批继续审查抽象的必要性，不重复把测试通过解释为每个公开机制都必要。通过Go AST按实际import alias扫描所有生产/tagged源码，排除测试与Bus自身实现，再逐项追踪实际消费者。当前Go Bus注册集由每次Agent Build创建、多轮Run复用；不是进程全局单例。服务可由宿主显式共享，Handle限制的是注册集写入，不是服务内部写权限或工具授权。

| 机制/能力 | 实际采用证据 | 结论 |
|---|---|---|
| typed Provide/Resolve | 7个生产注册点、5个解析点；共享ToolLockRegistry→Shell/Notebook、PendingService→host.Controller、AnalyticsSink→独立观察middleware有真实链路 | 保留。可选跨插件发现有独立职责；组件内已知依赖继续构造传参，不把所有参数都塞进Bus |
| Memory/Planning/diagnostics服务发布 | 有正式注册及替换/对外发现测试，但仓内生产工具直接持有已接受的Service/Manager，没有其他生产Resolve | 保留现有能力key；准确标注为对外可发现契约，不冒充已被仓内跨组件消费，也不据此删除可替换服务 |
| 独立Topic/Subscription | Blackboard创建Topic并暴露Subscribe；当前协作hooks读取历史，无实际生产订阅者 | 保留独立有界通知原语及已有BoardReader公开签名。其有界ring/lag/clone语义与历史记录各有职责；不为制造消费者把控制流改成通知驱动 |
| TopicFor | 无仓内生产引用；同类型隐式发现还需约定首个cloner/容量，Scope当前刻意持有不同Topic | 建议删除发现层与registry.topics；需要共享Topic时显式持有/注入，无需再造按类型动态创建的注册表 |
| StateCell | 无仓内生产引用 | 建议删除该SDK预留；未来实际状态owner可在外围实现快照/订阅，不要求通用内核拥有任意状态格 |
| BusPlugin/PluginRegistrar/RegisterCapability/StartPlugins | 无仓内生产引用；实际Agent已有Plugin Register→Seal→Finalize，不经过这套启动器 | 建议删除独立两阶段帮助API。普通非Agent使用者仍能New→Provide→Seal，服务启动/关闭由其owner控制，不要求为了用Bus导入Agent层 |

此前将Blackboard封装Topic称为“生产通知闭环”过宽，本批明确纠正；有发布/订阅方法声明与存在运行期订阅者是两种证据。也不能把测试消费者当成正式产品消费者。

**设计取舍**：最小候选只保留typed能力注册与显式独立Topic，Bus生产源码407→225行，减少182行；无生产消费者源码改动，不移动包、不增加别名或统一runtime。损失是9个公开函数/方法和3个类型被删除（包含StateCell四个方法及PluginRegistrar.Name）；使用这些入口的外部SDK源码会不兼容，仓内零引用不证明外部零用户。收益是少一套生命周期、少一套通用状态公开契约、去掉运行期隐式Topic创建。未声称二进制必然缩小或性能提升，Go原有未引用代码可被裁剪。

与[Pi的EventBus源码](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/src/core/event-bus.ts)对照：Pi的emit/on/clear是轻量扩展通知；不能据此为我们额外的能力注册、状态格、启动器逐项背书。AutoGen的Agent消息路由和LangGraph的图状态也不应直接搬进L0。未来同步服务直接调用已发现接口，异步服务/订阅自行持有context和Close owner；强制拦截与取消/真实join仍走既有执行契约，通知不取代命令/模型流/耐久事实。

**具体候选已在隔离副本验证，尚未应用主工作区公开API**：整树所有82个package完成desktop-tagged编译（go test -run '^$'，没有运行业务测试）；SDK/architecture/harness/Shell/Notebook/host/Hub product的629项（含子测试）/20包race通过。Bus/Agent装配/Scope及独立扩展性实验分别实际运行JS/WASM（Node）与WASIp1，每目标181项，均无skip；替换loop、上下文与多Agent实验仍通过，没有用新SDK接口补救裁剪。外部SDK无引用证据及全量产品运行重测不在本候选验收范围。

主工作区本批只更新本记录、Bus与上层地图的实际消费者说明；SDK源码、六份公开快照与go.mod/go.sum保持批前逐字节一致。候选删除对应API的测试并保留剩余广播/类型/封闭注册防线，候选快照显式反映源码不兼容；不把主工作区上一批1750项全树通过当作裁剪后全树测试结果。

证据：`/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/alva-bus-necessity-7_3r3g5q`。`bus-usage.json`/`capability-inventory.json`为真实采用清单，`candidate.patch`/`public-api.diff`为可审阅差异；候选目录、实际回归/portable编译运行和后续质量结果均保留。未stage/commit/push，未删除或修改主工作区源码/旧栈/用户数据。

## 2026-10-18 迁移后 L0→L7 逐层复核（上一批）

本批按用户要求，从当前Go实现自底向外核对契约、依赖方向、实际消费者、所有权和失败收尾；延续前批已完成的外围逐域审查，不把未改动组件的既有结论推翻重造。逐层复核已完成，发现的可复现问题已修复；本批没有新增公开方法或合并抽象。

| 层次 | 审查结论与改动 |
|---|---|
| L0 wire / bus / sandbox ABI | 修复Bus装配回调前未冻结recipe、Name重复调用、typed-nil插件准入、取消后继续Start/返回成功、panic留下可写Registrar，以及nil Registrar panic。Registrar封闭与写入同步；不承诺回滚插件自有资源。StateCell/Topic的clone和update移出状态锁，独立writer保持更新/通知顺序，clone仍单独串行，不要求调用方新增并发能力。StreamAccumulator拒绝Kind之外的非空载荷/finish身份，不能静默丢失call/usage；History逐字段校验UTF-8，Decode拒绝超过ABI硬上限的预算 |
| L1 最小契约 | 保留ConversationReader/Log/可选Store分工、typed事实与扩展namespace、真实Tool身份和声明稳定性；未知执行不伪装未执行。无新增产品字段或存储后端要求 |
| L2 通用循环 | 核对每轮model/tool/log绑定、原子intent/outcome、prepared与frozen参数、并行wave按模型顺序结算、wrapper零/一次与顺序重试窗口、真实join及只读最终准入。保留机制接口，无新增hook或产品政策 |
| L2.3 / L2.5 Agent词汇与装配 | 保留Task/Scope/Provider在内核之外、同步封闭Registrar、只读Finalize和每轮配置副本。仅修正一处已有retry测试fixture：usage/content chunk不得夹带被忽略的text，保留其原有“公开失败不能吞掉”断言 |
| L3 能力库 | ContextSource与事实历史、ContextSystem与压缩政策继续分离；Memory的backend/embedding可替换；Security保留actual invocation与直接父权限边界；Graph独立于AgentLoop。沿用前批锁外callback/取消/所有权修复，无需扩充SDK |
| L4 可选扩展 | Builtin、validation/retry、Skill/MCP/AEP、Shell/Web/Browser、Hooks/diagnostics/Media、Memory/SubAgent/Planning/Notebook/Interaction/Utility逐域对照实际配方；保留Tool/Plugin/Middleware，重依赖和资源owner留外围，portable timer仍有依赖闸 |
| L4.5 / 横向适配与平台 | Engine根契约继续仅标准库，Alva/process adapter在外层；codec/transport、协议/授权、host/system、native/product存储权威保持分开。核对进程reap、关闭重试、WASM每job授权/预算/真实退出，不建统一runtime |
| L5 Harness | 保留同名默认Plugin替换、实际native claim、引用优先、近期turn工作视图、Pending和child独立重绑；SDK用户不被迫接入我们的harness或SQLite |
| L6 产品入口 | direct CLI/desktop直连harness，不进EngineRuntime；Gateway仅协议API；debug/UI是观察与声明action，执行owner独立。核对取消、终态、冷读、关闭及operation不重放 |
| L7 Hub | 核对engine callback串行/错误锁存/迟到拒绝、exact attempt物理收尾、Team/automation/worktree准入与双层lease、native/product双store及desktop/server共享bootstrap/product。延续前批修复，无新产品权威或万能服务 |

**抽象结论**：现有核心可以保留。未来上下文替换、同步/异步外围服务、拦截、取消/中断和独立编排继续通过现有窄契约接入；真正的新runtime消费者出现后，再以执行组件复用成本决定是否新增接口，不能为了未来可能性预加hook。当前没有足够证据要求把Tool、Plugin、Middleware或AgentLoop、Graph、EngineRuntime合并。

**最终验证**：整树desktop-tagged race测试 **1750项（含子测试）/79包通过，0失败**；包含本地真实HTTP/子进程、Chrome/媒体fixture、CLI/SDK/WASI与各产品消费者。SDK/architecture分组另通过531项/15包；Bus新锁边界四场景重复20轮共80个pass事件，不与整树数量相加。wire/bus/sandbox ABI/agent-core分别实际运行JS/WASM与WASIp1，每目标203项，均无skip。首次WASIp1脚本漏写wazero的run子命令，未启动测试；保留原日志，修正运行命令后四包全部执行通过。

整树vet、go mod verify、6项构建/本地demo、gofmt/差异空白与架构闸通过。构建涵盖macOS arm64/Linux amd64/Windows amd64全部desktop-tagged命令、WASIp1 worker和native CLI；交叉构建不代替目标平台运行。六份SDK快照只有Bus的私有sync import记录更新，导出方法/字段/类型签名不变；其余五份及go.mod/go.sum逐字节不变。完整SDK首轮另暴露旧fixture的混合载荷，按上述契约修正fixture后整组及全树通过，未削弱原断言。反例失败、快照import闸失败和最终结果均保留，不隐去失败重算覆盖量。

WASM实时token通道按实际交互消费者需要添加，不是当前单轮沙箱迁移必需项；具体实现仍为有界buffered exchange。Windows专用adapter/实机、完整视觉/AX/IME/声学、真实付费模型/外部engine账户与完整Rust fixtures逐项对照仍有独立验收边界，本批不冒称这些已通过。没有删除旧源码/数据、stage、commit或push。

证据：`/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/alva-layered-02rx_7ha`；逐层范围及前批边界见review-matrix.json，反例/全树/portable/builds/quality与源码差异均在同目录。

## 2026-10-18 全层续审与修复（上一批）

按用户“每块都进行，不用问了，做完全部”的范围，将此前内核/装配/能力与外部执行审查继续覆盖到剩余扩展、harness、横向适配/宿主、所有产品与命令入口。本轮审查范围已完成；下面的已修复问题有行为反例或实际消费者回归。审查通过不是“没有任何未知缺陷”或Rust全部行为已经对等的承诺。

| 范围 | 结论与本轮修改 |
|---|---|
| Hooks / diagnostics / Web / Media | Hooks成功stdout控制拒绝重复键/大小写别名/非法Unicode；Notify与普通调用随run取消，run end must join actual callbacks before facts release; ErrFinalization marks join timeout, and terminal Close/CleanupComplete is registered in the actual Hub attempt resource list. Owner retries never replay Stop/SessionEnd. diagnostics Tool与直接Manager共用严格参数，拒typed-nil Manager，回调后复核取消/active run，输出必须是同repository/owner/native的真实且完整匹配reference。Web审批/prior risk进入run lifetime。Media Tool和WrapModel均登记实际在途调用，Close取消并join模型/工具/回执及decoder；超时保留owner，不增加模型/工具之间的串行锁。无Processor配方仍可跨run复用 |
| file / memory / notebook / interaction | 修复write_file的content:null被当空字符串覆盖已有文件；文件参数先拒null/重复键/非法Unicode。Notebook完整原件也拒重复保护字段，不能以editable:false再加true绕过保护。Memory/Interaction拒绝所有nil-capable类型的typed-nil，要求实际派发Tool、非空matching call；参数拒歧义JSON。未把独立volatile服务强行绑定native DB |
| SubAgent | 参数/actual Tool身份补齐；ChildFactory即使Prepare返回error，返回的Child.Close也转移清理责任，必须实际执行并保留原错与cleanup强错。原生factory已清理的失败不返回closer，避免双清理；父输出引用发布前Validate。未合并SDK child与Hub Worker |
| L3 Graph / Scope | Checkpoint clone移到状态锁外，取消后不提交；Channel的clone/equal/reduce移到状态锁外，writer单独串行，callback可以读Version等元数据；callback不可递归写同一channel或修改其输入。Transform拒typed-nil、回调后复核取消并clone结果，保留方不能改写返回messages。Scope/board的直接父权限上限与独立消息clone保持。这里修改的是L3私有实现，没有新增Graph或内核公开签名 |
| Harness / host | Harness在任何Plugin.Name/ID callback前冻结本次Build的完整recipe切片及ContextSystem描述符，callback对Builder的编辑只影响下一次Build。native run claim在recovery前验证actual repository/owner/session，失败的并行run end不能释放另一run的claim。Controller在启动goroutine前拒nil/typed-nil Runner。审批与问题broker继续独立，observer退订不Stop |
| 模型 / 外部engine | provider拒大小写折叠后的重复header；默认HTTP transport不消费环境proxy，只有timeout而无Transport的显式client也沿用该默认，代理必须显式提供Transport。四协议codec/损耗/终态门禁保持；process journal编码/脱敏失败锁存，不能被取消或后续帧恢复成完整成功 |
| 协议 / OS / storage / sandbox | ACP回调真实join、CDP-owned关闭、Skill根descriptor/归档、process真实reap、审批/问题及native facts/引用/恢复/SQLite隔离逐块核对；本机真实Chrome、ffmpeg与本地进程/HTTP fixtures覆盖实际路径。WASIp1/QuickJS仍是显式每job能力，无host代理/凭据继承；未把buffered模型桥描述成实时流，也未为审查新建统一runtime |
| Direct / desktop / debug / Gateway | 冷读与执行owner、严格HTTP/独立debug token、排队action取消与operation不重放、Gateway真实source终态/上游取消及Close顺序核对；direct仍不经过EngineRuntime。真实窗口线程demo/导出见下，不用无窗口Tester冒充视觉验收 |
| Hub core / store / product | Hub对engine callback串行提交并锁存第一次校验/持久化错误，adapter忽略错误再发成功也必须失败且禁止fallback；Execute返回即封闭callback，迟到事件不能改facts。真实并发callback/迟到事件和假成功均有store回归。拒user_version=0但已有任何业务表的未知库，不向其补建Hub表且保留原数据。native_history/hub_history/hub_team补齐actual Tool/call及严格DTO参数边界 |
| Hub外围 / 全部命令 | bootstrap exact attempt资源、Shell跨轮lease、Team FIFO/资源认领、automation准入owner、worktree固定SHA/原字节adoption及unknown不重放、HTTP/SSE/Inspector/devtools/多窗口观察、direct/Hub/worker/Gateway/debug CLI逐块核对；各自保持原有责任，无新增上帝接口或ad-hoc内核功能 |

**抽象结论**：保留Tool（可调用能力）、Plugin（装配贡献）、Middleware（流程拦截），继续让ContextSource/ContextSystem、标准AgentLoop、Graph和EngineRuntime各有独立消费者。本轮实际缺陷集中在外围身份、取消/join、证据保真和配方快照；L3 Graph的锁内callback属于真实实现缺陷，直接修复，而非据此扩充核心hook。kernel/ABI/agent-core源码、六份公开面快照、go.mod/go.sum本轮均未变；SDK仅上述Graph私有实现及其测试/文档变动。

**最终验证**：首次冻结源码后的整树desktop-tag race测试通过 **1736项（含子测试）/79包，0失败**。随后资源消费者复核发现Hooks缺少attempt级清理契约，补齐terminal Close/CleanupComplete及实际Hub资源注册，并把run-end join超时标为ErrFinalization强失败；Close后不再准入迟到finalizer，owner重试不重放Stop/SessionEnd。最终完整复测Hooks、Hub bootstrap、Alva adapter、product、server、desktop六包，**153项/6包race通过、0失败、0跳过**；不是把重叠分组相加冒充全量数量。整树vet、8项构建/demo、gofmt/空白/公开面与依赖哈希检查通过。整树仅跳过三个无直接测试的support包和不带参数的durable helper入口；真实独立进程恢复实验已通过。分组中因测试尚在修改时编译依赖快照不一致导致的apps首轮编译失败、provider新增测试缺time import等测试过程错误保留日志，不当成产品bug或隐去重算。实际业务反例与装配缺口均保留修复前失败和修复后回归。

Graph/Scope另外编译并实际运行JS与WASIp1：每目标Graph12、Scope5项，均无skip；不是仅交叉编译。8项构建包括portable JS/WASIp1、macOS/Linux/Windows命令、实际WASIp1 worker、native CLI与真实本地demo；交叉编译不代表跨平台运行。

本轮另以临时owner-only数据和独立debug token，实际launchservices启动自己的Hub/direct原生窗口，验证真实UI线程创建/Send、本地demo完成、Inspector JSONL落盘、同operation不重复导出，两端都通过。当前截图均明确返回frame_pending，正确拒绝过期scene；**本轮没有新的compositor/AX/IME或声学通过证据**，不覆盖历史独立验收结论。

本轮审查关闭不等于所有平台和产品验收关闭：Windows pinned shell/console/worktree mode-ACL与真实平台运行、完整视觉/IME/声学和真实付费provider/外部engine账户仍按文末范围独立验收。WASM token增量及Hub/direct desktop WASM入口是后续按消费者需求选择的产品扩展，不是单轮沙箱迁移的必需条件；没有自动重放或清理旧栈。

证据：`/var/folders/ct/65l3f8k577x1kjkbmw3rvc140000gn/T/alva-effects-7vyjzemd`，包含批前快照、各反例/分组与全量日志、真实WASM/窗口结果、构建/质量结果、逐域review-scope与最终patch。未stage/commit/push，未删除旧源码、依赖、缓存或用户历史。

## 2026-10-18 L4外部执行扩展审查：实际授权与完整收尾（前批）

本批继续向外深审MCP、AEP loader、Shell与Browser，追踪实际协议/进程owner及Hub资源装配。结论是保留Tool/Plugin/Middleware分工；可复现问题出在外围的实际调用身份和owner结束条件，没有理由为此改造内核或增加统一执行总包。SDK源码、六份公开面快照及go.mod/go.sum逐字节未变。

- **MCP实际authority**：OnRunStart必须是同一native repository/owner/session；仅相同ID的MemorySession不能取得原生worker权限。gateway在审批/日志/执行前要求实际注册Tool、正确name、非空匹配CallID与同native authority。复用原有strict JSON原语，拒绝重复键、字段大小写别名、非法Unicode与过深输入；保留256KiB总量界限。普通SDK真实native会话仍能只装一个gateway，正常模型轮不会提前启动MCP。
- **AEP实际调用**：gateway补齐ActualTool/name/CallID核对，整个输入先限256KiB再解码，并拒绝歧义JSON；schema/result仍留同会话引用。Close取消本轮上下文，审批也进入原有dispatch gate；必须等其退出才能删除snapshot。没有新增执行平台、重连或回放。
- **MCP清理与Hub owner**：run end/Close取消并等待gateway内的审批和实际调用。OnRunStart也受同一gate保护，避免清理字段并发读写；过期run end不会取消其他run。CleanupComplete独立证明gateway退出且全部worker已释放。Close发生observer/journal错误但进程/worker已真实清完时释放句柄并保留强错误；物理未完成仍保留精确句柄供重试，不以error等同进程仍存活。Hub把实际MCP Plugin加入既有attempt资源清单，失败/超时后由同一owner重试，不在活跃清理下关闭facts。
- **Browser完整收尾**：run context覆盖审批、prior activity与真实操作；Close先取消/停止manager，再等待Tool gate，包括仍可能写native facts的回调。等待超时保留原manager，CleanupComplete为false；实际退出后可重试。并发的旧cleanup不能清掉后来的manager；显式owner Close永久封闭实例。参数拒绝重复键/非法Unicode，CallID非空；run-owned私有profile、正常结束清理和不保留跨轮登录的策略不变。
- **Shell完整收尾**：审批前登记实际in-flight调用，owner/run取消到达审批、管理回调及操作；Close在真实job/group/lease清理之外，还等待所有已准入Tool及事实写入结束。后台stdin管理即使进程已退出也不能提前报告完整清理。新增计数不取得global/job lease，原后台跨轮策略和全局执行排他保持。参数拒绝重复键/非法Unicode及空CallID。

同ID借权、复制Tool调用、歧义JSON、MCP已reap但error仍保留句柄、Browser提前完成，以及Shell审批/后台stdin回调提前释放owner，均有行为反例；失败日志保留。新增本地真实模型HTTP→Hub装配→MCP stdio→耐久journal闭环，验证保存配置不启动、独立批准后按需启动、实际attempt资源注册和物理收尾。初版测试引用错MemorySession包、旧MCP测试允许同ID内存会话、Hub测试尚未通过独立启用API等测试问题均保留并纠正，不冒称新产品缺陷。

最终选定范围 **434项测试（含子测试）/15包race通过、0失败**：四个扩展完整包、harness、architecture及真实消费者实验、MCP/AEP协议、全部host包、process、Alva adapter与Hub bootstrap。唯一skip是独立运行时不带probe参数的durable helper入口；实际独立进程恢复实验已通过。真实本机Chrome测试无skip；本地fixtures无付费模型/外部账号。首轮8包domain因旧MCP测试使用MemorySession而整体exit 1，修正测试authority后上述完整最终范围通过，没有覆盖失败日志。

**8项构建/演示检查全部PASS**：SDK/codec/可选校验、重试、timer、Skill的js/wasm与WASIp1构建；macOS arm64、Linux amd64、Windows amd64所有desktop-tagged命令无cgo构建；真实WASIp1 worker构建、原生CLI及本机显式demo。全树desktop-tagged vet、gofmt、逐文件diff空白与保护文件哈希检查通过。本批WASM为构建复验，未重跑其运行时测试；不宣称全树race、跨OS实机或UI交互全量验收。外部Go回调仍须响应context；忽略取消时保留未完成ownership并返回deadline，不伪称可以强停任意Go代码。Hooks/诊断、Web/Media等其他外层能力的逐条深审继续留给后续批次。

本机证据目录`alva-execution-z9tos4im`保存批前源码、完整patch/哈希、red-corrected.jsonl、shell-red.jsonl、targeted-2.jsonl、shell-targeted.jsonl、final-tests.jsonl与结果、builds.json、vet-result.json、quality.json及result.json；未stage/commit/push，不删除旧源码/历史。

## 2026-10-18 L4工具与Plugin审查：独立能力与可替换服务

继续向外审查，先盘点18个扩展包的直接依赖与装配方式，本批深审Utility、Planning、Skills及真实消费者。结论是保留Tool/Plugin/Middleware三种贡献机制，把可复用Tool与原生产品装配分开，并把已有Service契约接入真实消费。没有合并抽象、新增Plugin种类或修改SDK/kernel源码；六份公开面快照及go.mod/go.sum逐字节未变。本批不宣称已逐行深审完MCP/AEP/浏览器/Shell等外部执行型扩展。

- **普通Tool可独立使用**：原Utility的sleep必须随SQLite、native会话/偏好恢复和Middleware一起构造。现在`extension/utility/timer.NewTool()`只依赖标准库与kernel ABI/wire/bus，独立SDK的内存会话可直接跑真实model/tool/model；原生Utility Plugin复用同一实现，默认工具名/顺序保留。纯Tool没有文件/网络/会话能力，ctx拥有等待的取消，Coordinator/ManagesOwnTimeout保持。不把源码子包算成新Plugin抽象或外部依赖，也不宣称整个原生Utility或产品CLI已去掉SQLite。
- **轻依赖由CI保证**：实际`go list -deps`确认timer无native/integration/SQLite/wazero或第三方依赖；architecture加入直接/传递边界与父Plugin/原生存储/外部SQLite三个反例，防止未来重新绑重。此改动提供可装配的WASM Tool，不自动增加现有WASIp1 worker的默认工具目录。
- **Planning现有Service真正可替换**：`Config.Service`选择提供者实例，Tool、Service()与typed bus消费同一实例，普通nil才构造默认nativeService，typed-nil拒绝。显式替换不恢复默认snapshot、不隐式双写或迁移旧业务状态；生命周期/并发/校验/业务持久化由提供者负责。实际native log、Guard、run/session/cwd与详情引用authority保持，不能借相同session字符串换实际会话。真实SDK回归验证自有Service收到Create、详情投影存到实际会话、没有默认planning state写入；默认坏快照仍fail-closed，显式替换可独立工作。外部状态不冒称耐久，提供者的输出引用须能由实际会话reader读取。
- **Planning投影与取消**：详情json.Marshal错误不再变成空成功artifact，编码正文≤128KiB；SetNotes返回引用须Validate。默认Notes读取消行为与Get/List一致。未把Service内的业务校验/存储实现迁到kernel，也没有引入统一存储总包。
- **Skill可替换仓库的边界**：工具自己校验ActualTool/CallID/name及固定session、单个≤4KiB UTF-8 JSON、非null字段和相对resource path，不依赖默认Harness的schema Plugin。typed-nil repository/reference在目录读取前拒绝；各读取/存储回调后检查取消。selected document/resource的来源身份、UTF-8、1MiB正文与有界元数据必须有效；错误引用不能进入结果。确定性坏参数保持IsError反馈，backend/能力/取消错误仍中止；实际Skill文件读取、二进制不自动投递及不执行脚本的语义保留。

Skill问题先以行为反例复现；Service接线缺口用批前源码overlay验收编译确认，业务替换随后通过真实SDK执行验证。投影编码错误和取消读也有批前overlay反例，未把初版测试代码的编译笔误当成产品行为缺陷。独立timer的依赖差异与运行探针分别保留。测试不是接口设计“唯一最优”的证明；外部Service仍须满足文档中的业务/归属契约。

最终 **780项测试（含子测试）/35包race通过**，覆盖SDK、全部extension、harness与architecture；真实宿主、Alva engine adapter及Hub bootstrap另 **95项/5包race通过**，共875项/40个互不重叠包。纯timer全包3项与Skill内存仓库边界14项，在 **js/wasm（本机Node）和WASIp1（既有wazero）各17项实际运行通过**；不把需要原生文件Root的Skill FS测试冒充WASM运行验收。

**8项构建/演示全部PASS**：SDK/codec/可选校验、重试、timer、Skill的两种WASM构建，macOS arm64/Linux amd64/Windows amd64所有desktop-tagged命令无cgo构建，实际WASIp1 worker、原生CLI及本地显式demo。全树desktop-tagged vet通过；gofmt、逐文件diff空白、SDK源码/六份公开面/模块文件哈希检查通过。本批没有重新执行整棵树的race命令，也不冒充跨OS实机UI或真实付费模型验收。

本机证据目录`alva-extensions-nfa3_atp`保存批前源码、源码差异/哈希、initial-skill-boundaries.jsonl、纠正后的initial-planning-api.jsonl和initial-planning-boundaries.jsonl、依赖前后清单、domain-tests-result.json、consumer-tests-result.json、portable-runtime-results.json、builds.json、quality.json与result.json。

## 2026-10-17 L3能力库与实际安全消费者审计

本批继续向外审查context、memory、security，重点验证组合、可替换实现、所有权和取消边界，并沿真实安全/子Agent消费者修复动态值比较。保留ContextHandle、独立memory backend、Guard/host resolver及Tool/Plugin/Middleware分工；无新增公开方法、生命周期、依赖或兼容层。kernel仅修正实际log值比较的内部缺陷，六份公开面快照与go.mod/go.sum未变。本批不冒称完成scope/graph或所有L4工具的独立审计。

- **context组合保留前层决定**：HooksChain先设置Critical、后续只改文本时，最终priority不再丢失；各层看到原message identity，最终message/priority脱离callback持有的指针。budget hooks各拿独立map/slice snapshot，不能改写原输入或下一政策的输入。真实SDK模型请求回归证明Critical消息经后续压缩仍保留，原始conversation facts不变。
- **外置能力不占工作集写锁**：InjectMessage、RewriteMessage、RewriteBatch、Externalize在锁外完成TokenCounter调用，再在锁内提交工作集；可重入的只读Store观察不再死锁。整批rewrite仍预验证并原子修改，外部化仍在提交前核对完整snapshot及ctx；不把任意callback当成可以强制停止或回滚的任务。
- **context回调真实结算**：summary回调返回后保留timeout/cancel，不再把迟到成功值替换进工作视图；外置memory facts Query返回后重查取消，迟到facts不继续交给调用方。外部化验证返回reference，保留后端原processing证据，取消时明确artifact可能已存、工作视图不替换。正在运行的callback仍须自己响应context。
- **memory检索政策固定**：每次Search在外部调用前捕获一份权重，零权重真正停用对应FTS或embedding/vector调用，避免混入无效零分候选及不必要的外部IO；权重总和overflow拒绝。入口、等待写锁后和检索/索引阶段间检查取消，取消后不启动下一阶段。已完成的backend写入不回滚；FileReplacer与非原子CRUD fallback的区别保留。
- **security缺失能力与迟到取消**：NewGuard拒绝typed-nil resolver，普通nil仍允许无文件能力recipe；typed-nil approval handler按不可用返回ErrApprovalRequired，两个nil函数adapter直接调用返回error。resolver返回后再次检查取消，返回空path并同时保留真实resolver错误；不冒称词法SDK提供OS沙箱。
- **动态值身份不panic**：Go struct的类型可比较，interface字段的当前值仍可能含map/slice。安全wrapper、SubAgent直接父授权、NativeChildFactory改查实际Value.Comparable，不能证明身份仍按Unknown审批或明确拒绝，不用内容相等猜授权。kernel日志去重同样修正；合法value log可在取消时真实Flush，不能在收尾panic。真实Agent回归验证无审批不执行、显式审批只执行一次。
- **验证中发现的QuickJS预算边界**：隔离race实际复现冷编译耗尽脚本10秒期限、合法脚本从未执行；同配置非race通过。修正为原始/计量代码编译受已有job总deadline约束，编译后再次检查job取消，再启动独立10秒实例初始化/执行期限。共享fuel/内存/授权/fresh实例不变；同步AOT仍可能延迟响应取消，不能冒称编译硬中断。CLI测试给冷启动三分钟预算，生产默认总期限仍为五分钟；无限循环测试先预热代码缓存再量执行超时，仍验证部分事实和后续Agent可用。

主要缺陷均保留修前反例与修后回归。独立审查完成security及真实消费者修复，并交叉复审context/memory和QuickJS期限收尾，未发现新增阻断。最终SDK能力/实际消费者/harness/architecture的 **351项测试（含子测试）/9包race通过**；CLI整包32项通过，sandbox host的61项分批通过（整包59项成功、2项冷启动预算失败，随后这两项及独立无限循环期限共3项补验全部成功）。本批相关11包共 **444个去重测试条目分批通过race**，不把重复执行次数累加成覆盖量。

四个SDK包的完整测试分别在 **js/wasm（本机Node）与WASIp1（既有wazero）各113项实际运行通过**：context 38、memory 17、security 26、kernel/core 32；不冒充浏览器或实机UI验收。**8项构建/演示全部PASS**：SDK/codec/可选参数校验及重试Plugin的两种WASM构建、macOS arm64/Linux amd64/Windows amd64所有desktop-tagged命令无cgo构建、实际WASIp1 worker、原生CLI及本地显式demo。最终全树desktop-tagged vet、gofmt与逐文件diff空白检查通过。

全树race首轮整体exit 1，明确不记为单次全树PASS：QuickJS脚本冷编译消耗执行期限，随后CLI及sandbox host因当次150秒包级测试总上限中断；其余76包通过。隔离race再次证明脚本未执行即独立期限耗尽，非race相同用例通过；修正期限归属后CLI整包通过。host整包后续发现两条fresh Runner SDK/fuel集成用例的自身一分钟job预算不足，最终只给这两条fixture三分钟冷启动预算并补验；tiny fuel、取消、fuel剩余/输出/恢复断言均保留。上述失败输出不覆盖，最后只补验受影响范围，没有宣称重新执行整棵树的一条命令全绿。

本机证据目录`alva-capabilities-4pwsrgaj`保留批前源码、完整差异、哈希清单与独立红绿证据；full-tests-result.json与cli-script-isolated-race-result.json记录首轮失败，final-domain-tests-result.json、final-wasm-consumers-result.json、final-host-cold-fuel-result.json、portable-runtime-results.json、builds.json、quality.json和result.json记录各阶段实际状态及最终覆盖范围。

## 2026-10-17 内核外一层：Agent ABI与装配审计

用户要求从内核向外继续review。本批覆盖`agent/abi`与`agent/core`的Plugin/Registrar/Builder/Finalize、配置所有权、取消和注册失败边界，并沿真实消费者修正Analytics、SubAgent、默认预算与Shell任务状态。保留现有分工；无新增公开方法、生命周期或总包，kernel源码、六份公开面快照及go.mod/go.sum逐字节未变。本批不宣称已完成context/memory/security等L3能力库的整层审计。

- **装配配方固定**：Build在贡献实现/回调前复制完整配方和贡献切片；回调通过原Builder新增插件或改loop/prompt/config仅用于下次Build。修复未Register插件被本次Finalize执行并索引越界、finalizer改写本次loop的问题。Builder仍仅同步使用，不承诺与setter并发安全。
- **取消与失败封闭**：Register/Finalize前后检查ctx，取消后不启动后续回调、不返回成功Agent；defer保证任意退出路径封闭已交出的Registrar/bus，包括panic展开，原panic仍传播。正在执行的回调须自己响应context，没有伪称可强制终止任意Go代码或统一资源回滚。
- **配置所有权**：ContextSystem在SDK/Harness setter和Registrar贡献时复制描述符，Run另复制context描述符与middleware切片，避免原struct或替换loop污染后续执行；Harness默认BudgetAdmission也绑定接受时的配置，不能在Build后通过原指针放大最终预算。Hooks/Handle/Apply/Counter等服务对象保持有意共享，内部状态及生命周期由实现负责。Plugin/Middleware/Command身份及Command说明拒绝非法UTF-8。
- **可执行贡献与通信注册**：CommunicationRegistry拒绝typed-nil，在锁外单次读取Kind，避免可重入Kind查询注册表导致死锁；ChildCompleteFunc(nil)返回error。SubAgent构造时拒绝typed-nil必需/可选能力，Attach返回的缺失hook或typed-nil callback在Factory.Prepare前拒绝；普通nil可选值仍有效。resolver的typed-nil模型、factory的typed-nil Runner/Session不会进入实际执行，无效已准备child仍走真实Close；factory/resolver函数适配器对nil返回error。
- **实际调用归属**：Analytics使用InvocationSessionID记录本轮模型调用，wrapper对State.Session的修改不再把A的模型事实误记为B；回归同时核对A/B实际模型intent/outcome，允许B拥有合法生命周期审计。
- **Task真实消费者所有权**：Shell List和job.snapshot统一复制Task.StartedAt/EndedAt、ExitCode指针，调用者修改返回值不再改动内部owner状态。修复位于Shell消费者，不给Task ABI增加复制接口。

上述问题均先用针对性反例复现；独立审查完成ABI/Shell修复，并复审Root的装配及实际消费边界，未发现剩余阻断。SDK/extension/harness/architecture完整选定范围以desktop标签通过 **706项测试（含子测试）/34包race、0失败**；这不是apps/integration/platform的全树运行重测。最终全树desktop-tagged vet通过。

装配新增19项、Agent ABI全包13项、Analytics全包5项，还分别在 **js/wasm（本机Node）与WASIp1（既有wazero）各37项实际运行通过**，不冒充浏览器或真实音频设备验收。**8项构建/演示检查全部PASS**：SDK/codec/可选参数校验及模型重试的两种WASM编译、macOS arm64/Linux amd64/Windows amd64全部desktop-tagged命令无cgo构建、实际WASIp1 worker、原生CLI构建及本机显式demo。Linux/Windows为编译验收，demo只用本地fixture，无付费模型调用。

本机证据目录`alva-agent-layer-1v1cbuiv`保留批前源码、全部差异/哈希清单、core-before.jsonl、domain-tests-result.json、portable-runtime-results.json、builds.json、vet-result.json和result.json。ABI/Shell先红后绿及独立审查日志来自本批审查者证据，汇入该目录供追查；失败输出保留。gofmt及逐文件diff空白检查通过，公开快照没有重写。

## 2026-10-17 通用内核扩展性实验

用户确认按“不同Agent形态的扩展成本”继续验证。新增`architecture/extensibility`独立测试消费者，仅通过公开SDK/原生存储接口实现四类小型实验，不加入产品默认装配。生产Go源码、SDK源码、六份公开面快照、go.mod/go.sum均与本批开始逐字节一致；不是为通过实验先加新hook。

| 实验 | 接入方式与实际验证 | 已确认限制 |
|---|---|---|
| 替代上下文策略 | Plugin贡献ContextSource，用独立索引选择历史事实和最新完整user/tool turn；不使用四层Store或CompressAction；原始历史保留，Source失败时Messages读取0次 | 成功路径仍调用ConversationReader.Messages，不宣称有界native恢复、向量检索或真实语义搜索 |
| 多Agent调度 | 外围graph调度两个独立SDK Agent，真实model→tool→model并行及确定性merge；各自会话隔离；工具观察取消后继续被测试闸门持有，验证调度等待真实退出 | 不是完整分布式团队/跨进程调度器；graph仍与AgentLoop平行 |
| 持久业务阶段恢复 | Plugin工具写独立namespace/schema的审批账本，外围coordinator通过现有native BeginRun取得执行所有权；独立子进程重开SQLite恢复，审批ID校验、重复恢复无追加；真实文件副作用后os.Exit(37)，恢复留unknown且不重放 | 恢复的是业务阶段，继续时新建Agent Run；不恢复Go调用栈，不承诺任意crash位置自动完成或exactly-once；applying已有成功结果但done尚未提交时仍保守要求reconciliation |
| 双向交互 | Plugin提供私有typed transport能力，可替换AgentLoop消费独立有界输入/输出；输出阻塞时仍实际接收下一输入，取消后join所有自建工作 | 单次请求LanguageModel仍是Builder必需项；替换loop不自动继承默认middleware、context、工具或耐久审计，fixture对不支持配置明确拒绝；帧是本地确定性字符串，不是实时音频产品 |

独立复审补强了反例：写工具用已有SerialGlobal避免同批消耗审批竞争，request工具不能重置approved/applying，applying绑定真实RunID/CallID，完成必须匹配成功耐久ToolOutcome。未获批准调用、同批重复effect、effect后重置、只回复完成而未执行四种不合作模型均通过同一Plugin验证；没有把合作模型脚本当作权限保证。原型和权限目录的错误在测试层修正，不放宽生产后端。

设计结论：现有SDK已能在外围表达上下文替换、独立Agent编排和持久业务状态，不能仅凭ABI存在具体context词汇就断言必须搬层。双向交互证明了接入能力，也暴露“可替换loop”与“低成本复用完整执行语义”之间的差距；后续若有正式实时runtime消费者，应优先审查模型调用、工具准备/提交/结算等执行组件如何复用，不先堆新hook、统一大事件类型或持久runtime。当前实验不据此提前改变稳定契约。

本批最终desktop-tagged `go test -race ./architecture/... -count=1` **173项测试（含子测试）/2包、0失败**，其中静态架构闸152项、实际消费者实验21项；子进程helper在父进程常规测试中明确skip，只由指定子进程运行。并发/取消/不合作模型场景独立重复10轮 **160个pass事件、0失败**，不与173累加。根架构静态闸继续不执行被扫描包，消费者实验在独立子包运行。

上下文、多Agent和双向实验还各在 **js/wasm（本机Node）13项、WASIp1（既有wazero）13项实际运行通过**；native跨进程SQLite实验有显式build约束排除这两个目标。WASM两项测试编译、`go vet ./architecture/...`、gofmt与逐文件diff空白检查通过。初次js wrapper因继承环境总长度超过Go WASM启动上限而未启动测试，改为显式PATH/TMPDIR/LANG的本地运行环境后通过，没有打印/传入模型账户或修改SDK。不是浏览器、音视频设备、真实模型或整个产品的新一轮验收。

本机证据目录`alva-extensibility-c1ztk_ds`保留批前文件、完整差异和哈希清单；initial-tests-result.json保留临时目录权限错误的首轮失败，js-runtime-result.json保留上述启动失败，final-tests-result.json、concurrency-repeat-result.json、js-runtime-minimal-env-result.json、wasi-runtime-result.json与result.json记录最终验收和限制。

## 2026-10-17 Middleware 装配精简

用户在竞品源码对照和实际调用探针后接受实施：Plugin继续作为统一贡献包，Tool/服务/context/Middleware均通过现有typed贡献接线；不拆出第二个执行系统，不减少必要的冻结、准入、取消和结算边界。

- 删除公开PhaseHandler执行注册、PhaseOrder七档映射和PhaseEffect四类标记；独立复审后继续删除没有生产注册/消费方的PhaseContribution、Registrar.Phase与快照Phases字段，不保留空metadata注册系统。agent ABI的Phase时间线词汇独立保留；外部Hooks直接贡献七个窄typed函数适配器。
- 删除无生产实现的MiddlewareConfigurer/MiddlewareContext以及Build阶段Configure调用。保留构造、Register与可选Finalize，依赖在实际需要的位置接线。
- 删除MiddlewarePriority及各实现的Priority方法。Builder.Middleware与Registrar.Middleware只用一个可选装配order，默认3000、同值稳定注册顺序；较小值先进入，after及返回反序。Harness同入口透传并复制order参数。已有native claim、Pending、Hooks、Analytics、schema校验和末端媒体的特殊顺序显式迁至注册处，媒体Finalize继续核对真实最内层位置；直接kernel按调用方提供的slice执行。
- 提供已有11个执行hook的单函数适配器；每个只实现一个可选接口，不挂空hook或透传wrapper。空函数/空或非法名字注册时失败，绕过Builder的直接无效调用也返回error。没有增加执行阶段、服务总包、排序DSL或依赖图。
- 保留PrepareTool与提交后检查、BeforeModel与最终AdmitModel的不同边界；continuation、冻结目录/参数、真正收尾和工具副作用语义保持。普通业务错误分类、Utility依赖拆分、Planning后端注入及显式范围读取不在本批冒充完成。

设计取舍：整数order把顺序决定放回装配处，仍需少数显式数值；不引入依赖排序DSL。11个函数入口是现有hook的便利写法，七个已有Hooks消费者，其余四个保持一致性，代价是公开面维护。独立复审未发现阻断问题，不以竞品对照或测试通过宣称唯一最优。

本批最终源码的SDK装配/Hooks/媒体/参数校验/Analytics/Harness/kernel/Agent ABI/architecture共 **414项测试（含子测试）/9包race、0失败**，其中architecture覆盖分形文档、依赖边界与六份公开面；kernel-abi、agent-abi、agent-core三份快照随有意契约变化显式更新，其余三份未变。原Rust实现、旧数据与已有Go会话均不删除或改写。

全树desktop-tagged race的其余 **1,391项测试/76包通过**，包含真实CLI→WASIp1 worker→QuickJS；该次命令整体exit 1：运行中删除旧phase_test.go使启动时的core测试文件清单失效，报`open sdk/agent/core/phase_test.go: no such file or directory`，不记为全树PASS。之后基于最终文件集完成上述9包race复验，并以desktop-tagged全树`go test -race -run '^$' ./...`重新编译所有测试通过；后者只算编译验收，不冒充全树运行重测。

最终 **8项构建/演示检查全部PASS**：SDK/codec/可选校验与重试Plugin的js/wasm、wasip1/wasm编译；macOS arm64、Linux amd64、Windows amd64全部desktop-tagged命令无cgo构建；实际WASIp1 worker、原生CLI构建及本机显式demo。Linux/Windows和浏览器仍是编译验收，demo明确使用本地fixture，不冒充实机UI或付费模型验证。最终全树desktop-tagged vet、go mod verify、gofmt与逐文件diff空白检查通过，go.mod/go.sum逐字节未变。

本机证据目录`alva-middleware-cleanup-so4duafq`保留批前源码、完整差异和哈希清单；full-tests-result.json记录上述非零退出，final-followup-result.json、final-test-build-result.json、builds.json、quality.json与result.json分别记录最终验收，失败输出不覆盖。

## 2026-10-17 历史：核心对抗审查后实施

用户接受三方独立及交叉审查结论并授权两批落实。本批有意识修改公开契约，快照作为审查记录；保留Tool/Plugin/Middleware和loop/graph/多引擎边界，不合并为durable runtime，不加兼容alias。

- 模型可见工具定义与实际实现固定同一invocation目录；注册/替换/删除下轮生效。ToolPreparationContext显式携带同一ActualTool，参数校验/效果/调度/执行不再从live registry二读。撤权仍在执行准入检查，目录删除不是完整撤销协议。
- model next逐层canonicalize真实model/State/RunID/workspace/bus；InvocationSessionID绑定本轮log，避免State.SetSession影响本次预算归属。Tool continuation每层及base零或一次，拒绝重复/并发且违规不可吞；保留真实join与迟到拒绝。
- Model continuation保留顺序多next的私有采样/缓冲重试；每层和base拒并发，每次actual Stream独立关闭callback。逻辑流公开任何chunk后，后续base或wrapper失败不能透明覆盖，包括“先公开固定前缀再私有探测”的保守限制。任意可信Go wrapper自行延迟/生产chunk的来源无法在现有签名下自动证明；未引入attempt selection或token rollback协议。
- 可选ModelAdmissionHook位于每次captured模型接口前，检查全部wrapper后的真实request；每个检查器独立副本，检查不能改发送值。拒绝不调用本次模型，未公开输出时可修改后重新准入。政策仍在外围；组合器内部opaque HTTP候选和缓存数据访问不由此自动覆盖。wrapper内部Tools/Config投影不自动改logical工具执行权限。
- sdk/agent/context.BudgetAdmission为真实外部消费者；Harness仅显式配置ContextSystem时按名默认安装context-budget Plugin，支持同名替换。检查最终完整输入估计（含工具和后置注入），不自动二次压缩；无Counter的JSON rune/4仅粗略估计，不是真实provider/多模态token或输出reservation保证。独立SDK需显式注册，未配有限预算不宣称硬限制。
- extension/modelretry提供显式opt-in重试Plugin：1..8次、1byte..8MiB编码buffer、1..65536 chunks及≤5分钟总期限，调用方显式提供Retryable。失败流私有、成功完整验证后才公开；取消、SessionError、buffer/协议错误和发布失败不重试。未默认装配，无provider fallback、backoff、持久尝试账本或工具重放，未声称统计失败尝试的完整计费。
- ContextHandle由24方法收为8个工作集方法；文件/记忆/rewrite/summary/externalize能力移至L3，DefaultApply显式接Summarizer；七hooks保留。Rules经公开Entries/AdvanceTurn而非断言具体Store；MemoryFact仍由Injection.Memory实际消费，不做无消费者迁移。
- Registrar.ContextSource为独立贡献，不覆盖ContextSystem。真正reader-only wrapper只读本轮会话；配置Source后错误不回退完整Messages，原ConversationLog完整语义不变。native-context-source Plugin提供固定高水位的近期完整user-turn视图，目标64条、最多512条/8MiB恢复预算；最新turn超限拒绝，较老turn可显式省略并给真实引用与“非摘要”说明，不冒充持久摘要/head或自动恢复。辅助conversation partial index减少非conversation事实的扫描，不改变schema版本或原事实。

本批剩余边界：SDK无Source仍可全历史读取。Source预算约束恢复的记录和正文，不是整个session heap的恒定上限；Store跨Run的旧RuntimeInject保留仍是已有内存欠账。持久inbox、完整native能力后端替换、正文政策统一与durable挂起/版本迁移没有在本批完成；无自动副作用重放。

本批验证按实际源码时点分别记录，不重复累加：全树desktop-tagged race checkpoint通过 **1,491项测试（含子测试）/76个含测试包、0失败**，包含真实CLI→WASIp1 worker→QuickJS及fuel/引用/持久结果回归。全树启动后加入的modelretry与最后核心收尾，另以SDK/Harness/modelretry/architecture **459项测试/16包race、0失败**验收，包含Context失败启动/复用和会话切换生命周期；最终architecture再通过 **152项测试**，覆盖分形文档、导入边界与六份公开面。kernel-abi、agent-core两份快照随本批契约显式更新，其余四份未变。

最终 **8项构建/演示检查全部PASS**：SDK/codec/可选校验与重试Plugin的js/wasm、wasip1/wasm编译；macOS arm64、Linux amd64、Windows amd64全部desktop-tagged命令无cgo构建；实际WASIp1 worker、原生CLI构建及本机显式demo。全树desktop-tagged vet、go mod verify、gofmt与逐文件diff空白检查通过，无新增模块依赖。Linux/Windows和浏览器仍为编译验收，不冒充对应实机、桌面视觉或付费模型测试。

本机证据目录`alva-invocation-cleanup-opkkn5k9`保留本批前文件、完整差异及哈希清单，full-tests-result.json、final-followup-result.json、final-architecture-result.json、builds.json和result.json分别记录验收阶段。

## 2026-10-17 历史：核心第一轮整顿

本批针对Pi durable逐项对照中实际复现的边界问题，不合并既有抽象，不移植Pi运行时。

- Native存储组件将批次条数改为`2+3*wire.MaxStreamContentBlocks`（当前12,290条），容纳完整模型响应和整轮准备失败的工具结算；每条取序列化大小与原始字符串/字节数的较大者，整批≤64MiB。检查在deep clone、脱敏和写入前执行，另有既存单条字节预算。容量属于后端政策，未向kernel发布新常量、预算函数或会话接口，也未给SDK MemorySession强加native容量限制。
- `sdk/kernel/abi/sessiontest.Run`为第三方可复用的PersistentSession测试支持，Memory和SQLite共用：全有/全无、身份/因果、拷贝所有权、意图/结果配对、未知扩展、exclusive序号游标和取消。容量、脱敏和落盘特性在具体后端单独验收，不假定所有实现具有相同政策。
- 核心运行时只修复AfterModel改写绕过wire既有内容块上限的问题；SDK六份公开契约快照与本批开始保持一致，不添加hook、setter或把外围能力写入kernel/agent-core。
- `extension/toolvalidation`经既有Plugin/PrepareTool/BeforeTool接入。native Harness按名默认安装，独立SDK用户可显式注册同一Plugin；SDK自身仍只依赖标准库。无效参数返回模型可恢复错误，准备阶段尽早拒绝，冻结后按实际Tool再验证，防止后续改参绕过；不包装/替换实际Tool身份。
- 校验器固定`santhosh-tekuri/jsonschema/v6 v6.0.3`，仅在扩展层；schema和instance都用json.Number，验证大整数边界、enum/const和十进制multipleOf而不改写原始参数。禁止外部schema资源（含本地file与网络），默认dialect固定2020-12并支持显式draft-07；schema≤1MiB、参数≤4MiB、cache≤128，数值字符/指数有明确工作量预算。非法schema在已知装配阶段失败，finalizer/动态替换贡献在运行或首次使用检查。
- native `Messages()`保留完整对话语义，按64MiB存储正文加每条512-byte估算开销预检，不再套用4096条完整records读取阈值。4097条小消息重开后可进入显式ContextSystem压缩；原历史保留。`Records()`的4096条预算、冷Inspector分页/检索、归属与hash验证不放松。

WASIp1 worker暂不默认接入完整schema引擎：真实fuel+QuickJS恢复回归在冷启动达到64.65秒，超过既有60秒job预算，已撤回该接线；保留guest既有逐工具严格参数校验，不提高deadline来掩盖回归。扩展可移植编译不等于默认沙箱预算已验收。

尚未完成：持久化摘要/head工作视图、默认token ContextSystem接线、最终模型请求预算检查、完整native后端能力替换、统一正文政策与持久inbox。这些不由本次解除消息条数限制或安装工具校验冒充完成。没有增加自动副作用重放。

本批验证：含desktop标签的全树race在最后一次政策归位前 **1,522个pass事件/76包/0失败**；按用户要求将预算收回native组件、恢复核心公开契约及MemorySession后，SDK/native session/校验Plugin/Harness/architecture再跑 **503个pass事件/17包/0失败**。六份SDK公开面快照，以及kernel ABI源文件与SDK内存后端，均与本批起点逐字节一致；核心实现仅保留AfterModel已有上限的审计修复。

最终 **8项构建/演示检查全部PASS**：SDK/codec/可选校验Plugin的js/wasm与wasip1/wasm编译、本机/Linux amd64/Windows amd64全部desktop-tagged命令无cgo构建、实际WASIp1 worker构建、原生CLI构建及显式本机demo。全树desktop-tagged vet、go mod verify、gofmt与逐文件diff空白检查通过。Linux/Windows和浏览器仍仅编译验收，不冒充这些目标实机或付费provider测试。

证据在本机临时目录`alva-core-cleanup-cjov_pwi`：pre-policy-move-tests.jsonl、policy-followup-tests.jsonl、core-contracts.json、builds.json、result.json与changes.patch。历史QuickJS与其他迁移checkpoint不代替本批验证。

## 2026-10-17 QuickJS-NG 升级至 v0.17.0（当前实现）

用户要求使用最新正式版，已将QuickJS-NG core从v0.15.1升级为v0.17.0。
上游源码归档SHA-256为`559bc4c420475e55c7ab4510adbc562f55d7524d75e8e89d79ce4bb02f5687d9`；
不再修改QuickJS源码，直接使用上游WASI栈保护与内存计量修复。原bridge自有allocator、
32MiB tracked heap、512KiB软C栈和2MiB linker栈reservation继续保留。

显式WASI SDK34构建出的资产为936,180字节（约0.89MiB），比上一版增加27,474字节，
仍在1MiB资产预算内；普通Go编译/运行仍不依赖C/Node/npm工具链。
二进制SHA-256为`ce29aaec478123bc26825288c43560cfdb6c70bf1802bfad1ef54ea77c7271d7`；
源码/bridge/bootstrap/编译参数和MIT许可随资产同步记录。

本批新版回归还包括Resizable ArrayBuffer缩小/扩展后的TypedArray.at、Iterator Helpers、
private fields/BigInt和FinalizationRegistry可用性；堆/输出超限不得用deadline冒充，递归软栈溢出须能在同一实例内catch后继续执行。
本批验证独立记录，不沿用下方旧版QuickJS/MoeJS结果：脚本/host与真实CLI→同SDK WASIp1
worker→QuickJS链路 **12项race回归/2包PASS，132.25秒**；完整architecture
**152个pass事件/1包race PASS**，包含导入边界、六份SDK公开面和分形文档闸。
两处独立临时目录重建的二进制、provenance及MIT许可逐字节一致。

**11项构建/静态/演示检查全部PASS**：macOS arm64、Linux amd64/arm64、Windows amd64/arm64
全部命令的无cgo/离线readonly构建；SDK/codec的js/wasm与wasip1/wasm；实际WASIp1 worker；
全树desktop-tagged vet；go mod verify；本机CLI demo。Python构建脚本语法、gofmt及diff检查通过。
本批未重跑全树race，Linux/Windows和浏览器目标仍为交叉编译；未冒充这些平台实机或付费模型验收。
本机证据目录`alva-quickjs-upgrade-ts1c99bo`保存源码归档、升级前文件、两次构建、
script-tests-result.json/architecture-result.json/builds.json和rebuild-result.json。

## 2026-10-17 历史：用户决定切回 QuickJS v0.15.1

用户在比较实际WASM体积后明确决定继续用QuickJS。本决定替代下方moejs试用选择。
恢复固定QuickJS-NG v0.15.1 core WASM、C桥接、trusted bootstrap和显式WASI SDK构建脚本，
删除活动树中的moejs执行器/Go reactor/资产以及go.mod/go.sum依赖，不保留并行实现或兼容alias。
moejs试用源码、资产和模块文件归档在本机本批证据目录；未删除Rust源码、Go构建缓存或用户数据。

当时资产908,706字节（约0.87MiB），对比moejs的12,968,100字节少约11.50MiB；这只是内嵌
脚本引擎大小，不是整套CLI/桌面产品体积。普通Go构建和用户运行不需要C/Node/npm工具链；
维护者只有重建该独立核心时才使用显式WASI SDK34 clang与checksum-pinned源码。

每次脚本fresh独立WASM，恢复默认wazero编译后端。64MiB实例reservation、32MiB跟踪heap、
512KiB soft C stack、10秒deadline、共享job fuel、原目录/fetch授权、实际取消收尾、错误恢复和
引用优先均保留。fuel回归先用真实WASM父计数器测量引擎启动前缀，再给循环明确的小额余量；要求观察到
JS循环入口、实际指令耗尽及owner取消，不用初始化耗尽或先到的wall-clock deadline冒充计量。原始源码、bridge/bootstrap和二进制SHA、MIT许可仍在scriptwasm记录。

用校验过的原WASI SDK34归档重新解包、重建后，得到与恢复前原版完全一致的SHA-256：
`ec852321ef08681fc2b664dc939125ca54cf04d3109d9e8a015d85b4f59b4a4f`。
本段仅记录当前切回实现，验证分开记录，不沿用下方moejs或旧QuickJS checkpoint。

本批最终验证：host脚本与真实CLI→SDK WASIp1 worker→QuickJS→文件/引用链路 **11项race回归/2包PASS，130.89秒**；
完整architecture **152个pass事件/1包race PASS**，含六份SDK公开面、分形地图和导入/源码出处闸。
**11项构建/静态/演示检查全部PASS**：macOS arm64、Linux amd64/arm64、Windows amd64/arm64全部命令；
SDK/codec的js/wasm和wasip1/wasm；实际WASIp1 worker；全树desktop-tagged vet；go mod verify；本机CLI demo。
gofmt无未格式化文件，活动Go源码与模块清单不再引用moejs或已移除的portable script包。
证据在本机临时目录 `alva-quickjs-restore-gvfrg5uo` 的final-script-domain-result.json/builds.json/rebuild.json。
本批未重跑全树race；Linux/Windows及浏览器目标仍是编译验证，未冒充这些平台实机或付费模型验收。

## 2026-10-17 历史：moejs 试用（已由上方 QuickJS 决定替代）

用户为简化同一CLI跨原生、js/wasm、wasip1/wasm的构建与维护，决定替换QuickJS。
现固定 `github.com/Calcium-Ion/moejs v0.1.0-alpha.7`，不修改上游源码，不引入Magpie应用/Bun。
`platform/sandbox/script` 提供fresh strict全局脚本、有限结果与显式callback；只有stdlib/moejs依赖，
不拥有文件/网络/凭据/进程或module resolver，SDK公开面不变。`host/scriptwasm`使用同一份代码
构建独立Go WASM reactor，普通Go构建和显式引擎重建都不再需要C/WASI SDK。

`--wasm-scripts`仍默认关闭。每个脚本自己的Go runtime/内存独立于Agent worker；host继续重审
原job路径/写入/fetch授权，完整结果引用优先，10秒脚本deadline/64MiB linear-memory reservation、
总job预算、真实取消收尾与共享fuel均保留。脚本错误可恢复；总job取消或fuel耗尽仍终止job。
引擎内部资源约束改为近似累计32MiB JS分配预算与512帧递归限制，不能声称旧QuickJS heap/C栈语义。
Go WASM引擎约12.4MiB（旧QuickJS约0.87MiB）；host为该独立脚本实例使用wazero解释后端，
避免大型Go解释器/编译器的native JIT冷编译消耗短脚本deadline。原生调用方直接执行纯Go库。

旧QuickJS活动资产与C构建链已替换，旧引擎文件在本机本批证据目录归档，未删除Rust实现、
Go构建缓存或用户数据。Apache-2.0原文、模块h1、Go构建命令、本地源码/二进制SHA均保存在
`platform/sandbox/host/scriptwasm`；同源码重建与版本/出处闸单独验证。
本节保留当时moejs试用的验证记录；对应活动实现已由上方QuickJS决定替代。

本批验证：脚本执行器/真实WASM/实际CLI与相关架构回归 **72个pass事件（含子测试）/4包race PASS，263.36秒**。
最后移除未使用的旧中断import并让FIFO测试区分实际启动与阻塞后，原生执行器+完整architecture
**155个pass事件/2包race PASS**，最终host FIFO/模块出处 **2项race PASS**。
**12项构建/静态/演示检查全部PASS**：macOS arm64、Linux amd64/arm64、Windows amd64/arm64
全部命令及portable脚本；SDK/codec/script的js/wasm和wasip1/wasm、实际WASIp1 worker、
全树desktop-tagged vet、WASIp1脚本/bridge vet、go mod verify与本机CLI demo。
相同源码/Go工具链重建得到相同SHA-256 `f2f5e3de2f0ad6933a3b6eeb3b2e0903f39810816f2f79a2e08cfa26eae46809`；
源码及资产出处闸、六份SDK公开面、gofmt和分形地图全部通过。
证据在本机临时目录 `alva-moejs-final-k9fyn9ba` 的script-domain-result.json/builds.json/rebuild.json。
本批没有重跑全树race；浏览器、Linux/Windows目标是编译验证，没有冒充对应实机运行或付费模型验收。

## 2026-10-17 吸收 Magpie，替换 RelayKit

用户明确决定停用 RelayKit，直接吸收 Magpie 的协议转换。现使用 `yetone/magpie` 的
`e085d6e9262786815c0f6c5ff65c5a585916cc9e` MIT 源码切片，位于
`integration/llm/codec/internal/magpie`，完整许可、原文件 SHA/保留声明和本地修改见其 provenance.json。
实际请求 parser/builders、IR、完整响应与流式 encoders 已接入原 Exchange/Gateway；原生同协议保留
自己的薄适配/opaque state。响应只有一套 native 解码权威，Magpie 消费已验证的内容/顺序/用量。
RelayKit module/sum 与全局 hook 已移除，未引入完整 magpie 应用、账号发现、自动重试/路由或工具执行。

新增实际 custom tool/freeform、namespace 与 client tool search 的非流式/流式逆向映射，
不同请求同名工具状态隔离、扁平名冲突拒绝、交错并行工具参数/顺序、字面 think/call 文本、
未知工具/媒体/控制、精确 thinking budget、schema 保真及大完整响应不误套 SSE event 限额回归。
请求无法保留的 intent/约束显式 LossError；源签名/加密续传、cache presence、Gemini 用量、
实际终态、取消与输出预算继续独立验证。首文本可增量；工具及其后内容校验/排序后发送。

验证分开记录，不用先前 checkpoint 覆盖最终新增：全树 `go test -json -mod=readonly -race -p 1 -tags desktop ./... -count=1`
通过 **1381 个 pass 事件（含子测试）/75 个测试包，0 failure，607.79 秒**；worker 与私有 Magpie 两包没有测试文件，
不是跳过了测试用例。随后补入严格 grammar/加密 schema、字面业务值、Gemini 无调用 ID 的历史只解析/绑定一次、
缓冲与派生体积检查后，最终 codec/provider/Gateway/architecture **409 个 pass 事件/4 包 race PASS**。
最终源码的 **10 项构建/静态检查全部 PASS**：macOS arm64、Linux amd64/arm64、Windows amd64/arm64 的全部命令；
SDK/codec 的 js/wasm、wasip1/wasm；实际 WASIp1 worker；全树 desktop-tagged vet；go mod verify。
全部 gofmt、分形/导入/六份公开面快照与 Magpie 源码归属/哈希闸均通过，SDK 公开面未变。
证据位于本机临时目录 `alva-magpie-final-dm7c2j7p` 的 result.json、final-domain.jsonl 与 final-builds.json。
实际账号/付费模型和 Windows 实机没有在本批联调；交叉构建不作为实机验收。
下方 RelayKit 验证段落属于替换前的历史 checkpoint，不作为 Magpie 当前实现的验收证据。

## 2026-10-17 原生演示、侧栏终态与 Phase 审计续审

本批实际打开本机独立临时demo bundle/data，通过系统原生AX控件完成中文会话标题、创建、composer编辑、Send、Inspector分页、工具outcome原文读取与私有JSONL导出；没有使用debug action代替这些点击。重启修复后的bundle并复用同一Go数据目录，冷恢复仍为原3行；第二轮实际Send后6行、product history sequence14、无active run，同一native session的工具记录从seq12到seq35。23-record/25696-byte JSONL的complete footer及SHA256已核对，未删除或重写旧导出。窗口留给用户继续操作，本地fixture标明demo且不调用付费账户。

- **P2：正文已完成，侧栏永久显示running**。真实AX演示复现。列表页是固定快照、独立poll只刷新selected detail；现按同session/UpdatedAt的新旧关系投影真实状态到按钮和AX行标签，不改列表成员、排序、cursor或触发新查询。修前四种终态反例失败，修后completed/failed/cancelled/interrupted、较新列表优先和foreign detail隔离均通过；新原生窗口第二轮侧栏与正文同时completed。
- **P2：Phase适配器虚构其他阶段的审计调用**。修前只注册AfterModel的handler实际执行一次，却在多个阶段的middleware_hook_batch出现；原adapter暴露所有可选hook及透传wrapper造成。现各私有adapter只暴露目标hook，共用原identity/priority；不改变公开PhaseHandler/Registrar/Phase类型或kernel。真实model/tool/model回归核对十个可执行hook（模型相关2次、工具相关1次、start/input/end各1次），并核对单AfterModel只审计after_model、mutation仍真实落盘。私有receiver变更被公开面扫描器记录，已审核只更新agent-core快照，其他五份不变。
- **Rust对照范围明确收窄**：核对`crates/alva-agent-abi/src/phase.rs`、`crates/alva-agent-core/src/extension/phase.rs`及kernel shared priority。七个order tier及0/1000/1500/2000/3000/5000/6000优先级一致；原实际handler为run/input/end、before/after模型、before/after工具，加BeforeTool的显式wrap分支。Go以typed hooks提供对应能力，另已有AroundModel/PrepareTool。枚举中的其他timeline不等于已实现handler；完整runtime timeline observation仍单独验收，未因此标为完成。PhaseEffect在原实际adapter也是描述元数据，不凭Observe的注释吞掉真实执行错误。

验证：SDK/harness、Hooks/AEP、Alva adapter、Hub bootstrap、两个desktop与architecture共529项/21包race PASS；最后新增的十阶段真实调用链与其余Phase回归5/5 race PASS。对应包vet通过。macOS arm64、Linux amd64/arm64、Windows amd64/arm64全部命令，Windows两架构SDK/Hub UI test binary、js/wasm与wasip1/wasm SDK/codec及同SDK WASIp1 worker共12项readonly/offline/cgo0构建PASS。这里没有重报一次最新全树race，上一批全树checkpoint仍独立保留。

本机演示证据位于OS临时目录`alva-go-live-demo-26y940cm/demo-result.json`，当前Build的`demo-chat-updated.png`与`demo-inspector.png`为实际Window.Content光栅。系统AX的以上基础路径已实测通过；CUA compositor只返回小型倾斜thumbnail，完整系统视觉/AX/IME/声学验收仍未通过。Windows console/pinned shell/worktree权限与实机/外部账户剩余项仍保留，不用本次演示替代。

## 2026-10-17 Windows 适配与 SDK 入参续审

本批继续实际代码，不把Windows交叉编译计作Windows运行：

- **媒体Unix fd3缺口已补Windows路径**：process.Config增加显式StdinFile，子进程实际可seek但只有read权限，Process.Stdin.Close不关调用方文件，不使用copy goroutine。Windows媒体snapshot使用CREATE_NEW/delete-on-close和同对象只读duplicate，没有path reopen；ffmpeg走原生handle:输入，保持seek，输入whitelist为handle/pipe。Unix继续原fd3。调用方配置的Windows解码器必须支持[FFmpeg官方handle protocol](https://ffmpeg.org/ffmpeg-protocols.html#handle)，缺失明确失败，不下载/发现或回退原件路径。新helper验证seek/拒写/最后handle回收，Windows实际测试纳入CI。
- **Windows WASI/脚本文件适配已补代码**：普通读写复用Go1.27[os.Root](https://pkg.go.dev/os#Root)；NOFOLLOW先固定Root中的parent，再NtCreateFile只开单leaf的OPEN_REPARSE_POINT，读取同handle属性并拒绝reparse后才truncate，避开Lstat→follow/truncate竞态；append/truncate通过最终append-only access保证Seek后仍append。不依赖仅新Windows支持的OBJ_DONT_REPARSE，也不使用未受限DirMount。device/ADS/escape与in-root symlink不截断目标有Windows回归；native执行未验收。
- **P1：WASI create intent扩大了RDONLY返回descriptor的数据写权限**：将“需目录mutation授权”和“返回file可写”分开；RDONLY|CREATE可以创建但后续Write/Truncate仍EBADF、不能消耗成功写budget。append file的Pwrite明确EINVAL，不能忽略调用方offset后声称成功。真实Root反例已运行。
- **Windows direct/Hub CLI file/pipe已接线**：新增独立OS lineinput primitive，PeekNamedPipe可读才ReadFile、逐字节不readahead，取消不关borrowed handle。保留各CLI的32KiB/64KiB行与各自累计/审批政策；实际继承fd0的no-EOF取消后续读、两行/Unicode/CRLF、overflow/EOF partial已编写Windows helper。**P1**修复Hub Windows依靠Close中断同步Scanner的潜在无期限等待。Console cooked read和pinned-directory host shell继续明确拒绝，未用CancelSynchronousIo或普通cwd path冒充已验证能力。
- **P1：SDK typed nil interface绕过fail-fast**：修前实际反例中nil function model仍Build成功，Plugin.Register被调用一次；已修model/session/loop/plugin/tool/middleware/context/phase，以及Provider factory和运行期state替换。空log ID也在Plugin前验证，失败替换不改现有state/version。harness同名默认替换前也拒绝typed nil；直接kernel loop在所有lifecycle/emit前校验zero state、nil/typed-nil middleware与Context；Context全部入口（包括Bootstrap失败后的Dispose）统一检查required/optional executable，修前真实panic与hook副作用反例、修后7项已通过。都是现有契约验证，没有新公开SDK符号或功能政策；公开面闸会同时记录imports，新增reflect这一行已经显式审核并更新agent-core快照。

[Microsoft NtCreateFile](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile)及当前固定Go1.27.2的Windows Root/openat实现作为文件句柄行为参考；Windows原生测试与本机Mac实际回归分别记录。并发全树检查出现两项metered WASM冷编译超时（model calls=0/fuel未消耗）。停止其他编译后，用`-race -p1`分别执行同两个实际SDK/CLI用例，2/2 PASS；生产deadline保持原值。CI改package -p1，包内真实并发/race与所有用例不变；随后串行检查发现新增reflect import需同步公开快照，核实只有该标准库import的一行差异后显式更新；以上失败记录保留。

本批最终验证（不同轮次不重复累加）：

- 全树 `go test -mod=readonly -race -p 1 -tags desktop ./... -count=1` checkpoint整体PASS：1345项（含subtests）、75个含测试包，另1个worker命令包无测试；JSON记录0失败、0测试skip。旧记录的76 packages口径包含该无测试包。
- 最后补齐的kernel清理/preflight与Windows行边界在上述全树启动后完成，另对最终源码重跑SDK/harness/architecture/host/direct/gateway/Alva adapter，共476项/19包race PASS，包含新的7项直接kernel反例；不把checkpoint冒充所有最终源码的第二次全树执行。
- 最终源码的22项readonly/offline/cgo0构建与静态检查全部PASS：macOS arm64、Linux amd64/arm64、Windows amd64/arm64全部命令；Windows两架构各6个原生test binary及vet；SDK/codec的js/wasm、wasip1/wasm与同SDK WASIp1 worker。Windows行上限包含实际继承stdin的精确maximum内容+LF、未EOF overflow反例，原生测试只编译、未在本机执行。
- 最终全树vet、go mod verify、gofmt、六份SDK公开快照/架构/分形闸和tracked diff whitespace通过。无新增module依赖或SDK公开符号，未改变生产deadline、放宽授权或删除旧数据。非macOS二进制未在本机执行，CI声明也不计作原生运行。

## 2026-10-17 续批深审与补齐

本批核对当前Rust实际源码（`alva-app-gpui`、`alva-hub-gpui`及独立engine契约），不把历史Tauri目录里的构建残留当作当前产品源码；Tauri退役发生于本次工作之前，本次未删除旧树。旧GPUI迁移文档曾收缩到Hub/macOS；最新ADR-0012与根AGENTS仍要求Go direct与Hub分离，本批按最新约束补齐独立入口。

| 深审发现 | 已完成改动与实证 |
|---|---|
| P1 功能缺项：Windows私有存储/进程所有权路径只有拒绝分支 | 新增current-user SID protected DACL、同handle身份验证、独占LockFileEx；CreateProcessW创建时JOB_LIST原子归属、精确stdio HANDLE_LIST、kill-on-close与实际job清空/reap。Windows private token/API key file只读OPEN_EXISTING、缺失不创建。原生测试已写入CI并交叉编译，未声称在本机运行Windows |
| P1：窗口state已经变更，CapturePage仍可能返回上次Build的旧首页 | apps/debug的rendered/changed屏障只接受当前已完成Build；dirty/未Build返回frame_pending并请求重绘。实际Hub窗口已取得当前Inspector的443933-byte PNG；这是own Window.Content光栅，不是compositor/AX截图 |
| P1：Hub观察靠Build触发，遮挡/不重绘时可能停在Running | 独立250ms窗口观察时钟，至多一个排队UI closure；关闭仅detach，真实product runtime继续拥有执行。coalescing/取消/无迟到查询有回归 |
| P1 功能缺项：direct desktop只有内存foundation | apps/direct持有独立native owner、冷会话列表/标题/恢复、真实host Controller、原件附件引用、once HITL、bounded历史/Inspector/2KiB读取/JSONL。--data-dir显式启用，--session仅选择、不执行；默认无data-dir仍明确易失。不同workspace/route不得改绑，Close超时保留facts锁以待真实清理 |
| P2 功能缺项：执行之外的EngineInspector/Descriptor独立抽象未迁入 | engine根契约保持stdlib-only，新增bounded metadata/reference/typed child/live/export与保守声明。Alva RepositoryInspector真正消费native事实，Hub Inspector/导出改走registered Inspector；typed link和causal index双验证；外部engine明确unsupported，不借Alva记录。第五engine冷注册、不可替换、声明切片隔离、观察detach不释放actual run claim均有回归 |
| P2 功能缺项：原Alva/Claude每次请求的system prompt与max turns被统一拒绝 | 显式option-aware Alva factory将两字段交给实际harness；普通factory保守拒绝。Claude实际argv使用append-system-prompt/max-turns，保持dontAsk/safe-mode，真实helper验证max-turns耗尽不是成功。0为默认、非默认1..1024轮、system prompt≤8KiB；local HTTP验证真正system role。Codex/Pi旧代码拼user文本的伪system路径不迁入 |
| P2 功能缺项：Inspector分类/JSONL导出与原生文件动作缺失 | 两UI有LLM/Tools/Middleware/Other当前页分类；同一native编码器固定各session高水位，8MiB/2048records/32sessions/8hop，超限/坏hash无partial success。native仅私有新文件，HTTP仅下载，原件/原record哈希不改，未知JSON数字精度保留，不抓引用原件或restore。operation ID重复不能多写文件 |
| P2：借用Windows Unix fd号可能变成错误HANDLE，或密钥占用REPL/stdin MCP | Windows明确拒绝Unix key/token fd；增加existing private只读--api-key-file/--token-file，不替换prompt/MCP stdio。真实继承stdin的EOF/cancel/overflow与cold startup有回归 |

原生手动脚本为`architecture/native_window_qa.py --run --app hub|direct`，仅自建临时demo bundle/data。Hub和direct均在真实窗口线程经过Create→本次Send/真实terminal→持久化Inspector→private JSONL导出，同operation重试只有一个文件；不从一个初始Idle状态猜“已完成”。先前抓到旧首页的无效探针已明确标记acceptance=false，不能作为验收证据。

本批最终验证：`go test -mod=readonly -race -tags desktop ./... -count=1`全部76个测试包PASS，包含新增的3个真实request-options回归；此前同命令完整checkpoint为1332 tests /76packages PASS。随后engine/Hub typed-nil防护的全部受影响packages另跑123 tests /7packages PASS。macOS arm64、Windows amd64/arm64全部命令readonly/offline/cgo0构建通过；Windows native11个test binary以及最新engine/Hub5个test binary交叉编译与Windows vet通过，均不作为实际Windows运行。Go vet、go mod verify、gofmt与六份SDK公开面/架构/分形文档闸通过。SDK+codec的js/wasm、wasip1/wasm以及同SDK WASIp1 worker本批重新构建通过。

本批有效Hub结果：`alva-native-window-qa-lppsztlv/result.json`（OS临时目录），导出25535bytes、当前Inspector PNG443933bytes；direct结果`alva-native-window-qa-i_p9r3f5/result.json`，导出25723bytes、4条消息、正确拒绝未Build的旧scene。两者finally均正常回收自建窗口/进程。LaunchServices/ad-hoc签名仅作用于自建bundle，CUA仍返回cgWindowNotFound -10005；未修改系统权限，也未将own-content PNG当作compositor/AX验收。

EngineInspector只拥有观察，不合并Runtime执行或会话删除职责；本批未执行任何用户历史删除。声明/observer构造不发现外部程序、不读取账户或启动模型。`GET /v1/engines`冷能力与`/inspector-events` durable SSE均通过实际HTTP回归；流断线只退订，不Stop，不重放已有Turn。

## 逐层状态（持续更新，不代表全量迁移完成）

| 范围 | 本批目标 | 对等验收与后续边界 |
|---|---|---|
| L0 wire/bus | typed 值、完整媒体/来源、思考/签名/拒绝/用量、typed topic/state、两阶段 bus plugin | 已有本地回归；仍须逐项对照原公开面和全部适配器 fixtures |
| kernel ABI/core | 窄日志、实际共享 AgentState、资源冲突并行、有界事件、context生命周期、可替换 loop、锁/检测/超时 | pre-intent重新算effect/scheduling、原声明/有效intent分离、失败原子结算；真实MessageCommitted在成功提交后发出，不用delta猜最终稿。InvocationModel在全部BeforeModel后固定本次实际模型，保留多next语义但并发SetModel不重定向当前请求；反例/race通过，versioned cache-base/effective schema/prepared system指纹与无正文首输出/stream summary已合批落盘；真实cache presence区分hit/write/miss/unreported，malformed后置schema不生成假hash；完整原公开面仍核对 |
| agent ABI/core | Plugin/Registrar/晚期贡献、真实装配快照、同名默认替换、typed ModelConfig、Task/Scope/Spawn/黑板 | 实际provider/credential registry、token estimator、typed analytics与有界sink已有回归；SDK已有真实语义/反例测试，完整公开面逐项核对仍未宣告结束 |
| 能力库 | context四层/投影/检索/host-bound文件导入与原子rewrite、FTS/向量memory、Guard/HITL、平行graph BSP/checkpoint/retry | 持久SQLite FTS5/vector/cache、受控MEMORY.md扫描/显式笔记提取、同名memory Plugin真实闭环；Rust基线也没有自动LLM提取/网络embedding实现（只有提取文件写入与Embedding trait/Noop）；这些归新增能力，非已实现功能迁移缺口；完整原反例继续补 |
| Provider/Codec | Chat/Responses/Anthropic/Gemini 四种真实 HTTP/SSE、函数与思考续传、用量、显式媒体编码 | 已按本批决定移除 RelayKit、吸收 pinned MIT Magpie 转换源码；同协议保留原始状态，跨协议实际 parser/IR/build 与 renderer/encoder，损耗/精度/cache/源终态门禁及 custom/namespace/tool search 往返另验。此前 32 路基线为历史，新实现四协议矩阵与新增反例已通过本地回归；无付费账户验收，完整原 fixtures 对照仍未结束 |
| native root runtime | bounded journal/订阅、运行所有权、Stop/审批、SQLite 原子事实与恢复 | 重启续聊、原事实不重放、失败不假成功已有回归；Windows DACL/LockFileEx/创建时Job树已落地并交叉编译，真实Windows运行及非Unix专用adapter另验 |
| Go 自绘桌面 | direct harness与Hub独立入口；首页/会话/上传/权限、虚拟消息/Markdown/模块fallback、Skills/设置/Inspector/Team pane | direct持久化会话/原件附件/once审批/历史与Inspector已补；Hub独立产品保留typed child Inspector/子审批、静音派生视频显式Play/Pause/seek、automation基础CRUD、managed worktree差异/二次确认采纳已有真实MyGo Tester闭环；六类可选工具的真实下一Turn设置/独立同意、跨pane真人问题与最多8个共享owner窗口已接；基础heading/status/list label已补，完整AX/OS验收仍缺。OS事件循环与自绘窗口可见日志已观察，CUA找不到可截图/操作surface，不能算真实OS视觉验收通过 |
| Skill | 完整YAML、显式根、三级按需读、受管写/归档恢复、懒加载Plugin | 产品管理已接saved cwd；祖先symlink/pinned root反例通过。不下载/执行或扫描全局来源 |
| MCP | owned stdio worker、单gateway、HITL后选server懒启动、完整脱敏内存/文件日志 | 子/孙按直接父实际授权重绑独立worker/journal已实测，不授父日志；无JSON-path/子AI。产品能力全表和平台回归继续 |
| AEP | 安全bundle与pin、双向协议、七真实hooks、过期state handles、单gateway/完整脱敏日志 | 真实Python SDK/双向IO/改参/参数结果/race通过；next-turn独立批准/原pin改动拒绝、失败握手可Inspector、factory/初始观察失败也真实cleanup已闭环。CleanupComplete分离物理回收与不能吞掉的journal错误；健康peer的有界shutdown/ACK/尾输出drain/真实reap已接，ack后不退出/不ack/observer失败均有反例；Windows私有snapshot已接真实DACL/handle检查；未在本机运行Windows反例 |
| ACP/重外挂 | 标准ACP v1新实现；保留实际扩展职责 | 旧deprecated自定义ACP不迁兼容parser；新标准v1客户端+pinned官方schema/真实helper、取消/late callback/独立observer/stderr framing已通过连跑。Hooks七阶段真实改参/输出与tooling诊断均产品next-turn接线；旧LSP为deprecated Stub且明确无实际server I/O，不复制假零诊断；完整LSP归新增能力 |
| Browser | 原七Tool、owned CDP/多实例/导航/操作/快照/截图 | 独立私有Chrome profile实际跑通七工具、Guard/risk/ref、跨实例隔离、取消/晚callback/物理reap+profile清理；native产品next-turn独立批准/真实配方与HTTP仅预存开关均已接。CDP库传输上限256MiB，页面DOM/Chrome资源无硬配额，不冒称完整敌对网页隔离；无用户现有browser/profile发现 |
| Shell/Web/Planning | 真实owned前后台Shell/stdin/output/kill、明确网络HTTP/搜索、typed任务/Plan | 四包race/vet及独立交叉审查通过；shell实际global lease持到physical reap，Finalize防后注册替换；Web完整来源引用与DNS pin、逐跳私网拒绝，搜索明确仅Instant Answers；Planning单事实CAS/DAG/状态/notes。不移植假成功plan-mode/LSP/tool_search stub。Hub原生disabled JSON编辑/独立Shell host及Web network同意/HTTP仅预存enable均已接到实际配方；真实model→Tool→durable结果回归通过。原Rust plan-mode Tool也只返回stub文本，不冒充真实政策切换；Shell应用内跨Turn Sessions owner、当轮generation Lease、fresh Guard/reporter、真实Stop/Close、restart unknown不接管PID已补。SDK global lease与actual Hub workspace lease都保留到physical reap，waiting writer不造成二次reader自锁；原生/认证HTTP均能冷查/Stop/idle reset |
| Notebook/Interaction/Utility | 真实受管JSON cell编辑、独立问题broker、timer/实际工具目录/无害响应偏好 | Notebook pinned祖先与SHA/mode冲突校验、不执行代码；问题与HITL分离、长回答同会话引用；工具搜索读取实际registry与selected schema，偏好CAS有下一request消费者。真实race/交叉审查通过；三者已接原生设置/HTTP预存开关与实际产品配方；真人问题按actual root/typed descendant授权，跨产品/过期问题拒绝，不借用HITL权限 |
| 媒体 | 原件引用、显式probe/图片/真实视频帧/静音转码、选中图单请求与回执 | stdPNG/JPEG/GIF+显式ffmpeg WebP/MP4/MOV/WebM、真实PTS/伪造metadata反例、源码/产物SHA与损失已验。Tool原始image/audio/video DTO全部先引用；选图绑定InvocationModel且强制最内层middleware，outer retry拿不到图。产品native实际HTTP/SDK/重启零重放、冷metadata/显式JPEG预览与HTTP范围读取已验。已验证静音MP4可显式5fps名义clock播放/seek，不冒充原始PTS或语义理解；Processor/Player实际owner取消、等待全局预算/真实reap与Close重试均有反例。显式首音轨≤60秒mono16kHz PCM WAV派生、actual sample时长/SHA与canonical校验、原生MyGo voice的Play/Pause/seek/导航close已接；声音输出失败不报可听成功，headless控件以不发声test voice回归。真实声学验收、decoder CPU-RSS硬隔离/付费视觉验收仍缺 |
| 原始与产品存储 | native records/artifacts 单一权威；Hub 独立 product SQLite；固定高水位冷分页/搜索 | 10k 记录、损坏正文不污染冷 Inspector、跨 owner/session 拒绝、崩溃未知执行不重放已有回归；完整产品 schema 对照未结束 |
| Engine/Hub | Alva/Claude/Codex/Pi、真实Pi持久恢复、可逆branch/fallback/handoff、Worker独立会话/FIFO/claims/生命周期 | Team真实终态回执/崩溃未知不重放、SubAgent独立native/HITL/typed child Inspector已闭环；handoff同事务TeamSnapshot保留≤16Worker/队列/真实结果索引，Worker只self，16KiB明确省略老Turn，known凭据脱敏且重开不重写。native/HTTP读取exact保存快照。Worker附件已用原会话/分支metadata+refs入FIFO，16KiB scope+manifest超限保留草稿，queue admission/claim都固定actual branch，停驻head不预约其他Worker资源；原生Send与HTTP显式include均有实际回归，模型不自动拿原件；managed worktree已有真实Git/SDK隔离、持久SHA/path/old-new证据与双workspace租约下显式literal采纳，不执行git apply/filter/hook。现有Lead普通权限保持，新文件owner-only；失败unknown、原件可检查但不自动回滚。worktree HTTP已复用相同product服务并实测foreign/stale/显式literal采纳；额外补Rust已有的discard产品操作：whole-tree/index/HEAD审阅、独立删除确认、registered path/branch/common repo重验、真实Git退出/路径与registration消失、retire保留branch/history/evidence，native cwd不retarget，crash/失败unknown不重放。Rust没有多轮rebase/证据归档/人工恢复UI，归后续增强；完整ACL仍缺；不是原子fchdir/OS hash-CAS或同UID敌对沙箱 |
| HTTP/诊断/自动化 | 同源apps/hub/product+apps/hub/bootstrap的认证loopback HTTP/SSE与独立模型Gateway | session/turn/stop、路由/真实子审批/typed child Inspector、Team显式billing/FIFO/生命周期、原字节上传、Skills与预存进程配置启停、durable SSE及媒体冷预览已有真实race/交叉审查。计划默认暂停/乐观revision/明确计费/固定branch route、真实普通Turn同事务认领收尾；busy/missed跳过、Stop/失败/崩溃暂停、永不自动fallback/补跑，原生面板/HTTP CRUD与真实SDK链路已验。Open不启动timer，Close先取消准入再交Runtime收尾；cold-read barrier反例已补。独立Gateway已接真实RelayKit、≤16显式route/8并发/8MiB与真实owner取消，严格Bearer/Host/Origin/JSON、无凭据发现/重定向/自动执行tool或抓媒体。秘密stdin管道可取消且真实join；内嵌Gateway已接原生/认证HTTP显式billing Start/Stop、一次性token与轮转、timeout保留owner和取消/真实join；只读Devtools MCP使用pinned官方Go SDK，经独立token FD/真实stdio接既有HTTP，sessions/history/typed Inspector/显式record小页均通过，不启动Agent。另有独立默认off的原生UI debug服务，真实日志/level、语义树/状态、声明action及own-window PNG已接两个MyGo入口与官方MCP |
| 沙箱/平台 | SDK js/wasm、真实同SDK WASIp1 guest与fresh wazero host | 真实model→guest文件→引用→model已跑通；os.Root syscall桥只读/越界/symlink竞态、每跳fetch/HITL/无ambient env、真实CPU spin deadline、memory/IO/callback/Close反例/race通过。CLI实际二进制→真实同SDK worker已跑通，附件/历史小索引→字面search→4KiB显式read、host key只到transport、两个独立HITL与pinned目录替换反例独立交叉审查通过。显式Core V2基本块fuel已接原始/重写双验证、每job私有counter、真实loop/binary-start/SDK耗尽和compiler/interpreter对照。当前已按用户决定切回QuickJS-NG独立fresh WASM，恢复32MiB tracked heap/512KiB soft C stack；10秒deadline/64MiB整体reservation/共享fuel与实际CLI验收以本页最上方切回记录为准。Windows创建时owned Job及WASI writable/NOFOLLOW宿主适配已补；Windows原生运行与完整平台分发仍未完成，deadline与fuel继续分开 |

**WASM 模型交付范围（2026-10-17 源码复核）**：host 的 `OpModel` 先将一次 `LanguageModel.Stream` 的全部 chunks 收入有界 `ModelResponse`，同步 exchange 返回后，guest 的 `ProxyModel.Stream` 才逐个 emit。因此目前已验证内容/终态与实际工具循环，不具备模型 token 实时穿过 WASM 边界的交付或消费侧背压；这是当前受控单job请求/响应的实现选择，不属于沙箱核心迁移缺口。沙箱的必要验收是隔离、显式授权、字节/内存/指令预算、取消和真实退出；取消不依赖token流式交付。实际入口目前为 direct CLI 显式WASM单轮，REPL拒绝该组合；Hub/direct desktop WASM模式和浏览器Agent属于独立产品范围。仅当交互式消费者需要实时文本/音频或双向通信时，再讨论有界增量通道、背压和ABI版本，不预先扩充内核，也不把接口名 `Stream` 当作实时流证明。

## 2026-10-17 深度 review 与迁移补齐

本轮先复现缺陷再修复，旧Rust树和用户配置/历史未动：

- **P1**：engine发出成功Completed后再返回收尾错误，Hub曾把Turn保留为成功。现执行owner错误令产品Turn失败、保留原观察事实，不自动fallback/replay；Store也拒绝无成功attempt的Turn success。
- **P1**：外部engine stderr按任意Read块脱敏会泄露跨块secret。现先组成有界完整行/EOF partial，再做显式secret与已知credential字段脱敏；超长行正文省略且真实失败。
- **P1**：native审批pane没展示实际InputSummary。现审阅区展示真实命令/参数；真人澄清问题保持独立broker与会话归属。
- **P1**：显式Env凭据无字段回显曾进入Shell日志。真实进程已复现；shared redactor现接Shell/engine/MCP/AEP/Hooks/tooling/browser实际配置值，覆盖escaped JSON/声明binary/multiline，截断及配置值存在时的不完整行不留凭据前缀；原始用户上传不改写。
- **P1**：direct CLI及两个desktop入口的stdin API key用同步ReadAll，open pipe使取消等待EOF。direct原反例已真实失败；三个入口均改为shared secretinput后取消及时结束且不访问模型/创建store/窗口，两个desktop另将启动signal owner与窗口运行期分开；后续显式Send不被结束的startup context取消。两个独立CLI共用纯terminaltext显示转义，阻断模型C0/C1终端控制，原facts/执行参数不变。
- **P2**：Pi resume因无关empty/truncated JSONL拒绝整个会话目录。现跳过不完整header，仅匹配actual owner/cwd/id，目标缺失与冲突仍拒绝。
- **P2**：实际继承到fd0的blocking pipe不一定支持Go文件deadline，原shared secretinput会把合法CLI密钥管道拒绝；in-process os.Pipe测试此前没有覆盖。新增实际test-binary stdin继承反例，修前取消/EOF均真实失败；Darwin/Linux现用受SyscallConn.Control保护的bounded poll/read，不修改borrowed FD flags/deadline、不留reader goroutine，实际EOF/无EOF取消/超限不等EOF均有回归。其他未实现平台继续明确拒绝不支持的pipe取消。
- 真实optional-tool配方回归发现SharedLocksPlugin只挂middleware、没有在bus提供同一个registry；已补actual registry，Shell/Notebook Finalize真实闭环。

direct CLI原先只有单轮headless，现补独立native REPL：同harness/native session多轮、空REPL不调用模型、actual-ID once HITL、分离key FD、bounded Unix poll且不关闭借用stdin；初始附件只引用一次，WASM仍按原print路径单轮。

独立Hub CLI原先缺失，已补共用bootstrap/product的one-shot/REPL、cold sessions/history/handoff、真实resume和双engine往返，默认deny/actual-ID ask、单独key FD、timeout/真实取消与成功后最终stdout；native完整最终答复从exact committed record读取，拒绝假完整60KiB预览。

已补六类工具产品装配/设置与真人问答、worktree HTTP及显式discard、AEP礼貌关闭、LLM审计、内嵌Gateway、只读Devtools MCP、多窗口、音频派生/播放、WASM fuel与独立QuickJS核心。之后进一步补应用内跨Turn Shell owner/双层lease和Worker附件branch-pinned FIFO；不把本轮代码范围冒充整张迁移矩阵完成。

上一轮集成checkpoint（不覆盖下述续批）：`go test -mod=readonly -race -tags desktop ./... -count=1` **1291 tests / 74 packages PASS**，已包含Env凭据、跨Turn Shell/双层lease、Worker附件固定branch/discard、fuel/QuickJS、独立Hub CLI/direct REPL、两个desktop冷启动及actual inherited fd0路径。`go vet -mod=readonly -tags desktop ./...`、`go mod verify`、gofmt和architecture公开面/导入边界/分形文档闸均通过；七个命令入口`CGO_ENABLED=0 GOPROXY=off GOTOOLCHAIN=local` readonly构建通过，CI也补入Hub CLI与Devtools MCP。SDK/codec的js/wasm与wasip1/wasm及同SDK worker实际构建已再次通过，后续secretinput修改没有进入SDK/codec。两个独立临时目录用WASI SDK34重建同一QuickJS core资产得到相同SHA-256，源/bridge/bootstrap/二进制出处在`platform/sandbox/host/scriptwasm/provenance.json`，普通Go构建不需要C工具链。

真实macOS smoke `.app`日志显示App ready/visible窗口。收口又用最新源码、全新bundle ID/临时数据重建：系统App inventory确认isRunning，但CUA按完整app path和bundle ID均返回`cgWindowNotFound -10005`，没有截图/AX/可操作surface；已对严格匹配路径的自建QA PID发TERM并确认退出。新日志位于OS临时目录`alva-go-final-window-qa-l0wej2u7/alva-go-window-qa-1317170899/native-window-qa.log`。没有据此宣称真实OS视觉/AX/声学验收通过，亦没有修改系统权限。

另一次额外的三个真实二进制fd0消费barrier→SIGTERM复验，被自动审批的路径名机密启发式拦截（确认弹窗超时），该命令未执行，不计为通过。此前无消费barrier的手动signal探针也不作为可取消读取的成功证据；实际继承stdin的EOF/timeout/overflow由上面的真实子进程回归覆盖，启动context与后续Send由两个desktop命令回归覆盖。


## 原实现与新增能力的区别

本轮直接核对Rust源码，而非把计划文档或类型声明当成已实现功能：

- `crates/alva-app-core/src/extension/lsp/mod.rs` 明确deprecated scaffold、StubLspManager，真实server I/O不在已实现范围；Go保留可替换诊断Manager及真实命令Runner，不搬空成功。
- `crates/alva-agent-memory/src/embedding.rs` 只有trait/Noop；`crates/alva-app-extension-memory/src/extract.rs` 负责显式提取文件/索引写入，没有自动LLM提取调用链。因此完整LSP、网络embedding、自动LLM记忆提取归新增需求；用户未另行要求新增能力时按现有功能迁移。
- `crates/alva-hub-desktop/src/worktree.rs` 实际有create/preview/adopt/discard，没有rebase或证据归档/人工恢复UI。Go补齐discard的受管目录功能，保留分支和证据，不复刻隐式branch删除或native历史retarget；复杂多轮merge/rebase另作增强。
- Windows private DACL/LockFileEx/创建时owned Job、WASI文件读写/NOFOLLOW、媒体stdin/handle与CLI pipe已实现并交叉构建，真实Windows运行未验收；Unix fd3 ABI和未实现的pinned shell/交互console仍明确拒绝。真实macOS视觉/AX/声学及付费账户验收也仍未完成。上述状态不能用headless测试、交叉编译或一个窗口visible日志替代。

## 依赖和构建验收

普通 `go test ./...` 不启动 GUI；`desktop` build tag 才编译 MyGo 界面。
`architecture` 测试解析包括 tagged 文件的 import 图，拒绝 SDK/内核的直接及传递越界。
MyGo 固定 v0.5.5，Go 工具链固定 1.27.2；不增加 Node/Bun/npm 的生成或构建步骤。
依赖保持官方 Go checksum 验证。依赖下载与首次冷编译、缓存后二次编译分开计时。

2026-10-16 第一批 foundation，macOS arm64，Go 1.27.2，曾通过以下离线检查
（`GOPROXY=off GOTOOLCHAIN=local`，无真实模型账户/MCP 调用）：

| 检查 | 结果与范围 |
|---|---|
| `go test -race -count=1 ./...` | 基础 SDK、harness、Provider 本地 HTTP fixtures、native controller、CLI、无窗口展示状态与 import 防火墙通过 |
| `go test -race -count=1 -tags desktop ./apps/desktop ./apps/cmd/alva-desktop` | MyGo Tester 输入/中文 IME、流式 Markdown、Stop、10k 行虚拟化和阅读锚点通过；不是真实 OS 窗口验收 |
| 第一批集成 `go test -race -count=1 -tags desktop ./...` | 当时全量通过，包含慢 CLI 输出补齐、取消组合失败、工具错误语义与订阅容量恢复；不自动覆盖下述持续迭代 |
| `go vet ./...` 与 native tagged vet | 通过 |
| `go mod verify` | 全部已下载模块 checksum 通过；未关闭官方校验 |
| SDK `js/wasm` 与 `wasip1/wasm` build | wire/kernel/agent 包通过；不是完整沙箱/浏览器产品 |
| CLI demo 中文工具闭环 | 本地 fixture 实际运行通过，不是付费模型 |
| macOS arm64 native build | `CGO_ENABLED=0` 编译通过；尚未启动真实 OS 窗口 |

当前新增 cgo-free SQLite（ncruces/go-sqlite3）、完整 YAML（go.yaml.in/yaml/v3）和TOML（pelletier/go-toml/v2）parser，
另有真实CDP chromedp v0.21.0/cdproto、wazero v1.12.0和HTML5 tokenizer x/net；
各自公开声明的传递模块保留checksum校验，未额外引入已合并的remote旧module。SDK仍只用标准库，MyGo仅UI子树依赖。
module 不等于 package，不将第一批五个模块的统计继续冒充完整产品依赖总量。

持续迭代的分域 `-race` 已分别通过 SDK、文件工具、Skill、MCP、四 Provider 本地 HTTP、
platform/storage、harness、platform/host 与 CLI、Engine/Hub 和 tagged MyGo 的对应回归；新增源码持续变化，
最近一轮 tagged 全树 `-race` 和 `vet` 已通过（包括prepare/Team/子Agent/HTTP/AEP runtime/ACP/真实browser/media/WASM host）；
此后native媒体冷面板、CLI WASM、shell/web/planning与小型文件描述符能力均有新增owned race/vet；
自动化/handoff/HTTP最新回归亦通过。kernel阶段hook批次审计已接，LLM缓存审计指纹/无正文stream摘要已补，完整原fixture对等仍待补；
交互/utility/notebook、native视频/worktree和RelayKit也有真实分域回归，最终全树复验单独记录，不以一次旧checkpoint覆盖新修改。
公开面AST快照和分形文档CI gate已落且首次通过，SDK新增公开字段/签名需显式审核更新快照。
真实账户、真实 OS 窗口、跨平台运行仍未验收。

2026-10-17 集成收口（macOS arm64、Go1.27.2、官方checksum保持开启）：

- 最后一次 `go test -mod=readonly -race -tags desktop ./... -count=1` 全树通过，
  `go vet -mod=readonly -tags desktop ./...`、格式与architecture公开面/分形/导入边界检查通过。
- SDK与独立codec的js/wasm、wasip1/wasm编译通过；同SDK worker及CLI/Gateway/Hub server/
  两个自绘desktop的`CGO_ENABLED=0`真实构建通过。CLI中文demo闭环通过，无模型账户调用。
- RelayKit原生及跨协议32路、真实HTTP工具ID往返、截断/取消/大整数与高精度小数、字段大小写、
  signed/encrypted/refusal、opaque native item和Gemini累计usage均有实际回归，独立审查finding已闭环。
- 最终gate曾暴露Darwin自然退出→Close的EPERM瞬时否定观察被永久缓存；修前原MCP反例真实复现，
  修后原反例race连续50次、1280次真实子进程退出压力及独立窄复核通过。
  仅有界重新观察WNOWAIT/组内无live证据，不重复signal、不提前reap、不忽略真实权限失败；
  100ms是重观测预算，不冒充OS硬实时deadline或进程沙箱。

以上仅是当前已实现范围的集成checkpoint，不表示剩余迁移矩阵、付费模型或OS视觉验收完成；
没有提交/推送、发布二进制或删除旧栈/用户数据。

初次完整依赖下载耗时受本机网络影响，没有可靠冷构建基线，不报告冷构建提速比例。
已缓存依赖/编译结果的一次构建：CLI 约 0.68 秒，native 约 2.44 秒（`/usr/bin/time -p`）。
未剥离 native foundation 二进制约 16.7 MiB，CLI 约 9.3 MiB；不是完整 Hub 分发包、
安装器、运行内存或全量磁盘占用。尚未测量真实 OS 窗口与其他平台。

## 当前已核对的基线

旧 Web 前端 `node_modules` 为约 197 MiB（`du -sh`），已构建 `web/dist` 约 744 KiB。
两者是开发依赖与前端资源，不是完整桌面安装包。`target` 是指向 mbx cache 的链接，
其目标当前不可读，0B 的链接输出不能作为 Rust 编译缓存大小。
未删除任何旧依赖或缓存，尚不能报告迁移后全产品的磁盘/内存节省比例。

## 抽象变更台账

本批批准的职责合并：无。

审查已要求保留窄读写 ConversationLog 与可选查询 SessionStore、逐调用 Tool 调度/
只读判断、明确 ToolExecutionContext，以及不扩 kernel enum 的版本化扩展记录。
LateContext 的历史读取使用实际只读 wrapper，晚期发现的工具通过返回贡献登记，
不重新开放 Registrar/bus writer。中间件的 before/after 洋葱顺序按原 Rust 语义对照。
ToolResult.IsError 的业务失败可交回模型；Execute/WrapTool 的真正 error 则记录证据、
结算未执行调用并终止，取消同时发生的执行/写入失败不能被降格为普通停止。
历史兼容 API/格式不进入 Go，新实现的内存 demo 不冒充产品持久化或已完成授权系统。

## 尚未完成的迁移与验收

下列条目仍是实际未完成项；不以本轮测试数量替代功能对等：

1. **Windows运行与剩余adapter**：private storage的原子DACL/LockFileEx、创建时JOB_LIST/kill-on-close/真实job清空、继承stdin取消和显式只读私有token文件已实现；amd64/arm64命令和原生test-binary交叉构建及Windows vet通过。CI已声明真实Windows反例但尚未运行，不能据此宣称实机通过。Windows只读seekable媒体stdin/handle与WASI writable/atomic NOFOLLOW已补实现和原生测试；pinned-directory host shell与交互console仍未迁完；managed worktree仍以Unix executable mode/权限快照绑定证据（构造时也检查Unix execute bit），Windows模式/ACL对应策略尚未迁移或验收，不能仅放行git.exe后冒充对等。普通Unix fd3 ABI不伪装成Windows handle。
2. **原生UI调试实机验收**：动态日志level、bounded ring、实际产品语义树/状态/revision、声明action及own-window PNG已迁入独立apps/debug，两个原生UI实际绑定与HTTP/官方MCP stdio均有回归。控件mutation/Send双重opt-in、取消排队不迟到执行、operation ID不重放，诊断shutdown不Stop Agent。actual Hub own-window当前Build的Inspector PNG已通过，本批真实系统AX的创建/Send/Inspector/导出/重启基础路径已通过；CUA compositor仍只有thumbnail，完整视觉/AX/IME未通过，两者分开记录，不以MyGo Tester替代；语义树与原Rust手工InspectNode一样由产品注册，不冒充框架内部layout树。
3. **逐项对等核对**：当前六份Go公开面快照/导入防火墙/分形文档闸约束后续变更，不能证明Rust全部公开语义和适配器fixtures已逐项对照完成。direct desktop已补独立native持久化、会话恢复/附件引用/once审批/Inspector分类与导出，仍直连harness/host、不经EngineRuntime；完整Hub产品保留独立入口。不能用六份Go快照或新测试总数替代逐项Rust语义核对。
4. **真实平台和外部服务验收**：本机macOS基础原生AX演示链路已通过；CUA compositor仅返回thumbnail，完整视觉/AX/IME仍未通过；真实音频设备/声学、各付费模型/外部engine账户以及Windows运行与平台分发验收也未通过。headless MyGo Tester、本地HTTP/helper和构建结果各自只证明其实际覆盖范围。
5. 矩阵逐行验收完成后再讨论旧栈清理；当前未删除旧源码、依赖、缓存或用户历史，也未自动导入旧数据或配置。

完整LSP、网络embedding、自动LLM记忆提取和复杂worktree rebase不列入“Rust已有实现迁移”：原实现的stub/trait/规划不能作为已存在功能。其来源核对见上文，是否新增须单独确定需求。
