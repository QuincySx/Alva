# L7 · Hub product supervisor
> Durable HubSession/EngineBranch/Turn/Attempt 产品监管，不是 native Agent records 的第二权威。
## 地位
本包只依赖 engine contract 与标准库，具体 adapter/SQLite/UI 在外层装配。
## 逻辑

AEP产品快照仍由程序选择组件：显式interpreter供JS/Python，空interpreter供manifest验证后的嵌入式WASIp1；核心只做有界纯值验证，不读取manifest或启动runtime。
产品先认领耐久 Turn/Attempt，再执行引擎；自动 fallback 串行、固定同一Turn，显式账户/计费授权快照且只有 startup/rate-limit、无可能副作用才可用。Stop 不重放。EngineBranch 保留 native identity，手动切换可返回旧分支；handoff 仅索引/引用、不复制全部原始历史。
ActiveExecution只读当前物理owner的不可变身份（本次准入的初始Attempt ID，fallback内不变，同Turn恢复后换新身份）；StartOnBranchOwned/ResumeWaitOwned从原子准入直接返回该身份，快速完成后不另行发现替换owner。CancelExecution在原control/mu锁序内校验该身份再取消，迟到操作不取消替换owner，原Cancel入口共用实现。CancelAndWait在Runtime内捕获实际owner并等待其exited，覆盖尚无身份的准入；CancelExecutionAndWait保留UI已观察身份，拒绝替换owner。超时不释放owner，Worker Stop先冻结下一pump认领再join独立等待续轮，旧ID拒绝后原FIFO可继续。未完成准入返回active但空身份，实际Execute收尾前不释放owner；不是新的持久状态或权限。
ActivePermission只冷读真实ready owner的execution/attempt/engine/native四项binding，不制造pending或native身份；RespondPermissionBound按原control→mu锁序逐项重验，拒绝替换owner和同run fallback的新attempt/engine/native。普通RespondPermission复用同一分派；native产品在首次Detail读取前冻结binding，旧Alva审批不能重新发现并授权外部引擎。
等待通过独立WaitStore端口装配，ExecutionStore仍六方法；挂起结束实际Attempt但不结束Turn。同Turn显式继续认领新Attempt、无新prompt/handoff且禁止fallback；结果只保存不执行。普通输入、路由与next-turn配方在waiting时冻结。
Settings.PlanMode是产品下一Turn纯值；独立SetPlanMode只改该值并保留其他批准。实际只读Guard与可选进程集成收敛由bootstrap消费，存储统一Attempt事务拒绝Plan外部引擎准入（含fallback/Worker/自动化），不扩Runtime六方法执行依赖。

## 约束
Runtime仅消费六方法ExecutionStore；设置/浏览由实际产品consumer要求，SQLite实现及事务不拆分。
应用外观走独立PreferencesStore，选项/默认/合并校验集中preferences.go；不混入按会话的SettingsStore或执行依赖。界面拥有草稿，共享PreferencesService只发布成功提交的revision，存储端在原子事务内复用同一政策。
产品 ID 永远不等于 native ID。UI 只观察已提交产品历史，不拥有执行取消。可恢复 Error 不释放 active；唯一 Completed 收尾。SQLite产品store在子包，与native session.sqlite隔离，不读旧Rust DB。
引擎回调在产品层串行提交；首次校验/持久化错误锁存，adapter忽略错误或后补成功也必须失败且不fallback。Execute返回后关闭回调准入，迟到回调不能修改facts。
同键（Kind/ID/SessionID）纯文本增量在eventMu下合并，每条原始增量仍立即校验；不超过16KiB、约250ms提交一行，非增量事件先flush再提交，顺序不变。独立100ms时钟保证模型停顿时已收文字最晚一个窗口可见；Execute返回后先join时钟再flush剩余缓冲，最后关闭准入。
Completed后的真实Execute返回错误也不能让Turn成功或fallback；原始engine事件保留，产品记录execution_owner_failure并失败收尾，不能用早到事件覆盖owner错误。
NativeOwnerResolver是产品存储的显式只读路由契约；ErrNotFound统一缺失对象语义，不泄漏SQL实现。Runtime仍仅要求ExecutionStore六方法，不因Service的完整profile扩大执行依赖。

