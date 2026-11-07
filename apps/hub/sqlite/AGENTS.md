# L7 · Hub SQLite product authority
> 独立 product.sqlite 保存产品事实，不复制或恢复 native 执行正文。
## 地位
具体存储依赖 `hub` 与安全SQLite宿主封装；Hub core 不反向依赖本目录。
## 逻辑
独占进程锁后打开0700/0600私有库，先完整校验schema再修复中断Turn。新Turn和首Attempt、fallback认领、事件/终态都在事务提交；固定产品归属与activeAttempt防止迟到结果覆盖新轮。schema v6延续v1→v2偏好、v3连接/credential分表、v4目标/模型偏好和v5伙伴/群/资源视图，原子增加手动输入草稿与文件原件回执metadata；会话参数覆盖在已有Settings JSON内独立CAS。私有文件不等于加密或OS密钥库。
## 约束
首次版本不读取旧Rust数据；user_version=0但已有任何业务表的库拒绝准入，不向未知库补建Hub表。最多64分支、7个授权fallback路由；history每页128项。增量是派生显示数据：最终正文或其Reference提交后在同一事务删除，不改写旧行、不加迁移；截断且无Reference时增量是唯一完整正文，保留。handoff只传32条有界Turn索引，超量明确要求检索，不静默声称完整历史。Stop/可能副作用禁止自动fallback。实例锁释放与DB关闭由Store.Close显式拥有。
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
| composer_drafts.go / composer_drafts_test.go | 逻辑home/会话手稿、引用回执与保留空版本的CAS；首发共用创建政策原子绑定/复用Session，真实v5→v6升级/故障回滚、跨窗竞争/重开及旧回执隔离，不恢复执行 |
| workflow_migration_test.go | 冻结v4事实的v5原子升级/故障回滚、连接/目标/模型偏好保留与schema失败前禁止执行恢复 |
| permissions.go / permissions_test.go | 空闲native下一Turn权限事务、Full显式确认、普通导入不升权与Plan清除潜在Full |
| wait.go / wait_test.go | 独立WaitStore、branch归属审阅、同Turn新Attempt事务、waiting冻结与Worker/计划crash edge/no fallback回归 |
| store.go | schema与v1至v8事务升级，创建ledger/约束失败先回滚再禁止恢复；v7→v8追加repository/tab set且校验后再恢复，产品分支/handoff/分页历史与中断修复，所有Attempt准入拒绝已删除伙伴/群，不重置或换库；最终Message/reasoning内容块同事务删除同turn/attempt/ID的已合并增量（截断且无Reference时保留），History/HistoryBefore经LEFT JOIN hub_turns填TurnSequence |
| history_compaction_test.go | json_extract可用且走hub_events_turn索引、只删本attempt已合并增量、截断保留、TurnSequence与旧格式未合并行照常读取 |
| preferences.go / preferences_test.go | 应用级偏好 singleton、共享政策/乐观 revision 原子保存；左栏宽度/折叠与外观稀疏CAS、显式默认恢复及重开，新库、真实 v1 升级、并发、取消及损坏不重置回归 |
| store_test.go | 可逆路由/安全fallback、native事实归属、commit失败/忽略sink错误不能成功/大handoff/恢复回归 |
| plan_mode.go / plan_mode_test.go | 事务内只改PlanMode且保留批准；统一beginAttempt拒绝Plan借外部引擎，覆盖Worker/自动化/fallback/等待恢复 |
| settings.go | session-scoped默认禁用编辑、独立host确认启用与idle-only耐久snapshot |
| settings_test.go | 配置导入不自批、归属/active限制和关闭重开耐久回归 |
| compatibility_test.go | 项目相对配置路径默认off、导入不启用、idle-only与关闭重开保存；provider不读文件 |
| tools.go / tools_test.go | 六类可选工具默认off、编辑清授权、Shell/Web独立access批准、idle-only和重开耐久 |
| aep.go / aep_test.go | inspected AEP pin独立批准/编辑失效/idle-only与重开耐久 |
| commands.go | Hooks/diagnostics编辑清批准、独立explicit enable、idle-only的next-turn进程snapshot |
| media.go / media_test.go | 默认off媒体能力、decoder编辑清批准、独立host同意、idle-only和重开耐久回归 |
| browser.go / browser_test.go | 显式fresh-browser snapshot编辑禁用、独立批准、重开与active不变回归 |
| native_owner.go | native root到唯一product owner的metadata-only反查，不读native正文 |
| sessions.go / sessions_test.go | 首页全历史keyset/字面检索，仅主会话、不把Worker当无关会话 |
| navigation.go / navigation_test.go | 同一主会话过滤下的全库workspace聚合/稳定路径cursor、精确项目子会话keyset和100计划全局只读索引；保留空owner原义、location过滤不推创建来源，无新schema或执行策略 |
| timeline.go | shared product clock下分支/fallback/handoff typed时间线 |
| team_snapshot.go / team_snapshot_test.go | 同事务Lead完整roster/Worker仅self的handoff metadata，队列不消费、16worker/16KiB与重启冻结证据 |
| handoff_policy.go | 保存和engine编码前复用shared known-credential redactor，不复制regex或称任意秘密检测 |
| team.go | 独立Worker会话/FIFO事务、typed正文/文件/Skill可选入队与真实catalog/grant/branch重验；claim复用beginComposerTurn，排队Edit保留冻结输入与手动正文，公平资源预约/真实终态不变 |
| team_test.go | Worker身份/路由/队列、休眠/归档/恢复、crash未知不重放回归 |
| activity_test.go | native build/prepare启动前Activity耐久风险与6类effect fallback回归 |
| provider_failure_test.go | 真实Alva HTTP provider分类驱动fallback，mixed SessionError与prepare风险禁止重放 |
| automation.go / automation_test.go | owner-scoped乐观计划CRUD、明确计费批准、普通Turn同事务认领/收尾；busy/missed跳过、Stop/崩溃暂停且绝不自动fallback/重放 |
| worktree.go / worktree_test.go | 首Turn前one-way隔离、cwd原子绑定、revision证据/明确adoption；独立removal review/明确删除后retire，不retarget历史；未就绪/未知准入阻断与crash未知不重放 |
| errors.go | SQL内部错误到共享Hub语义的边界映射 |
