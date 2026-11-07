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
| L5 | harness | BaseAgent默认配方、引用/历史/子会话政策；可选durable等待Controller |
| L6/L7 | apps；根 cmd 启动 | direct、Gateway、Hub 与共享 apps/ui 呈现；direct 桌面位于 apps/direct/desktop |

integration/platform 为横向责任组，不代表所有内容都处于 SDK 之下。
LLM codec/provider 分开转换与传输；协议不拥有授权和持久化政策。
platform/host 拥有执行、审批/问题 rendezvous 和资源生命周期，system 提供底层宿主原语，
storage根包提供共享契约，实现位于其子目录；native records/artifacts与Hub产品事实分权。sandbox 实现 host/guest；纯 ABI 留 SDK。

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

planning/interaction/utility 根包为普通工具能力，可以只用内存 SDK 与宿主绑定的小服务运行；引用输出由配置显式选择。计划写入与任务CRUD可分别安装，原生政策继续在 native/ 装配。
各自 native/ 子包才组合共享会话插口、Guard 和持久证据；Hub 装配明确导入该适配，不把这些政策
变成普通工具的前置条件。原生适配复用同一 schema/业务操作，保留 Plugin.Name、工具名与记录
namespace。工具目录读取调用方的实时 registry；偏好投影由明确的 BeforeModel hook 消费。
只有标准库、SDK 与纯 strictjson helper 可进入这三个根包，传递依赖也受静态闸约束。

历史窗口选择及省略提示在harness，storage只提供固定高水位、有界元数据与hash/protocol验证。Hub Runtime只要求六方法ExecutionStore，产品浏览/Settings仍由各自consumer使用，同一SQLite事务保持。
可选telemetry Plugin拥有指纹/诊断schema及报告政策；kernel只给通用观察值，纯SDK缺失即不保存诊断。诊断失败报告后继续，执行intent/outcome、恢复证据和Flush失败仍中断。

可选[durable harness](../harness/durable/AGENTS.md)复用现有Plugin/Tool/hooks和四方法Backend，支持持久等待、受参数约束的一次性审批/问题结果与原子继续投递。MemoryBackend明确volatile；journal adapter消费同一事实插口，不绑定数据库。等待成功持久化后可退出进程；running/interrupted不自动重放，外部任务启动/取消仍归实际owner。direct CLI以`--durable`显式启用；Hub的Alva配方通过共享bootstrap/product接入UI与HTTP，默认off。Engine的Completed(waiting)仅结束本次Attempt；产品Turn保留，显式继续创建同Turn的新Attempt，无新prompt/handoff且禁止fallback，SDK内核不增加waiting状态。

## 人机交互、界面与模型评测

问答语义由现有`interaction.Question/Answer/Handler`表达；权限由`security.ApprovalRequest/Decision/Handler`表达。
CLI可直接实现Handler并串行读终端；Hub桌面/HTTP通过已有QuestionBroker/ApprovalBroker及产品Service观察与提交，
由产品校验实际session/run/call与子会话归属。UI不启动第二执行器，不把按钮点击或草稿当作已提交答案。

| 共享语义 | CLI表现 | 桌面/HTTP表现 |
|---|---|---|
| 单选、多选、自由文字 | 编号/逗号、Other后输入下一行 | 控件选项、文本框或表单 |
| 自定义答案 | Other提交Text | 自定义文本提交Text；不把界面额外行号当原选项 |
| 本次允许/拒绝 | exact request ID或未来绑定请求的按键 | 绑定实际请求的按钮/JSON，不显示后端未允许的授权范围 |
| 取消/到期/已回答 | 输入停止/拒绝晚答 | 禁用旧控件、清除已失效草稿；观察关闭不等于Stop |
| durable等待 | 输出回执退出、冷保存结果、显式恢复 | 保存后保持暂停、明确Continue，复用同一Controller与日志 |

各端可以改布局和键位，不能改选项原始编号、Text/Selected互斥、权限范围和结果提交语义。已有平台broker拥有待办与真实等待，
UI拥有焦点/草稿/控件；如多个前端控制同次执行，接同一个owner的Service/Broker，不能各持一份授权或等待状态。
当前直接CLI与Hub是独立产品装配，不假称它们已经跨进程控制同一个运行实例。

