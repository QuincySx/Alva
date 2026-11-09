# L7 · Hub SQLite product authority
> 独立 product.sqlite 保存产品事实，不复制或恢复 native 执行正文。
## 地位
具体存储依赖 `hub` 与安全SQLite宿主封装；Hub core 不反向依赖本目录。
## 逻辑
独占进程锁后打开0700/0600私有库，先完整校验schema再修复中断Turn。新Turn和首Attempt、fallback认领、事件/终态都在事务提交；固定产品归属与activeAttempt防止迟到结果覆盖新轮。schema v6延续v1→v2偏好、v3连接/credential分表、v4目标/模型偏好和v5伙伴/群/资源视图，原子增加手动输入草稿与文件原件回执metadata；会话参数覆盖在已有Settings JSON内独立CAS。私有文件不等于加密或OS密钥库。
## 约束
首次版本不读取旧Rust数据；user_version=0但已有任何业务表的库拒绝准入，不向未知库补建Hub表。每个版本最多64分支（整个会话至多256）、每会话至多64条版本线、7个授权fallback路由；单个Turn的prompt存储上限64KiB（store.go，超出拒绝创建；HTTP入口更严，为32KiB）；history每页128项。增量是派生显示数据：最终正文或其Reference提交后在同一事务删除，不改写旧行、不加迁移；截断且无Reference时增量是唯一完整正文，保留。handoff只传32条有界Turn索引，超量明确要求检索，不静默声称完整历史。Stop/可能副作用禁止自动fallback。实例锁释放与DB关闭由Store.Close显式拥有。
所有公开Store错误在provider边界映射共享hub.ErrNotFound；product/Tool/HTTP不得识别sql.ErrNoRows。内部SQL原因可保留供诊断，事务/schema行为保持。应用偏好按 expected revision 原子更新，与会话设置独立；v2 缺失/损坏偏好在恢复前失败，不补建、不重置。未知较新 schema 拒绝且保留原数据。

Waiting保留Turn身份、Worker Task及Automation的RunningTurnID；运行资源可以已退出，队列/timer不因此认领下一轮。只实际已开始且有合法checkpoint receipt的Attempt能提交Waiting；crash edge恢复保留该事实，无自动继续。

