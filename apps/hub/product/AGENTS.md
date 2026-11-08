# L7 · Hub shared product services
> Headless与native共用同一产品业务、DTO与事实权威，不依赖desktop/MyGo。
## 地位
位于纯 Hub core 之外的 L7 产品服务层；可消费 harness、platform/host、platform/storage 与显式 integration，不能被 SDK 反向依赖。
## 逻辑
认领HubSession后绑定native owner。上传原件耐久保存，模型仅收小引用和真实处理说明；Inspector和历史按目标归属有界只读。消息流内容块投影与窗口在本包：TranscriptService只依赖History/HistoryBefore两方法分页（Tail/Older每页128往前读到≥32个用户/回复/工具块或16页，Newer最多8页），TranscriptWindow按块而非事件累积、无I/O，同ID增量追加、最终消息替换、页边界块合并，按64MiB估算字节从离视口远的一端整块裁剪且不裁运行中块及其Turn。UI的行/Markdown/滚动呈现留apps/hub/desktop。工具块呈现由各engine adapter在事件里给出归一的engine.ToolDisplay（ADR 0013 §3）；本包tool_view.go只把各事件的Display折成ToolView（后到字段覆盖、先到补缺，事件到达时算一次，值发布后不可变、变更换指针），派生展开来源（Output引用或Text）与跳转（file/child/trace），页边界合并取当前值补旧值，字节计入窗口上限，同一Turn较早的待办块标Superseded；不含任何工具名或引擎名，无Display的旧事件按other显示名字与正文。执行轨迹在本包：TraceService只经HubService既有inspectionTarget路由（≤8跳typed child、引擎无Inspector或未声明BackwardPages时返回Unsupported结果而非错误，避免把忽略Before的Inspector误读成会话开头）读Inspector元数据页（Tail/Older向后、Newer/Around向前，每页20条），正文只在Detail/Raw按2KiB页读取；TraceWindow无I/O，按元数据重建步骤（model按RequestID、tool按RunID+CallID配对，工具时间优先StartedAt/EndedAt否则用提交时间并标近似，intent缺失的outcome等它进窗再出现），4MiB估算字节从远端裁剪，只释放被裁一侧的产品证据：该侧扫描边界移到最近的保留锚点、超出的锚点/起点丢弃，另一侧锚点与边界保留，下一页在该侧重扫所需范围。轮次号来自产品History中引用native序号的事件（TurnSequence锚点）：native run以用户输入记录分段，段内任一锚点给整段定轮次；无锚点段只在两侧同轮或候选Attempt数恰好相等时推断，否则留0。产品History只在窗口附近有界扫描（越过首个窗外锚点所在整轮即停，≤16页），不扫描整个会话。
## 约束
无MyGo/WebView/React；不靠build tags隐藏UI传递依赖。读取Inspector不restore、不启动Agent/MCP。History/Team工具验证actual Tool与非空matching call、精确字段/重复键/Unicode；产品归属由已存branch重审，不凭同名Tool或nativeID字符串推断。设置仅下一次装配消费。Skills限定saved cwd与可恢复归档。
ServiceStore组合hub.Store与必须的NativeOwnerResolver；审批直接消费声明的路由插口，不靠具体store附带方法。事实Repository、DTO/错误均来自共享storage契约。可选Team/Automation能力仍显式按既有契约发现；私有目录只依赖system/privatefs，不带SQL。

持久等待从已存active branch及该branch自己的Turn派生owner；冷读不运行Agent。Resolve绑定完整请求/revision且不执行，Resume绑定不变配方并交唯一Hub Runtime；Abandon在实际claim内重置native并协调产品边，保留未知/外部效果说明。

输入区的命令/Skills选择与项目引用共用同一产品服务：选择只准备草稿，明确Send在同一准入锁下重验typed来源、应用真实Plan意图并复用原Submit/Worker入队。每个窗口只持有本窗回执的DraftID/SHA256集合，Send必须显式携带捕获集合；空集合无附件，未知、重复、版本或branch不匹配拒绝且不执行，成功只消费该集合。项目引用和上传共用每会话8条/40MiB暂存总上限，选中集合另重验相同单轮上限，尚非每窗独立暂存预算。原件存既有Artifact，按session/branch/workspace绑定；目录只保留有界metadata，不增权限。草稿列表由独立短时读写锁发布；观察者不等待准入或文件I/O，移除不删除原件。Plan选择只通过共享setPlanModeLocked校验空闲和native checkpoint，再由已选store原子保存，不能绕过冻结配方。