模型工具名、UI显示名和外部协议消息名分别维护。`ask_human`向模型提供真实name/description/Schema，界面可显示“需要你选择”或“澄清问题”；
不能从显示名推断模型训练情况，也不能只改名便冒充厂商定型工具。Provider转换只负责线格式，是否需要模型专属Schema应由实际评测支持。

`apps/developer`是CLI/Hub开发者选项共用的产品外围能力，入口是`alva developer test/eval`与Hub侧栏Developer options，不增加独立维护程序。
test的接入用例检查工具选择/参数、输出通过/失败，不当作模型排名；eval使用独立版本化任务集、评分与重复trial，输出加权分、持续通过的任务数、延迟和实际usage。
评分绑定数据集ID/版本/包含期望与权重的SHA，模型请求另有SHA；执行故障不混入质量分母，未执行无分，覆盖与错误必须同时审阅。
当前均为single_response范围，测试不证明多轮权限/Other、评估不冒充整个Agent质量；小型基础任务可换成显式自有任务集。

共享Service拥有明确绑定的模型route与唯一job，UI只拥有选择/草稿/预览；观察不执行、退出非最后窗口不Stop，取消/Close实际join模型请求。
Hub无需聊天会话即可使用；程序可注入额外DeveloperRoutes，默认live只加入当前明确模型，不借用外部Agent CLI的订阅/账户。
CLI显式配置端点/模型/秘密，默认plan，--run才执行；Hub预览后明确Run，不向会话历史写评测正文，最新报告仅进程内保留。
`integration/llm/eval`在SDK之外只消费既有LanguageModel/wire并测量一次响应，不选择测试/评估政策、Provider或UI；新增场景与grader由外围扩展。
不向内核添加评分政策，普通CI只运行本地fixture。真实模型成绩、多轮Agent任务评估与本地实现回归分别记录。

## Hub 页面与业务落地

SmallPen页面描述布局、组件和交互意图；可运行能力以Go入口、共享服务与耐久事实为准。新增页面先找现有owner及端口，再补缺失领域，不能按“每页一个Service、每层一份规则”重复实现。

| 业务域 | 政策与执行owner | 持久化事实 | 入口与当前边界 |
|---|---|---|---|
| 会话、分支、草稿 | HubService明确提交；product/routing统一账户/计费准入；唯一Runtime拥有执行与取消/join | Session/Branch/Turn/Attempt；原件仍在native Repository | 桌面与HTTP；提交携带明确引用集合，不能隐式消费整个session草稿；历史branch导航不授予新执行 |
| 外观 | PreferencesService唯一默认/patch/CAS政策 | PreferencesStore / hub_preferences | 桌面与HTTP GET/PATCH；主题/字号生效，Density仅保存且布局未区分，原生暂禁切换；语言资源/应用级减弱动效未实现 |
| 权限、问答、等待 | 真实broker/Guard与原Controller；页面仅持回答草稿 | 原始审批/问题和同Turn checkpoint，独立WaitStore | Alva桌面与HTTP；审批必须匹配真实Alva owner；外部双向回调未接通 |
| 工具、Skills、插件 | 产品服务保存next-turn配方，bootstrap装配实际能力 | SettingsStore与实际Skill原件 | 桌面及已有会话级HTTP；应用级目录/安装体验未齐 |
| Team、Worker、自动化、工作树 | FIFO、timer和工作树owner各负其责，认领后仍交原Runtime | 独立Team/Automation/worktree端口及SQLite事务 | 桌面与HTTP；构造不执行，暂停只停未来调度，Stop等待实际owner；未提供Run once契约 |
| 模型连接与参数 | ConnectionsService唯一配置/私值/CAS；bootstrap解析Provider；Developer冻结job目标 | 连接metadata/credential分表，默认/会话参数独立revision | 桌面CRUD/目录/参数；HTTP只有冷targets，没有这些CRUD/参数入口；动态连接已进入Developer测试，本机账户认证仍缺 |
| 目标与引用 | GoalsService管理幂等目标和明确范围，选择仅准备typed原件引用 | 独立GoalStore/PageStore；原件用既有Artifact | 桌面创建/复用/首发；HTTP尚无目标/typed项目引用入口，普通上传另有接口 |
| 伙伴与群 | CompanionService独立身份/成员scope/有限回合，执行复用原Runtime | CompanionStore与可选HistoryStore，群×成员会话/回合/消息 | 桌面；HTTP未曝光伙伴域；删除保留只读历史，新执行事务拒绝tombstone，重开不执行 |
| 资源视图 | WorkspaceViewsService拥有位置/观察I/O；正文由实际资源owner提供 | WorkspaceViewStore只保存CAS位置，不是第二资源库 | 桌面文件/原件/任务/Inspector标签；HTTP尚无视图接口；关闭观察不Stop |
| 系统语音 | platform/system/speech拥有录音/回调，原生页持有草稿 | 无音频文件或云fallback | 仅macOS代码接线，明确on-device；许可/设备质量另验，其他平台明确不支持 |