## 业务域清单
| 文件 | 职责 |
|---|---|
| connections.go / connections_test.go | 连接/credential分表CRUD与CAS；v7 RequestID创建ledger同事务保存脱敏初次回执，恢复可见已编辑/已删除事实，不存秘密或其hash；严格单列PK/全局唯一约束在恢复前检查 |
| goals.go / goals_test.go | 目标稳定事实、RequestID幂等/冲突、范围绑定keyset分页及v3→v4原子升级，不隐式创建会话 |
| model_preferences.go / model_preferences_test.go | 独立模型参数/收藏CAS、失败原子回滚及重开；真实Attempt冻结和Waiting续轮沿用旧参数，不支持的目标零准入 |
| session_model_options_test.go | 会话覆盖CAS/重开/清除、普通Settings导入保留、运行中只影响下一新Turn及Waiting冻结 |
| companions.go / companions_test.go | 稳定Profile/群、隔离会话、成员快照与有限round/message原子记录；删除索引、seq/(created,id)绑定keyset页及统一Attempt事务tombstone拒绝；258消息/129轮、Wait续轮回归 |
| companions_rounds.go | 同回合进度CAS、固定Turn身份、幂等回复补投与阻断未审阅的等待/未知回合 |
| companion_skill_scope.go / companion_skill_scope_test.go | 伙伴会话的实际Skill允许列表、空闲native准入、普通导入不扩大与重开冻结回归 |
| composer_files.go / composer_files_test.go | 原件/派生文件只保存不可变metadata、分块SHA与home/session来源；显式pre-session捕获catalog、幂等保存/冲突、最大文件metadata预算、重开与有界recent索引；home→session grant只由真实Pending事务认领，普通session稿不能铸造归属，不复制文件正文 |
| workspace_views.go / workspace_views_test.go | 会话资源标签/顺序/宽度偏好CAS、branch归属、深拷贝和失败原子性，旧view不冒充保存成功 |
| developer_workspaces.go / developer_workspaces_test.go | 显式repository与worktree-scoped tab set的v8事务/CAS/归档/重开；tab/derived home draft身份跨scope事务内全局唯一且归档不释放、既有tab不可删除；SessionID只保留首Send绑定，原子复用createSessionTx+draft认领/文件grant，失败全rollback且不建Turn/样例；跨scope身份扫描只在next引入本scope未持有的tab/DraftID时执行，选择/归档/排序复用既有身份即跳过，启动仍全库校验唯一性 |
| composer_drafts.go / composer_drafts_test.go | 逻辑home/会话手稿、引用回执与保留空版本的CAS；新建会话同事务写入默认开启持久等待的设置（旧会话已存值不改）；首发共用创建政策原子绑定/复用Session，真实v5→v6升级/故障回滚、跨窗竞争/重开及旧回执隔离，不恢复执行 |
| workflow_migration_test.go | 冻结v4事实的v5原子升级/故障回滚、连接/目标/模型偏好保留与schema失败前禁止执行恢复 |
| permissions.go / permissions_test.go | 空闲native下一Turn权限事务、Full显式确认、普通导入不升权；开关Plan不改权限档 |
| routing_test.go | 引擎键分支、同引擎换模型保留原生会话（含旧整路由键行）、fallback仅当轮、过期fallback可见跳过、端点迁移审计与通知、FinishTurn失败不卡会话、长错误保留前缀 |
| wait.go / wait_test.go | 独立WaitStore、branch归属审阅、同Turn新Attempt事务（续接用进入waiting那次attempt自己的路由，只比较引擎）、waiting冻结与Worker/计划crash edge/no fallback回归 |
| store.go | schema与v1至v11事务升级（v9应用设置、v10排队、v11版本线各是独立的`if version < N`块，同一初始化事务内完成并设置user_version，失败整体回滚），分支按引擎一条（route列写engine:<id>，旧行按data里的引擎命中，无schema改动），Switch同引擎换模型复用分支（旧数据里若该模型曾有自己的同引擎旧分支，选择规则不变：优先活动分支，旧分支的上下文留在原处不合并，不另行提示——查找成本与误报风险高于收益，此为已知取舍），RecoverSession依赖单进程独占（Open持有数据库独占锁，TestRecoveryAndExclusiveDatabaseOwnership；desktop与server不会同时打开同一产品库，进程内由Runtime保证无存活owner），RoutesSeen按RouteKey分别记录各路由上次revision/端点（旧分支以分支当前路由的BillingRoute为基线），fallback用attemptBranch不改活动分支，RecordRoute/RecordFallbackSkip/RecoverSession；创建ledger/约束失败先回滚再禁止恢复；v7→v8追加repository/tab set且校验后再恢复，产品分支/handoff/分页历史与中断修复，所有Attempt准入拒绝已删除伙伴/群，不重置或换库；最终Message/reasoning内容块同事务删除同turn/attempt/ID的已合并增量（截断且无Reference时保留），History/HistoryBefore经LEFT JOIN hub_turns填TurnSequence |
| history_compaction_test.go | json_extract可用且走hub_events_turn索引、只删本attempt已合并增量、截断保留、TurnSequence与旧格式未合并行照常读取 |
| preferences.go / preferences_test.go | 应用级偏好 singleton、共享政策/乐观 revision 原子保存；左栏宽度/折叠与外观稀疏CAS、显式默认恢复及重开，新库、真实 v1 升级、并发、取消及损坏不重置回归 |
| store_test.go | 可逆路由/安全fallback、native事实归属、commit失败/忽略sink错误不能成功/大handoff/恢复回归 |
| plan_mode.go / plan_mode_test.go | 事务内只改PlanMode且保留批准与权限档；统一beginAttempt拒绝Plan借外部引擎；测试只覆盖切换路由后启动外部引擎、已授权外部fallback、批准与权限档保留及持久化、已准入/外部分支被拒，Worker、自动化与等待恢复没有对应的计划模式测试 |
| settings.go | session-scoped默认禁用编辑、独立host确认启用与idle-only耐久snapshot |
| app_settings.go / app_settings_test.go | v9显式迁移：hub_app_settings单行意图（revision CAS，落盘前清除主机批准位）与hub_workspace_approvals（工作区/类型/项/指纹）；会话覆盖的开始/改回、配置编辑（变更项关闭）与逐项启用 |
| settings_test.go | 配置导入不自批、归属/active限制和关闭重开耐久回归 |
| compatibility_test.go | 项目相对配置路径默认off、导入不启用、idle-only与关闭重开保存；provider不读文件 |
| tools.go / tools_test.go | 六类可选工具默认off、编辑清授权、Shell/Web独立access批准、idle-only和重开耐久 |
| aep.go / aep_test.go | inspected AEP pin独立批准/编辑失效/idle-only与重开耐久 |
| commands.go | Hooks/diagnostics编辑清批准、独立explicit enable、idle-only的next-turn进程snapshot |
| media.go / media_test.go | 默认off媒体能力、decoder编辑清批准、独立host同意、idle-only和重开耐久回归 |
| browser.go / browser_test.go | 显式fresh-browser snapshot编辑禁用、独立批准、重开与active不变回归 |
| native_owner.go | native root到唯一product owner的metadata-only反查，不读native正文 |
| sessions.go / sessions_test.go / sessions_manage_test.go | 首页全历史keyset/字面检索，仅主会话、不把Worker当无关会话；重命名/置顶/软删除在同一事务里读后写（不改UpdatedAt与Status；删除拒绝运行/等待中的会话，置顶上限32，Worker与伙伴会话不可管理），软删除由notDeletedFilter统一从列表/搜索/项目导航/置顶中隐藏，数据与原生记录全部保留，已删除会话拒绝新一轮；所有产品对会话的读写统一经liveSessionQuery，已删除会话在Detail/Settings/SetPlanMode/SetFallback/History/Switch/准入等处一律NotFound，仅DetailIncludingDeleted（内部管理读）能见；删除在同事务内拒绝仍有运行/等待Worker或运行中Worker任务的Lead（ErrSessionHasWorkers），并把该会话所有启用的自动化暂停（LastError=所属会话已删除，自动化列表可见原因，不再到点）；continue准入在同事务内要求最新一轮为cancelled（ErrNothingToContinue）；标题拒绝控制/格式字符（含孤立CR、NUL、bidi控制）；列表读（PageSessions/PageWorkspaces/PageWorkspaceSessions/PinnedSessions/LatestTurn）走独立只读连接（Store.list，query_only），写连接保持单连接串行，所以排在长事务后的列表读不再让左栏和搜索一直「加载中」；回归TestListReadsDoNotQueueBehindTheWriteConnection |
| navigation.go / navigation_test.go | 同一主会话过滤下的全库workspace聚合/稳定路径cursor、精确项目子会话keyset和100计划全局只读索引；保留空owner原义、location过滤不推创建来源，无新schema或执行策略 |
| timeline.go / approval_settled_test.go | shared product clock下分支/fallback/handoff typed时间线；RecordApprovalSettled追加Alva已决定审批一行（工具展示取自该调用已脱敏的tool_start，每attempt最多256条，计数存在attempt记录里不扫事件；只有根会话自己的调用取其展示） |
| team_snapshot.go / team_snapshot_test.go | 同事务Lead完整roster/Worker仅self的handoff metadata，队列不消费、16worker/16KiB与重启冻结证据 |
| handoff_policy.go | 保存和engine编码前复用shared known-credential redactor，不复制regex或称任意秘密检测 |
| team.go | 独立Worker会话/FIFO事务、typed正文/文件/Skill可选入队与真实catalog/grant/branch重验；claim复用beginComposerTurn，排队Edit保留冻结输入与手动正文，公平资源预约/真实终态不变 |
| team_test.go | Worker身份/路由/队列、休眠/归档/恢复、crash未知不重放回归 |
| activity_test.go | native build/prepare启动前Activity耐久风险与6类effect fallback回归 |
| provider_failure_test.go | 真实Alva HTTP provider分类驱动fallback，mixed SessionError与prepare风险禁止重放 |
| automation.go / automation_test.go | 计划按自己批准的路由运行（只比较引擎，手动换同引擎模型不使其失效）；owner-scoped乐观计划CRUD、明确计费批准、普通Turn同事务认领/收尾；busy/missed跳过、Stop/崩溃暂停且绝不自动fallback/重放 |
| worktree.go / worktree_test.go | 首Turn前one-way隔离、cwd原子绑定、revision证据/明确adoption；独立removal review/明确删除后retire，不retarget历史；未就绪/未知准入阻断与crash未知不重放 |
| errors.go | SQL内部错误到共享Hub语义的边界映射 |
| pending.go / pending_test.go | v10 hub_pending_inputs与队列操作：至多8条/16 KiB、编辑只限queued、取消不含interjecting；InputDelivered回执在写事件的同一事务里删除该输入，FinishTurn在终态事务里把无回执的插话按原顺序放回队首，崩溃恢复把interjecting标uncertain且绝不自动重发；派发由准入事务内的DELETE（要求state=queued）保证一条输入至多一个Turn |
| lines.go / lines_test.go | v11 hub_lines与hub_branches加line列、唯一约束(session,line,route)（建新表→拷贝→删旧→改名，同事务，迁移测试核对旧行逐字节不变；v10失败整体回滚）；forkAnchor只用主键/(turn,seq)索引查找（引擎自报的原生turn id与事件引用的原生记录序号作绝对锚点，无锚点即降级），不扫轮；DegradeFork在一个事务内把未花掉的分叉意图转成交接索引并写fork_degraded；forkLine在Turn准入事务内物化原线、截断父线区段、关闭父线开放段、建新线与按引擎分支（原生分叉意图先落库）；SelectLine仅空闲且恢复该线分支与状态；History/HistoryBefore按线分段、每段一次(session,seq)索引范围查询，无版本会话仍走原查询；handoff/LatestTurn按可见轮过滤；ForkSession不复制历史，写ForkedFrom与原生分叉意图或有界轮索引；团队/伙伴/自动化会话、带附件或技能与继续生成的轮不支持版本 |