## 业务域清单
| 文件 | 职责 |
|---|---|
| navigation.go / navigation_test.go | 真实保存workspace与精确子会话冷分页、managed路径显示政策、可选port回执/游标校验与100计划索引；不退回最近页、不扫描目录、不Tick执行 |
| connections.go / connections_test.go | 独立连接owner与完整保存回执校验；可选RequestID创建/明确恢复共用Apply/provider校验，精确重试不重复创建，编辑/删除不被重放覆盖；聊天/开发者共用凭据插口与取消/join |
| routing.go / routing_test.go | 完整账户/计费/能力route的一份准入政策；显式Switch/Fallback与既存branch历史选择分别表达，过期route在新Turn前拒绝，不把历史访问当执行授权 |
| connection_catalog.go / connection_catalog_test.go | 明确同源目录GET/测试、四协议分页/预算、来源绑定凭据及取消/join；保存不联网、不声称完成推理测试 |
| model_preferences.go / model_preferences_test.go | 收藏/Agent/默认参数独立保存、已提交快照、未知回执显式Reload和真实关闭，不改变连接版本或选择模型 |
| session_model_options.go / session_model_options_test.go | 当前会话参数读/CAS保存/清除、实际Descriptor和完整回执校验；复用连接owner取消/join，不改当前或挂起Attempt |
| companions.go / companions_receipts.go / companions_test.go | 伙伴/群CRUD真回执、稳定Skill来源与私聊/群成员隔离；有限回合复用既有Runtime和真实审批/Stop，取消/join后再关闭存储 |
| companions_browse.go | 可选删除身份冷索引、绑定owner/group/scope的keyset页回执与当前tombstone只读gate；浏览不创建或执行 |
| companions_rounds.go | 固定成员与Turn的同回合进度、等待审阅/明确继续/终止、已知回复幂等补投；未知效果不自动重放 |
| companions_history_test.go | 较新Turn占满历史页时仍找到原成员回复；真实私聊/群聊删除重开后可发现历史，普通Submit/direct Runtime均零执行 |
| workspace_views.go / workspace_views_resources.go / workspace_views_test.go | 逐会话标签/顺序/宽度CAS与严格回执；真实文件/任务/原件/Inspector有界读取、WorkspaceFileTab把工具路径（工作区内绝对或相对，Rel前对根与目标都解析软链接、失败用原值）钉成active分支file标签，只读metadata不授权、来源重验及独立观察I/O取消/join，固定共享metadata预算保留浅层根文件与partial分页范围回归；ABA不覆盖新位置，不重放执行 |
| developer_workspaces.go / developer_workspaces_test.go | 显式Repository/approved Git Worktree导航与worktree-scoped conversation tabs；Worktrees先只读复验保存的Path/CommonDir identity，changed common-dir以repository conflict拒绝且不列成员；tab/home draft身份跨workspace唯一且归档不释放，纯view metadata CAS/归档/重开；首Send显式注入DeveloperWorkspaces owner，写库前verify-only确认scope仍存在且canonical等值并仍是同repo实时成员（缺能力fail closed、绝不创建目录），再复用home draft原子Session准入并在取消时join，不启动模型、不Stop或删Git/session |
| goals.go / goals_test.go | Create核对精确canonical请求及已存不可变事实，Read核对Goal ID；坏/取消回执不自动重试，显式原RequestID恢复唯一目标。范围冷keyset分页、原件引用/首发同锁绑定与Send重审；创建/移除引用不执行或删原件 |
| permissions.go / permissions_test.go | 与Plan共用空闲native/checkpoint gate；显式Full范围确认、下一Turn策略持久且不启用可选能力 |
| composer_commands.go / composer_commands_test.go | 冷命令/受管Skills目录、有限参数、来源/branch/SHA绑定的typed preparation，选取不启用不执行 |
| composer_home.go / composer_home_test.go | 首发前显式项目受管Skill目录/原文与workspace/SHA准备；创建后复制绑定真实branch，Send产生精确typed Skill输入，不扫描HOME或开启全目录 |
| composer_draft.go / composer_draft_test.go | 明确Send重验全部选择、应用计划意图并共用普通/Worker准入；变动或篡改拒绝保留稿 |
| composer_drafts.go / composer_drafts_test.go | 显式逻辑范围的耐久手稿owner、严格保存既有准备DTO、跨窗已提交缓存与匹配revision消费；取消/join真实I/O，旧Send回执不清新稿；普通首Send与开发tab首Send共用只校验已存在canonical目录的gate，普通空workspace仍按原策略创建私有目录，开发tab绝不创建目录 |
| composer_files.go / composer_files_candidates.go / composer_files_documents.go / composer_files_test.go / composer_files_pdf_darwin_test.go | 首会话前显式本地文件流式捕获、private关联原件/有界Office文本派生、SQLite不可变catalog、项目/最近/绝对路径fuzzy候选与精确scope/SHA的Read/Search；短文本4KiB/总16KiB及上下文预算；PDF可注入系统独立worker，扫描/锁定/越界及媒体未处理状态真实记录 |
| composer_file_submission.go / composer_file_restore.go / composer_file_ownership.go / composer_file_submission_test.go | 真实持久草稿关联恢复原scope文件回执、仅atomic home→session的不可变grant、foreign session拒绝；普通Send/Worker共用typed engine.InputFile冻结器，原始正文/模型manifest分开，经原路由/Plan/准入，不从prompt解析授权 |
| workspace_references.go / workspace_references_test.go | saved项目有界分页搜索、文件原件/目录metadata artifact、共享附件预算和分支准入 |
| plan_mode.go / plan_mode_test.go | 下一Turn只读计划配置，空闲native/checkpoint校验与普通配置批准保留 |
| preferences.go / preferences_test.go | application级偏好共享owner、取消感知的串行存储及非阻塞已提交快照；Close停准入/取消并等待实际I/O、超时保留owner可重试，失败/旧版本不覆盖缓存，不选择后端或启动会话 |
| wait.go | Alva journal专属的共享UI/HTTP等待DTO、persistent opt-in、陈旧/跨分支拒绝、结果保存/同Turn继续/审阅放弃 |
| service.go | 产品session/Turn服务（Transcripts/Traces两个窄读取服务由bootstrap装配）、expected-branch原子准入、独立DraftID/版本显式选择与精确消费、20MiB UploadReader原文件引用与显式审批，持有独立Gateway/问答/会话外Developer及可选DeveloperWorkspaces/ConversationTabs owner；Transcripts为窄消息流读取服务，由bootstrap用实际store装配 |
| service_test.go | 真实双store/local harness、双观察者捕获/迟到发布/失败重试、上传/权限/历史/Inspector和其他engine等待不借Alva authority回归 |
| history.go | 产品绑定的有界metadata/字面关键词/正文range Tool与Plugin |
| tool_view.go | engine.ToolDisplay→ToolView折叠（overlay/merge）、ToolBody/ToolJump派生、字节估算、ToolTail/ToolLineSummary；不认工具名或引擎名 |
| tool_view_test.go | 起止Display折叠且不改事件值、各kind的展开来源与跳转、无Display旧事件为other、截断结果、Superseded待办（含向前翻页）、页边界合并与无Display部分补全、字节计入与WorkspaceFileTab只钉工作区内普通文件（含软链接拼写的工作区路径） |
| transcript.go / transcript_window.go | UI无关Block/BlockKind/BlockStatus（Block.History为折入的最新产品History序号，供轨迹跳转绑定轮次；Block.Tool为工具块ToolView）、TranscriptStore窄端口、TranscriptService分页（chunk条目不导出）与TranscriptWindow；迁入原desktop投影的用户输入/时间线/执行活动/思考与内容块/增量与最终回复/工具起止与引用/错误与重试/Completed状态传播/按attempt来源歧义，不是模型上下文 |
| transcript_test.go | 迁移的稳定ID/工具原owner/来源歧义回归，3000增量流式保留用户块且文字完整、最终消息替换、ApplyOlder边界合并、两端整块裁剪与字节上限、旧格式未合并数据、Turn传递与分页上限 |
| transcript_long_session_test.go | 长会话CI门（合成历史的工具事件带有界参数与Display）：真实BeginTurn/Record/FinishTurn写20轮×300增量并插入旧格式行，断言增量删除、Tail≥20条消息、流式用户块不丢、连续Older 50次不超字节上限；ALVA_LONG_SESSION_GIB合成GiB历史并断言Tail<100ms、输出Record p95 |
| inspector.go | 消费registered EngineInspector；分支/最多8hop真实typed child授权、向前/向后（InspectorBeforeAt，0=末尾）小索引/搜索/2KiB正文DTO/背压live metadata、默认禁用MCP导入 |
| trace.go | TraceService（Tail/Older/Newer/Around、Detail正文64KiB预算、Raw 2KiB页、向前字面Search）、TraceTarget/Child、TraceHint、TraceChunk；HubService.traceTarget区分无native记录的空分支与不支持的引擎 |
| trace_window.go | TraceStep/TraceRecord投影与TraceWindow：按元数据配对、ChildStepOf按link记录的parent找子Agent调用的子会话步骤、产品锚点轮次标注与无锚点段推断、轮内序号（窗口未含轮起点时标非精确）、远端字节裁剪且只收窄被裁一侧的产品扫描范围 |
| trace_layout.go | LayoutTrace：时长按真实时间，轮次/调用/子调用按组压缩（组宽按步数、组间留隙、组内保留真实相对时间，无时长时按序等分），输出与界面无关的0..1坐标与泳道 |
| trace_test.go | 真实双SQLite：跨页配对（向后/向前）、两端裁剪保留另一侧锚点、未声明BackwardPages的Inspector视为不支持、旧记录近似时间、轮次号与消息流Block.Turn一致、失败无锚点run推断、窗口裁剪后翻回不改标、子会话进入/返回与非link拒绝、工具intent查子步骤、无Inspector引擎与空分支、搜索跳转与详情/原始页、排布 |
| trace_long_session_test.go | 长会话轨迹门：从尾部打开固定10次元数据页、零正文读取、窗口字节与历史大小无关；ALVA_LONG_SESSION_GIB合成GiB native历史并断言首屏<100ms |
| export.go | 精确branch/typed child授权的bounded JSONL；HTTP只返回bytes，native显式保存到私有exports，不接受client host/native path |
| approvals.go / approvals_test.go | 真实running Alva root/typed child权限路由；首次读前冻结实际Runtime四项binding，替换owner与native同名字符串均不借用外部engine权限回调，不接受UI childID或借用父权限 |
| engine_interactions.go / engine_interactions_test.go | 非Alva的已提交native审批/问题冷快照、session/branch与完整physical binding的opaque handle；响应重审原handle、种类隔离，迟到点击不重新发现替换owner；不借用Alva broker |
| aep.go / aep_test.go | native/operator-only显式source检查、JS/Python解释器或嵌入WASIp1模式绑定、disabled pin/批准/变更拒绝；不执行检查或开放HTTP任意path导入 |
| commands.go | native/operator-only显式Hooks/diagnostics JSON disabled编辑，不读取或执行命令，也不作为HTTP任意命令导入 |
| tools.go / questions.go | native严格工具JSON编辑与显式下一Turn开关；actual root/typed ancestry过滤的独立真人问题/回答，不借用审批授权 |
| browser.go | native/operator-only browser严格JSON disabled编辑，不发现exe/启动进程/导入用户profile，不是HTTP任意host-path入口 |
| skills.go | saved cwd受管技能初始化、精确文档、Install/Archive/Restore |
| compatibility.go | saved cwd内显式JSON来源冷检查与下一Turn启停；不读取全局目录、不执行模板/脚本 |
| media.go | 已存branch→native的冷媒体索引/manifest/完整哈希预览/receipt，共用Library且不启动decoder/model |
| playback.go | 显式本地Play的product/branch/transcode-only归属；视频需独立decoder批准并构造caller-owned Playback，音频返回SHA/canonical验证PCM bytes；不发模型/新receipt |
| skills_test.go | 完整frontmatter、归档receipt与symlink祖先拒绝 |
| locks.go | 同一实际SDK ToolLockRegistry装配Plugin，非第二执行runtime |
| team.go | 有界owned Worker队列pumps、显式resume/stop、drain边界kick与账户路由准入；Stop先冻结pump，再由Runtime join实际owner，涵盖独立同Turn等待续轮；StopExecution保留UI身份，旧ID不取消replacement或pump |
| team_test.go | 实际完整Alva Worker FIFO/同native续轮、observer detach与byte-only上传预算；等待续轮Stop超时保留真实清理与后续队列，Stop成功发布前禁下一claim、迟到旧ID拒绝后FIFO继续 |
| team_tool.go | 绑定实际Lead/native的hub_team Tool/Plugin，mutation HITL、roster与独立8KiB路由分页/绝对索引、selected task 2KiB读取 |
| team_tool_test.go | foreign native拒绝、mutation拒批零变更与显式批准真实Team回归 |
| automation.go / automation_test.go | 显式owned本地timer与真实SDK普通Turn；构造不执行、下一完整间隔、无补跑/OS daemon，连接revision失效零Turn并经显式重审恢复，关闭先阻止准入再交既有Runtime收尾 |
| shell.go | 已存分支授权的应用内后台job冷状态/真实Stop/idle reset，无PID或nativeID客户端输入 |
| worker_draft.go / worker_draft_test.go | Worker引用及typed文件/Skill共用冻结器入原FIFO，重验真实Worker会话/branch，不借Lead/sibling/未采纳home原件；手动正文与增强prompt分别保存，失败保留稿、入队不执行 |
| worktree.go / worktree_test.go | 已认领Lead/Team/Worker隔离、冷SHA/raw diff与双workspace租约采纳；真实Git完成后按原binding/revision收尾，不重做fresh idle gate；Waiting阻止新写动作而保留只读审阅 |
| composer_references.go / composer_references_test.go | 实际owner校验与执行准备时原件/派生字节重验，context_read/search有界分页/长文检索；排队后损坏在HTTP前失败，不重放图像或扩大主机路径权限 |