表按业务权威划分，页面和目录只是消费者。core保留通用产品事实/执行契约，product维护共享业务规则，sqlite负责事务事实和迁移，bootstrap选择实现，desktop/server负责交互与传输。相同服务被两个入口装配，不等于两个入口已经曝光同样功能，不能把缺失入口写成端到端闭环。

原生工作台21/22现已消费该链路：页面只持草稿与浮层，明确首发经现有Create、可选Switch、逐项导入、初次Detail读取及捕获文本的Submit。首发失败保留实际身份与输入；普通导航、建议和Build不启动执行。正文复用共享chat的有界列表、Markdown及阅读锚点，输入区由Hub装配；旧高级能力收进可达菜单，业务owner不复制。

当前首页交互按稳定 requirement ID 核对，动作验收与视觉检查分别记录；测试不能继续锁定与设计相反的临时约束。

| 设计行为 | 真实实现与边界 | 代表回归 |
|---|---|---|
| `req_alva_composer_add` | 首页只选待导入路径；首发Create/Switch后复用Upload逐项保存。失败保留已创建身份、成功引用与未导入余项，不删除原件 | `TestWorkspacePartialImportRetryKeepsSavedReferencesAndNativeBytes` |
| `req_alva_composer_submit` | 冻结点击时的prompt；导入/准入期间新输入可编辑，仅匹配的旧草稿在接收后清理，Worker同样使用捕获文本 | `TestWorkspaceAdmissionKeepsEditsMadeAfterSend` |
| `req_alva_composer_execution` | 本窗按不可变物理owner身份保留未完成Stop，条件取消防止影响后续执行（含同Turn恢复）；导航不视作回执，Cancel错误仍待真实清理，可明确重试 | `TestWorkspaceStopSurvivesNavigationUntilActualCleanup` |
| `req_alva_composer_focus` | 实际路由菜单方向键/Enter选择、有限视口滚动、焦点恢复；提交/等待/停止/禁发提示来自当前真实状态 | `TestWorkspaceComposerTargetKeyboardScrollsConfiguredRoutes` |
| `req_alva_navigation_settings` | 应用设置共享单一返回来源，分类切换不覆盖；Inspector位置保留，媒体只恢复冷索引，运行/播放owner保持原生命周期 | `TestWorkspaceSettingsMediaReturnRestoresIndexWithoutBodyOrPlayback` |
| `req_alva_composer_configuration` | 已提交应用连接的路由可选；失效路由与不兼容附件提前禁发。Ask/Auto/Full通过共享setter只改下一Turn，Full单独确认；Plan最高只读且重置Ask | `TestWorkspaceSavedPermissionWaitsForActualStore`、`TestNativePermissionPoliciesEnforceRealWrites` |

输入区现在通过共享product服务处理typed命令/Skills与项目引用：选择仅改草稿，明确Send重验来源/branch/SHA并共用Submit或Worker入队；文件原件和目录有界metadata保存在既有Artifact，引用不授予权限。PlanMode持久化于已有JSON设置，独立原子setter保留其他批准；bootstrap使用真实只读Guard并在本轮移除进程/网络集成，统一SQLite Attempt事务拒绝外部引擎绕过（含fallback、Worker、自动化）。

权限是产品策略而非SDK新模式：Auto仅自动批准已知原生write_file/edit_file在Guard确认后的工作区写入，命令/网络/MCP/未知效果仍问，可信安全命令分类尚缺。Full在已有Guard与独立host批准范围内自动批准，不启用插件或扩大根目录；首页确认绑定workspace/route并在首发迁移后消费。Plan、运行/waiting与未放弃checkpoint冻结共用同一gate，普通配置导入不能升权或恢复潜在Full。