## 业务域清单
| 文件/目录 | 职责 |
|---|---|
| connections.go | 独立连接/模型DTO、秘值隔离、CAS编辑与revision路由；可选RequestID创建/恢复端口和不可变脱敏回执，不扩大执行存储；DefaultCatalogURL按请求地址推出同源模型目录，认不出的形状返回空而不猜；API Key可选：编辑时没有Key（新建未填或明确清除）即无认证，原有Key在存储中缺失仍报错；ConnectionModel 的上下文窗口分目录明确声明（CatalogContextWindow，优先）与运营者手填（ContextWindow），0=未声明、1Ki–100Mi 外拒存，DeclaredContextWindow 按此取值 |
| connections_catalog_test.go | 各协议请求地址推出同源目录、拒绝带凭据/查询/非HTTP(S)，推出结果必过同源校验 |
| goals.go / goals_test.go | 不可变目标事实、项目/会话范围、RequestID幂等回执与独立存储/范围cursor分页插口；创建不执行 |
| model_preferences.go / model_preferences_test.go | 连接/模型稳定收藏键、Agent偏好、参数校验/能力准入及独立revision；Attempt冻结不受后续编辑影响 |
| session_model_options.go | 当前会话按账户/模型的独立CAS覆盖、显式清除与深拷贝；下一新Turn生效，挂起续轮沿用原Attempt |
| companions.go | 稳定伙伴Profile、固定Skill来源、群成员/范围快照与隔离会话；独立mutation及可选删除历史/keyset浏览插口，不扩大ExecutionStore |
| companion_skill_scope.go | 实际伙伴Skill装配的有界固定来源/SHA允许列表；nil普通会话与明确空限制区分，普通配置导入不得扩大 |
| workspace_views.go | 逐会话资源标签/顺序、来源身份、查询/分页/滚动/专注及有界宽度偏好CAS；纯观察位置不授予执行权 |
| navigation.go | 独立可选只读workspace目录/精确子会话分页与全库有界自动化索引；已存路径位置过滤不冒充项目来源事实，不扩大ExecutionStore或创建会话 |
| developer_workspaces.go / developer_workspaces_test.go | 显式Repository、approved Git实时Worktree与conversation-tab纯值DTO/端口；tab/derived home draft身份跨worktree全局归属且归档不释放，scope/revision/active/archive/1024 metadata预算，首Send只原子绑定一个真实Session，不接管ExecutionStore或资源Tabs |
| composer_drafts.go / composer_drafts_test.go | home/会话逻辑范围的共享手动输入、显式引用回执与单调revision CAS；首次创建身份独立原子插口，清空保留版本，不执行、过期或删除原件 |
| composer_files.go | 首会话前不可变本地文件catalog、显式来源scope与分块SHA DTO；独立只读ownership插口读取真实home→session grant，原件不重归属，ID/URI/hash不代替会话授权 |
| runtime_options_test.go | 冻结参数传给真实adapter、配置能力不支持时零Execute |
| permissions.go | 产品Ask/Auto/Full值与校验；Settings仅保存下一Turn意图，真实宿主执行政策不进入SDK |
| model.go | 产品事实、分开的ExecutionStore/SessionStore/SettingsStore与产品Store组合接口与版本化交接索引；HistoryEntry.TurnSequence读取时填充、不写入事件；等待继续/放弃的时间线kind常量供消息流投影 |
| preferences.go / preferences_test.go | 应用级外观与左栏折叠/有界宽度DTO、三组选项与唯一纯值政策、稀疏revision CAS端口；消费者修改选项不改变共享校验，布局不覆盖独立外观 |
| wait.go | 可选WaitStore、分支最近Turn观察、显式同Turn继续与输入冻结政策，不增加SDK状态 |
| runtime.go / runtime_identity_test.go | 独立Descriptor/engine registry、串行执行与流式增量合并写入、绑定实际owner身份的取消/审批与授权fallback；权限四项binding原子分派、同run fallback拒绝旧审批及准入/替换/取消收尾身份回归 |
| runtime_coalesce_test.go / export_test.go | 1000增量合并且逐字节一致、穿插工具保序、停顿期内可见、契约拒绝与持久化错误锁存后零写入；仅测试可调短合并窗口 |
| descriptor_test.go | 第五engine声明/不可替换、纯冷读与snapshot切片隔离、关闭后拒绝注册 |
| settings.go / aep.go | next-turn私有MCP（本地进程或远程Streamable HTTP：地址/请求头/认证无或OAuth，OAuth要求https、loopback除外）/AEP snapshot、native SubAgent及项目兼容配置文件的opt-in纯值与预算，不引入protocol/进程依赖 |
| app_settings.go / app_settings_test.go | 应用级能力意图（MCP/AEP/工具/Hooks/浏览器/媒体/子Agent/记忆）、按工作区+配置指纹的批准事实与唯一ResolveSettings：会话覆盖（含旧会话已有配置）优先，否则应用意图仅在精确批准的工作区运行；应用设置落盘不含批准位 |
| tools.go | Shell/Web显式配置、独立access批准与Planning/Notebook/Interaction/Utility下一Turn可选开关纯值 |
| commands.go | lifecycle Hooks/diagnostics明确argv/env/timeout与独立host批准纯值，禁止内核进程政策 |
| media.go | 默认关闭媒体工具、两条显式decoder路径和独立host批准纯值，不推测模型视觉能力 |
| browser.go | 默认关闭browser的明确exe/env/headless/proxy/timeout与独立host/network批准纯值 |
| workspace.go / workspace_test.go | 有界公平workspace共享/独占租约、actual admitted lease可显式retain至后台physical reap；seal拒绝late retain，不重排reader造成writer死锁 |
| workspace_action.go / workspace_action_test.go | 显式文件mutation复用双方workspace独占租约、Runtime关闭真实取消/join、重叠路径去重 |
| team.go | Team/Worker/Task及原branch pin；可选ComposerWorkerDraftStore保存冻结文件/Skill与原始手动正文，复用原FIFO/执行owner，不扩大TeamStore |
| team_snapshot.go | handoff的真实Team/Worker/head/last outcome小索引，不转移正文或native日志权限 |
| worktree.go | Worker私有工作树binding、固定SHA/path evidence、revision/adoption/明确删除审阅及removed/unknown纯产品事实合约 |
| team_runtime.go | FIFO任务同事务认领并复用唯一Hub Runtime执行owner |
| automation.go | 默认暂停、固定branch/route的本地计划事实与独立AutomationStore，同普通Turn原子准入而非第二runtime |
| sqlite | 私有耐久产品store、事务认领、历史、外观singleton原子迁移/CAS与崩溃修复 |
| product | UI无关L7共享服务、消息流内容块投影/字节有界窗口与窄TranscriptService、工具事件Display→ToolView折叠（不认工具/引擎名）、审批与问答块（两条交互路径合成一个视图、统一回答入口）与attempt用量块、图表数据块格式/严格解析与唯一模型说明、执行轨迹窄TraceService/TraceWindow（Inspector元数据分页、配对、产品轮次锚点、时间轴排布）、独立应用外观owner、真实文件/历史/Inspector/Skill与SDK配方；路由准入及显式fallback政策共用一份；附件/项目引用按显式DraftID/版本集合重验及精确消费，伙伴删除索引与完整只读分页 |
| bootstrap | Hub共用实际装配和安全关闭，图表格式说明在装配期送入引擎（Alva插件提示、Claude追加式系统提示；Codex/Pi的通道会替换用户配置故不注入），Connections以精确revision进入独立Developer目标源，显式CLI配置路径来源与实际adapter一致；MCP/AEP等exact attempt资源重试与facts释放顺序，desktop/server不各造recipe |
| desktop | 原生窗口按钮与36px共享页头组成的正式工作台/输入区，按真实titlebar安全区布局且侧栏完全隐藏/恢复；消费共享产品服务与虚拟消息（经Transcripts按块窗口、滚到顶部自动加载；回复中的chart数据块自绘为图表）；中间窗口「对话/轨迹」切换，轨迹只经Traces读取，工具块按ToolView类型呈现（命令/读/改/搜索/子Agent卡片/待办）并跳到对应步骤、右栏文件或子会话；审批与问答在对话里是卡片（卡内操作、处理后折叠一行），回复底部一行淡色用量；共享保宽/保焦点Loading、Foundation Switch、ModelRow整行状态与独立操作及ComposerMenu/General布局回归、一小时opt-in稳定性、paint-only减弱动效与平台发送快捷键准入；已提交模型参数观察与表单草稿分别持有；群编辑不污染执行事实，scope切换隔离旧历史；应用外观及左栏折叠/宽度稀疏CAS跨窗同步（Density/语言入口暂隐藏，减弱动效真实消费），首发/设置返回/Stop/等待保留真实上下文；问答/审批独立读错、防重及迟到回执隔离，不拥有执行 |
| worktree | 显式approved Git、live repository/worktree只读发现、no-checkout直读blob、pin父目录原字节diff/采纳；不运行filter/checkout hook或自动commit/reset |
| server | 认证 loopback HTTP/SSE，同源 product 服务与耐久游标，不接客户端 host path |
| devtools | 独立只读stdio MCP诊断；显式Hub/token fd，真实bounded历史index/typed Inspector/2KiB读取，不启动或修改Agent |
| composer_execution.go / composer_execution_test.go | 可选原子Composer准入端口、原始正文与模型输入分开、同Runtime typed文件/Skill冻结与能力安全fallback，不扩大六方法执行存储 |
| engine_interactions.go | 只观察已提交pending与实际owner四项binding；成功回调仅隐藏原请求到真实resolved事实，失败可重试，旧回执不能重新发现替代owner |