目标共用表单/稳定事实与首发原件引用、伙伴/群独立领域、资源标签、模型收藏/默认和会话参数以及显式目录GET已有专门产品服务，均复用原Runtime/事实与共享存储端口。跨项目目录、compatibility来源Skills枚举、任意shell型slash、本机Agent认证、语言资源切换与应用级减弱动效覆盖仍缺；运行中插话/自动排队策略未定。macOS语音代码不等于设备验收，媒体播放和递归Inspector仍由专用pane承接。

外观链路为：**原生草稿或HTTP请求 → 同一个PreferencesService → PreferencesStore → SQLite事务**。
允许值、默认值、patch合并及revision规则只定义在`apps/hub/preferences.go`；存储在事务内调用同一纯值政策。
传输层只做认证/格式/错误映射，UI只做输入与呈现。共享owner发布已提交的不可变快照，构建界面不等待数据库；读写失败保留旧快照并报错。
多窗口/HTTP使用expected revision，冲突拒绝覆盖并保留草稿，由用户重新加载；保存成功后统一应用主题和正文字号。Density值虽可耐久保存，当前正式两档布局未区分，原生切换暂禁用，不能以CAS通过宣称布局功能已完成。
Application.Close先停止偏好读写准入、取消并等待实际存储调用；超时保留owner及数据库供重试。已提交但迟到返回的结果按事实发布，不自动重放写入。

产品SQLite由schema v1→v2→v3→v4→v5原子升级，保留外观/连接/目标/模型偏好事实，v5新增独立伙伴/群/回合/消息及资源视图表；会话参数覆盖保存在原Settings JSON。新建/迁移才写初始事实，迁移与完整schema验证先于执行恢复。
未知版本、缺行或损坏会失败，不重建为默认，也不修改独立native数据库。会话SettingsStore与六方法ExecutionStore的职责不变。
连接链路为原生草稿→同一个ConnectionsService→ConnectionsStore事务→已提交缓存→显式路由选择→实际Attempt的immutable Provider配置。Key与自定义header值不进入列表、普通DTO、日志/历史或错误；仅私有SQLite权限保护，不宣称加密或系统钥匙串。空Key编辑保留，显式清除/Auth=none删除；编辑/启停/删除增加revision，使旧路由明确失效，不悄悄切账号，也不取消已经使用旧快照的实际Attempt。冷AvailableTargets共用于桌面、HTTP和Team/Automation，不在Build访问SQLite，默认仍由程序明确配置。
新增连接不发网络请求；显式连接测试/模型目录GET与动态连接推理fixture已接通，目录成功不等于模型推理授权或质量通过。本机Agent账户认证仍须由实际产品适配owner实现。

## 存储替换边界

目标是**同一契约从内到外贯穿**，由程序/组件/Plugin装配选择实际实现。只在最底下多加几个backend不算完成：
如果harness或工具仍要求具体Repository、界面仍识别SQL错误，替换仍会被中途卡住。

```text
程序入口/组件装配选择实现
           │ 同一组共享契约
产品 → harness → Plugin/Tool → 存储组件
   冷Inspector/UI ← 共享DTO、页、引用和错误
```

端口按实际权限和一致性职责组合，不要求所有数据共享一个万能Store：

| 消费者 | 接收的契约 | 贯穿规则 |
|---|---|---|
| SDK循环 | `abi.ConversationLog` | 只看实际log的原子append/Flush；SDK不依赖platform |
| 产品会话与执行事实 | `storage`中的SessionReader/Records/Conversations/Children等 | 返回同一`storage.Session`/ConversationTail/DTO；不暴露SQL/文件句柄 |
| harness配方 | `SessionStorage`/`ChildStorage` | 组合上述同一组端口；历史窗口/子会话政策由harness拥有，不打开库 |
| 执行证据Plugin/冷Inspector | 各自只需的共享能力子集 | 工具无存储Close；Inspector不获得执行claim或恢复Agent的能力 |
| 模型引用 | `context.ReferenceBackend` | 同一实际会话绑定，有界读取/搜索；由storage.References返回接口 |
| 可选等待 | `durable.Backend`，或`journal.Storage`适配 | 原子CAS状态+交付及独占claim；重启等待需事实/字节都persistent |
| 可选记忆 | SDK `memory.Backend` / `storage.MemoryStore` | namespace由装配固定，Bind不返回具体SQL类型 |
| 外部Agent身份 | `storage.BindingStore` | 原子create-or-verify owner/workspace；process adapter不创建数据库 |
| Hub产品事实 | `ExecutionStore`/`SessionStore`/`SettingsStore`及可选能力 | DTO与ErrNotFound在Hub契约；Runtime仍仅依赖六方法ExecutionStore |
| Hub应用外观 | `hub.PreferencesStore` | 与会话无关的singleton及原子revision CAS；原生/HTTP共用同一owner，失败无默认fallback |
| 不可变大对象字节 | `blob.Store` | Put/bounded Read；不取代事实事务或授予宿主执行权 |

`apps/direct.Config.Storage`必传；`bootstrap.Config.OpenStores`必传，Memory/Bindings工厂只在相应能力启用时需要。
现有程序入口显式选择stock Local配方；其他程序可装入自己的组件，所有下层消费者和返回值继续走同一插口。
具体provider只在实现目录、cmd或bootstrap/storage.go出现。不存在、typed-nil或工厂失败直接报错，不另开默认库。
实例所有权由装配移交；真实执行join后Close，失败保留同一组件供显式清理重试，工厂部分资源也不能丢弃。
私有目录/文件是`system/privatefs`原语，不因调用EnsureDir而把SQL传递带入组件。

共享契约保留实际语义：原子记录、快照CAS、独占claim、owner/session范围、因果关系、固定高水位及hash验证。
`SameAuthority`验证实际权威而非字符串ID；任意云盘都不能只上传文件就冒充这些保证。
记录与artifact retention分别声明；临时引用明确进程生命周期。若字节另用blob组件，先发布不可变对象再提交metadata，
失败可留下孤立字节，不自动重试/删除，不声称分布式事务。账号与实现选型均留外部装配。

契约、具体实现和产品默认是三件事；本轮纠正依赖方向与贯穿方式，不增加云provider、隐式数据迁移或新内核存储类型。
公开验收入口在`platform/storage/storagetest`、`platform/storage/blob/blobtest`及SDK的`sessiontest`；只由测试导入，factory由后端作者供应，权限/原子性语义与现有插口相同。它们不构成新的运行时层，也不替代持久化/故障/跨进程验收。
独立module消费者在`examples/sdk-agent`，只导入公开SDK；默认循环、Plugin/Tool/Middleware和真实工具内取消在native/Node/WASIp1共用源码。接入验收不修改内核契约或产品默认。

外部Plugin仍复用AEP协议与现有Plugin/Tool/Middleware：JS/Python由显式解释器执行，独立WASIp1模块由`platform/sandbox/plugin`执行，再由`integration/protocol/aep`消费owned Peer。执行peer只依赖标准库/wazero，不认识协议/SDK/授权/存储；`extension/loader`拥有快照、能力上限、Guard、引用和journal，Hub产品拥有检查/批准及下一Turn选择。WASM不接收目录/环境/网络，自有总内存/IO/时间限额。此分工不把整套SDK编译WASM、QuickJS脚本和独立插件合成统一runtime，也不给内核新增runtime枚举或hook。

## 强制边界

- SDK 不得直接/传递依赖 integration、platform、extension、harness、apps 或具体第三方 Provider/UI。
- kernel 不依赖 agent 语义/装配；窄 ConversationLog 不强迫用户采用 SQLite 或产品会话模型。
- direct 产品不进入 EngineRuntime；Hub core 只依赖 engine 根契约，具体适配器在 bootstrap 装配。
- UI 与 Inspector 观察已提交事实，不负责恢复、自动重放、执行取消或审批旁路。
- 取消须传播并真实 join；物理未完成的资源仍由原 owner 持有，关闭重试不能重放副作用。
- 文件/媒体/大结果/MCP/历史引用优先；直接父授权和 child 自有事实/日志不因共享 Bus 而放宽。

`tests/architecture` 按真实 tagged imports 强制依赖图、公开面和分形文档；
`tests/extensibility` 独立运行 SDK 消费者实验，手工窗口工具位于 `tools/qa`。
Bus 与 Registrar 的治理见 [BUS-RULES](./BUS-RULES.md)、[REGISTRAR-RULES](./REGISTRAR-RULES.md)。

## 技术栈与历史

桌面为固定 MyGo 自绘，无 React/WebView/npm；普通 Go 构建嵌入固定 QuickJS-NG WASM，
只有维护者显式重建该资产需要独立 WASI C 工具链。协议转换吸收固定 MIT Magpie 切片，
不吸收其完整应用、凭据发现或工具执行；SDK 不依赖其 DTO。

[ADR 0012](./adr/0012-go-native-stack.md) 记录技术栈和边界决策。
Rust 及旧 UI/构建工具已存于 `archive/rust-final-2026-10-18`；旧架构/产品规范可用
`git show archive/rust-final-2026-10-18:docs/ARCHITECTURE.md` 等命令查阅。
旧声明不自动成为当前实现或验收证据。

## 当前目录与入口

```text
cmd/                         八个独立 main：CLI、桌面、HTTP、MCP 桥、WASIp1 Worker
sdk/                         L0–L3；原公开包路径与契约保留
extension/                   L4；按能力组织的可选工具/插件/中间件
  files/                     模型参数、引用读取、备份与编辑政策
  security/                  actual Tool 身份/effect/HITL 装配
  diagnostics/               显式诊断命令与可替换 Manager
  telemetry/                 可选无正文诊断编码/保存，失败显式报告
  planning/                  独立计划/TaskService 任务工具，可选引用输出；native/ 为原生会话适配
  interaction/               普通 Handler 问答、可选引用输出；native/ 为原生 journal/期限适配
  utility/                   普通偏好/实时目录、timer/；native/ 为原生 CAS/授权适配
harness/                     L5 默认配方、历史窗口选择与强恢复事实
  durable/                   可选等待/审批/问题恢复，native/复用既有事实存储
integration/                 llm / protocol / engine 外部边界
platform/
  host/                      Controller、command/browser 资源 owner
    workspace/               独立 pinned 文件系统叶子，不依赖 host 父包
  system/                    底层宿主原语，严格 JSON 校验在 strictjson
  storage/                   native facts/catalog、可插拔blob、persistent memory 与 SQLite 原语
    blob/                    immutable byte端口；directory与volatile memory适配器
  sandbox/                   实际 host/guest 与固定 QuickJS 资产
apps/
  direct/                    直接产品会话/执行 owner
    desktop/                 direct 窗口/诊断/订阅/耐久页面
  ui/chat/                   共享聊天呈现与原生控件
  ui/modules/                可信版本化消息 renderer
  debug/                     默认关闭的观察诊断
  gateway/                   独立模型 HTTP/SSE 产品
  hub/                       根调度契约及 product/bootstrap/sqlite/server/desktop 等子包
tests/architecture/          静态依赖、布局、分形、公开快照与转换来源闸
tests/extensibility/         独立 SDK 消费者实验；其他单元测试仍就近
tools/qa/                    显式手工窗口验收，不由普通测试启动
docs/                        当前架构、使用说明、治理、ADR 和 history
```

cmd 是启动程序的源码集合，CLI 是其中的终端交互形态；各入口有独立生命周期，使用对应产品
时才启动。L 编号是架构职责标记；运行阶段主要由 L2 协调，不按 L0→L7 顺序逐层调用。

共享 UI 不依赖 direct/Hub/harness/host/engine，产品通过回调接入实际 owner。direct 的私有
诊断状态随产品 wrapper 保留；共享模型仅提供明确的行数保留构造、只读序号和已存在运行的
观察重绑，不导出可变 ListState/内部序号，也不产生新执行。

workspace 只提供 Guard-bound 有界读/列目录/搜索、串行 Update、快照和根生命周期，不导出
os.Root。Update 在同一锁内读取当前字节、执行调用方变换/备份回调并原子替换；回调失败不写入，
回调不得重入同一 Workspace。引用/backend 和模型可恢复错误仍归 extension/files；未知工具
审批与实际 Tool 信任归 extension/security。宿主路径 Guard 不代替模型执行的 effect/HITL。

SDK、Graph/AgentLoop、默认 harness、EngineRuntime 的职责未合并；单 module 身份未改变。
所有实际消费者迁到新外围包路径，旧入口/UI/builtin/tooling/httpjson 路径不留 alias。
诊断 Plugin 的既有 Name("tooling")、配置键、工具名和记录 namespace 保留；包目录改名不重写事实。
后端合规套件、可选扩展、harness、协议/provider 和存储继续保留外部复用入口，未整体 internal 化。

2026-10-18 的初始覆盖表和整理依据完整保存在
[结构审查存档](./history/2026-10-18-structure-review.md)，其中数量/旧路径描述审查基线。
当前执行和剩余验收见根 [MIGRATION](../MIGRATION.md)，运行与构建见 [USAGE](./USAGE.md)。
