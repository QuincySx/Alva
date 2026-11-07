# Hub 业务流程验收

本表把用户动作、服务与持久化结果连起来。页面可见、Go 测试、真实窗口操作和真实模型请求分别验收；静态 UI 或一次成功回复不能替代整条流程。当前实现与本轮实际结果见根 [MIGRATION](../MIGRATION.md)。

## 业务职责与入口

先按[业务域与入口覆盖表](./ARCHITECTURE.md#hub-页面与业务落地)找到实际owner，再读[Hub目录地图](../apps/hub/AGENTS.md)。业务域按会话与路由、人类交互、执行调度、连接与参数、目标与引用、伙伴与群、资源观察、外观、语音分别维护；桌面页面或HTTP路由不是新的业务权威。

| 一次用户动作经过哪里 | 该层负责什么 |
|---|---|
| `desktop` / `server` | 草稿、焦点、交互状态或认证/请求解析；传递明确选择和预期revision，不复制准入规则 |
| `product` | 业务校验、范围/版本绑定及服务编排；所有入口复用同一政策 |
| Hub Runtime / 各自调度owner | 认领实际执行、串行回调、取消和真实join；FIFO、timer、群回合各有独立owner |
| 存储契约 → `sqlite` 或native Repository | 产品事务/CAS/耐久事实与native原件各守其权威；执行存储仍只有六方法 |
| `bootstrap` | 一次选择并装配实际组件，按真实owner收尾后关闭；不在每个页面重复配方 |

新增或修改功能同时记录触发方式、输入/选择快照、准入、提交事实、失败后保留内容、取消/迟到回执及重开行为，并补对应验收行。UI实现、共享服务、SQLite事务、HTTP曝光、真实账户和平台验收分别标记；共享服务存在不能当作全部入口已完成。

## 原生顶部行增量 · 2026-11-05

正式依据及静态来源见[原生顶部壳记录](./design/alva/exports/archive/native-shell-2026-11-05/report.md)，当前安装和测试回执见根MIGRATION的同日原生标题栏条目；这一增量取代下面早期72图标轨的布局描述。

| 用户动作 | 共享事实与边界 | 验证入口 |
|---|---|---|
| 用原生系统按钮，收起或展开左栏 | 系统owner保留，移除假按钮/独立32条；实时native safearea内共用36内容行。侧栏收起整棵隐藏，无rail，宽度220–432和已有稀疏CAS不变；不Stop、不清草稿 | `window_chrome*`、`workspace_shell*`、`workspace_layout*`；实际窗口收起/展开与草稿/原README保持通过，系统按钮点击/整窗拖动待平台验收 |
| 从设置或新建连接返回 | 唯一实际返回在主Header，原handler/保存守卫不变，卡片不重复返回 | `workspace_settings*`、`TestNativeConnections*`；实际空OpenAI表单经来源、模型设置回到同一completed会话与空草稿，无保存/模型请求 |
| 有待处理审批或问题时选择供应商 | 来源卡片外层按剩余高度滚动、内部不挤扁；正常尺寸/居中不变，选择与pending分别绑定原owner | `TestWorkspaceSettingsPendingSourcesKeepEveryChoiceReachable`、`TestWorkspaceSettingsSourcesKeepUncontendedCardCentered`，左右native事实/五种选项与真实pending，回答/批准/保存计数保持0 |

中间真实截图曾暂时缺Panel/More，后续实际More→设置→顶部返回通过且图标正常；2×无窗口相同布局操作序列的7帧图标像素不变、未复现，但没有覆盖AX SetValue/原生GPU/系统捕获，缺图原因仍待鉴别。原生红黄绿被系统胶囊覆盖，AX三种系统动作存在，不能据此代替按钮像素/点击验收。测试、无窗口图、真实窗口与其他平台范围分别保留。

## 工作台结构复核 · 2026-11-05

正式21/22是本轮组合依据，旧首页或单独控件测试不能代替完整工作台验收。保持设计包修订，具体整合和实机结果见根MIGRATION及本轮ui-parity检查点。

| 用户动作 | 共享事实与边界 | 验证入口 |
|---|---|---|
| 展开项目及子会话，翻页或重试 | 可选WorkspaceNavigationStore读取普通会话已存路径；项目8条、子会话16条独立分页。失败不提前消费cursor/back，旧epoch不覆盖新页，不创建空会话 | product/sqlite `navigation_test.go`；desktop `workspace_navigation*test.go` |
| 首页打开自动化 | 可选AutomationBrowseStore有界读取全部owner的真实计划，点击后冷验精确已存任务；实际CRUD复用原服务。浏览不启用计划、Tick或执行 | `workspace_automation_index_desktop_test.go`，隐藏读取后设置返回同样结算 |
| 设置切换8类及选择任务 | 应用级配置与任务范围分开；选择经原导航保稿，匹配Detail回执后才Open，正常poll不抖动可用性 | `workspace_settings*`、`workspace_navigation_boundaries_desktop_test.go` |
| 会话同时打开右侧文档 | 用户消息右对齐、回复头像及真实身份，与432px文档面一起经过实际page.Build组合；同名文件以真实身份建key，路径区分操作；查找限当前载入内容，原始数据单独打开 | `workspace_transcript*`、`workspace_resources*`；明暗窄窗整页及真实Codex已完成会话→根README→Find→收起/重开通过，不据fixture宣称模型账户通过 |

## 日用路径增量 · 2026-11-05

本轮优先补正常桌面启动、双侧布局和 Codex/Alva/Pi 执行。下面记录可复现动作及证据边界；聊天定位点条仍按用户允许后置，Anthropic/Claude 不启动真实联调。它不是整个软件完成率。

| 用户动作 | 实现与必须保持的事实 | 本轮验收边界 |
|---|---|---|
| 从 Dock 打开唯一 Alva | 无参数只消费安装者固定 bundle manifest；注册明确的 Codex/Pi 及 Alva，普通 CLI 参数独立，不发现或复制认证 | 三个入口的普通/race 回归各32项通过，真实安装/重启另记 MIGRATION |
| 折叠/展开左栏，拖动两侧分界，缩窄窗口 | 左栏220–432，收起整棵隐藏；展开时左右共享指针/键盘splitter。窄窗只夹取显示，资源专注保留返回；观察/折叠不 Stop | 左右布局、偏好稀疏 CAS、真实 SQLite 重开、旧回执/失败/取消/关窗/运行 owner 回归；平台鼠标拖动单独验收 |
| 保存布局时同时编辑外观 | 同一 PreferencesService 只提交实际改动的布局字段；width/collapsed独立版本，另一窗的宽度/折叠不被旧值覆盖。实际存储回执前阻止关闭，已取消观察不陷入关窗循环，失败保留本地意图 | 新增9项持久化及3种双窗确定性CAS冲突回归；不复制 General 未保存表单 |
| Codex 新发 → 读取文件 → Stop → 同 thread 再发 | 本机明确 CLI 配置，工具开始/结束成对、原 native thread 不变；取消必须真实 join | 本机0.165.0真实账户三步通过，两次新随机文件内容匹配；不借此推定真实人类审批/多问题全通过 |
| Pi 新发 → 读取文件 → Stop → 原生恢复 | 显式 RPC 和固定内容哈希扩展，握手/私有 session 验证后才 prompt；审批在写/编辑/bash执行前，问题使用独立callback | 本机1.4.0真实账户三步通过，两次工具读取匹配、Stop cancelled、native ID不变；写入批准/拒绝与问答另由真实CLI+本地HTTP验证，不冒充付费模型决定 |
| Alva 原生执行与页面回执 | 保存的连接交给既有 native harness/provider；演示明确标识；与外部 CLI 的授权和原件分别管理 | 实际 bootstrap/SQLite/MyGo 无窗口、本地 HTTP 流程单独验；本轮没有把 Demo 当作真实远程账户 |
| 后台检查 UI | MyGo `ui.NewTester` 操作实际控件、内存渲染；e2e 构建才可选官方 Unix socket 自动化 | 无窗口布局/交互与运行窗口自动化分别记录；后者仍是实际窗口，不把 socket 或静态图算无窗口业务验收 |

共享颜色消费当前 Foundation `ui.color.*`（不是旧原型 `color.*`）；Primary 使用 text/surface，输入普通态 subtle、焦点 surface，Secondary 各状态沿已保存变体。这里只同步共享角色/输入家族，不据此宣称所有页面的阅读容器、活动卡片或平台字体均已逐像素对齐。

## 逐模块对齐顺序 · 2026-10-26

本轮从共享控件、输入区、连接和通用设置开始。每个模块都沿同一条链核对：功能 SPEC → Foundation Token/组件/变体 → 页面交互 → 共享产品服务 → 存储事实 → 失败/取消/重开 → 实际画面。修改共享控件后检查真实消费者；页面不复制服务准入、SQLite 或取消 owner。

| 模块 | 规范与实现入口 | 本轮推进与仍需核对的部分 |
|---|---|---|
| 基础控件 | [组件约定](./design/alva/COMPONENTS.md)、`desktop/workspace_controls*` | 统一 Text/Leading Loading、保宽/保焦点/防重、系统减弱动效，校准 Ghost/Danger 与焦点环；Input/Select/Checkbox 和全部状态仍需后续逐项复核 |
| 工作台与输入 | 原生 `home-composer` SPEC、21/22、`workspace_composer*` / `workspace_picker*` | 按钮和发送快捷键共用可发送判断，提交/停止 Loader、当前权限图标、Full 确认按钮对齐；发送策略已确认见GAP-09，仅手动草稿，不自动插话/排队 |
| 模型连接与参数 | 原生 `providers` / `navigation` SPEC、23–26/39/42/43、`ConnectionsService` / SQLite / `workspace_connections*` | 获取/测试/保存接共享 Loading，保留重试动作名、焦点和真实取消；模型选择/参数浮层继续逐项核对。本机账户管理及可信容量仍见 GAP-02/10 |
| 通用设置 | 原生 `preferences` SPEC、31、`PreferencesService` / SQLite / `preferences_desktop*` | 保存接共享 Loading，存储返回前不提交外观、不重复写入；语言/密度暂隐藏，应用减弱已接CAS和真实Loader，边界见GAP-04/05/06 |
| 问答、审批与等待 | `home-composer` 执行契约、40、`product/approvals` / `wait` / desktop 同名入口 | 逐请求的选择/自由输入、保存/继续、超期/冲突/取消和回焦点待按当前组件完整复核；Codex双向回调已接、真实验收边界见GAP-01 |
| 目标、项目与附件 | `home-composer` Add/References 契约、`GoalsService` / workspace references / SQLite | 已有创建/重用/原件/明确 Send 的业务回归；表单、长内容、首发失败重试和跨窗引用需按当前规范复核，首页范围缺口见 GAP-03 |
| 语音 | `req_alva_composer_voice`、`workspace_voice*` / `platform/system/speech` | 录音点/计时、转写 Loader/文字与独立 Stop 对齐；macOS 设备/许可/声学质量仍见 GAP-08，不把假 recorder 算实机通过 |
| 伙伴与群 | 27–30、`product/companion*` / SQLite / `workspace_companions*` | 已有 CRUD、隔离历史/范围和真实 owner 回归；设置/选择/取消、长历史与群回合的 UI/逻辑需整域复核 |
| 右侧资源与 Inspector | 22、`product/workspace_views_resources*` / resource view store / `workspace_resources*` | 关闭/退订不 Stop、分页/重开/弹出已有回归；标签与各详情种类逐项复核，拖拽/改宽仍见 GAP-07 |
| 工具、插件与规则 | 32/33、`product` 配置与 bootstrap 装配、desktop tool/AEP/Skill panes | 逐项核对编辑/检查/独立批准/下一轮生效、失败保稿和明确禁用；不从可见开关推断授权或执行 |
| 调度、自动化与 Worker | `product`、各独立调度 owner、SQLite、desktop automation/team | 复核暂停优先 CRUD、FIFO/等待续轮、真实 Stop 和故障回执；Run once 不属于当前已确认契约，不自行增加策略 |
| Gateway、开发者测试与诊断 | 36–38、`apps/developer` / `product` / desktop 对应入口 | 复核冷预览、显式 Run/Stop、精确路由 revision 与实际报告；HTTP 入口覆盖和真实窗口/可访问性仍见 GAP-11/12 |

表中“本轮”只表示该范围进入施工和验收，不表示整域完成。专项检查结果写入根 MIGRATION；旧测试或旧图只保留其原范围。基础/功能规则沿用现有正本，缺少明确功能契约的模块先补文字范围与状态，再维护必要代表组合；不为每个浮层或回执复制页面。

2026-10-26追加：正式设计稿已更新14页，范围与原生图审查见[设计记录](./design/alva/DESIGN.md#正式页面稿更新--2026-10-26)。这批改变了设置壳、模型目录和部分阅读布局；后续原生同步以各模块记录为准，静态页面不能直接标记业务闭环。

模块关闭至少要有：可复现的用户动作、实际服务/持久化结果、失败保稿与取消/迟到结果、重开行为，以及相同主题/尺寸下的 UI 检查。测试、无窗口渲染、真实窗口/设备和外部账户分别记录；存在待决策略或平台缺口时不改成“全部完成”。

## 2026-10-27 模块闭环增量

当前范围补齐交互回执、资源布局与窗口返回、连接新建恢复、Worker typed FIFO、伙伴/Goal、自动化/worktree和设置/Gateway生命周期；共享服务与存储仍各守原 owner。下表只记录本批可复现边界，整体检查与真实窗口结果见根 MIGRATION。 本批全仓2981项/98包通过，5项跳过；唯一客户端已更新，但资源系统窗口走查因CUA挂接失败仍未验收。

| 用户流程 | 必须保持的事实 | 实际回归入口 |
|---|---|---|
| native 回应成功但 resolved 尚未提交 | 隐藏已接收请求，拒绝重复；失败保留可重试，不制造 resolved | Runtime/desktop engine interactions 与 approvals/wait tests |
| 拖动资源标签、改宽、重开 | CAS保存顺序/宽度；临时夹取不覆盖偏好，旧 metadata 读取取消隔离 | `TestWorkspaceResourcesPointerAndKeyboardReorderResizePersistAndClamp`、`TestWorkspaceResourcesMetadataReadCancelsAndIsolatesSessionABA` |
| 长标签、分页、弹出与返回 | 单行52页头；完整关闭/双分页动作；返回已有来源窗或当前窗恢复，观察动作不 Stop | `TestWorkspaceResourcesSelectedLongTabRevealsCloseAndUsesSurface`、`TestWorkspaceResourcePopupReturnAndPageActionsStayVisible`、windows controller tests |
| 新建连接未知回执后显式恢复 | 原 RequestID 一次创建、不可变脱敏回执；后来编辑/删除不被旧请求重放 | `TestConnectionServiceUncertainCreateExplicitResolveAndReplay`、SQLite v7 schema/connection tests |
| Worker 附文件/Skill排队、重开和等待 | 属于实际 Worker，会话/branch/字节冻结；保存答案不执行，明确同 Turn 继续与 FIFO Resume | `TestNativeWorkerCapturedFilesAndSkillReopenExplicitFIFO`、`TestNativeWorkerQueuedFileCorruptionFailsBeforeHTTPAndRetainsUnsentDraft` |
| 伙伴设置期间换页、Goal 回执失真 | 迟到结果不换选择/导航；停用成员可移除；Goal 精确回执及原 RequestID 恢复 | companions edit tests、`TestGoalsCreationRejectsChangedReceiptWithoutReplayingCommittedFact`、`TestGoalsCancelledReceiptCheckPreservesFactForExplicitRetry` |
| Git 已完成时 Lead/Team 状态变化 | 原物理 owner 仍能结算；新操作准入另验，Waiting 只读证据不受阻 | `TestWorktreeProvisionOwnerCanSettleAfterLeadAdmissionOrTeamEnd`、`TestWorktreeWaitingLeadAllowsEvidenceButRefusesMutationAdmission` |
| 关闭设置再开其他表单 | 冷读/保存按会话、generation、预期 pane 采纳，实际收尾不丢 owner | tools/compatibility desktop 生命周期回归 |
| 按住旧 Stop 时执行或 Gateway 被替换 | 旧手势与 handler 固定原执行身份；新按钮仍可正常 Stop | `TestWorkspaceComposerPressedStopDoesNotFollowReplacement`、`TestNativeGatewayPressedStopCannotFollowPolledReplacement` |

连接 UI 未保存草稿/RequestID 属于本窗，数据库只持久保存连接及创建回执；不把 API 恢复能力描述成应用重启自动恢复表单。Worker typed FIFO 不改变普通聊天仅手动草稿的已确认策略。自动化仍为暂停优先 interval CRUD，没有新增 Run once。

## 本地可重复流程

所有自动化使用临时 SQLite、项目自己的子进程或本地 HTTP 服务，不启动用户的 Claude CLI，不使用真实模型账号。

| 流程 | 操作与预期结果 | 事实与测试入口 |
|---|---|---|
| 输入发送快捷键 | 编辑器 ⌘/Ctrl+Enter 与按钮共用准入，使用同批按键的最新正文；菜单开启时只消费快捷键，普通 Enter 仅选择。忙碌/运行/等待/录音/IME 时不发送，迟到接收回执不清除新草稿 | desktop `TestWorkspaceComposerSendShortcutSharesAdmissionAndKeepsDraft`、`TestWorkspaceSendShortcutCannotBypassBlockedComposerStates`、`TestWorkspacePickerCommandsEnterOnlyPreparesTypedDraft` |
| 控件加载与真实回执 | Text Loading 保留文字测量并居中 Loader，Leading Loading 保留原图标槽、名称与焦点，重复点击/Enter 零新增操作；General 保存与连接重试使用实际 gated 存储/本地 HTTP | desktop `TestWorkspaceControlLoadingRetainsWidthNameFocusAndAdmission`、`TestNativeGeneralPreferencesBuildIsColdAndCloseWaitsForActualSave`、`TestNativeConnectionCatalogRetryKeepsItsActionWhileRequestIsInFlight` |
| 减弱动效与设置保存状态 | Loader 仅重绘、不重建页面；系统减弱动效时静止并停止请求帧。保存权限/Plan 独立显示保存设置，收到真实存储回执前不更新已提交策略、不发送 | desktop `TestWorkspaceLoaderHonorsReducedMotionAndStopsRequestingFrames`、`TestWorkspaceSavedPermissionWaitsForActualStore`；不代表 31GB 内存事故已定位或解决 |
| 会话模型参数观察 | 从真实导航打开会话，重开/跨窗保存后菜单和发送预检使用已提交会话覆盖；空覆盖与缺省不同，旧poll不盖新Save，未读取/失败明确禁用并可重试，保留dirty草稿 | desktop `TestWorkspaceSessionModelOptionsNavigationObservesCommittedOverride`、`TestWorkspaceSessionModelOptionsLatePollCannotReplaceNewSave`、`TestWorkspaceSessionModelOptionsEmptyExternalOverrideCanSelectAndSend` |
| 群编辑取消 | 已保存mentions群改为all后取消，继续显示真实策略且可选择成员发送；保存失败留表单，未提交值不能参与执行 | desktop `TestWorkspaceCompanionGroupCancelKeepsCommittedExecutionPolicy` |
| 群范围切换 | 修改成员或工作区保存后，立即清除旧scope正文/轮次/cursor/审阅并冷读新scope；过期读及A→B→A导航不能回填旧事实 | desktop `TestWorkspaceCompanionGroupScopeSaveDropsPreviousFactsAndReadsNewScope`；不改变原群执行/等待owner |
| 未实现的密度选择 | 正式两档几何未区分前，原生控件禁用且保留既有值，不把仅保存选项当作布局切换 | desktop `TestNativeGeneralDensityCannotAdvertiseUnimplementedLayoutChange`；完成条件仍见GAP-04 |
| 多窗引用选择 | 两窗在同一会话各自准备文件；Send只带本窗捕获的DraftID/版本集合，成功只消费该集合。空集合不带任何附件，原件保留 | product `TestSubmitConsumesOnlyTheObserversCapturedReferences`、`TestSubmitEmptyReferenceSelectionDoesNotReadOrConsumeSharedDrafts`；desktop `TestWorkspaceObserversKeepSeparateCapturedAttachmentDrafts` |
| 引用生命周期 | 关窗、取消picker或迟到的成功准备结果不能占住无主暂存；真实在途上传/Send返回后才释放本窗未消费项，不删原件 | desktop `TestClosingObserver*`、`TestClosedObserver*`、`TestCanceledPicker*`；每会话共享8条/40MiB暂存上限，不承诺每窗独立额度 |
| 路由与历史 | 修改连接后旧目标不能用于新Turn/Worker/自动化/fallback；旧branch仍可查看。自动fallback要求精确账户/计费路由与明确批准 | product `TestProductRoutingRejectsStaleConnectionsButPreservesBranchHistory`、`TestProductFallbackRequiresExactRouteAndExplicitApproval`；HTTP `TestHTTPRoutingUsesCommittedRoutesAndExplicitFallbackApproval` |
| 原生审批归属 | 冷读后换owner或同run切fallback，旧审批不能派发到新引擎；同native字符串也不能借用Alva broker | product `TestNativeApprovalReadDispatchRejectsReplacementOwner`；hub `TestRuntimePermissionBindingRejectsSameRunFallback` |
| 问答与审批回执 | 双击只提交一次；独立读错不遮蔽另一待办或终态；旧poll/提交回执不能恢复已答请求或改掉重开的页面 | desktop `TestNativeApprovalDoubleClickSendsOneDecision`、`TestNativePendingSnapshotCannotRestoreSubmittedRequest`、`TestNativeApprovalLateReceiptCannotChangeReopenedView` |
| Worker真实停止 | 同Turn等待续轮也等待实际owner退出；Stop冻结下一次FIFO认领，迟到旧executionID不能取消替换owner或停掉队列 | product `TestWorkerStopJoinsIndependentSameTurnResumeOwner`、`TestWorkerExecutionStopFreezesNextClaimThroughActualSuccess`、`TestWorkerLateExecutionStopCannotCancelReplacementOrHaltFIFO` |
| 目标复用 | 新建目标后关闭重开；在原项目或当前会话范围选择已有目标；只有明确 Send 才运行，仍引用同一目标原件 | `sqlite.TestGoalsPagesReachOlderThanListLimitAndPinScopeAcrossReopen`；desktop `TestWorkspaceSavedGoals*`、`TestWorkspaceGoalFirstSend*` |
| 首页范围变更 | 准备项目 A 的目标后切到 B；Send 拒绝，保留正文与引用，允许移除或重选；不创建空会话 | desktop `TestWorkspaceGoalChangedHomeProjectRejectsBeforeSessionCreateAndAllowsReselection` |
| 首页计划准入 | 准备 `/plan` 后选外部引擎；Send 在 Create 前拒绝，目录显示真实可用性 | desktop `TestWorkspacePreparedHomePlanOnExternalFakeRouteCreatesNoSession` |
| 已删除伙伴历史 | 私聊／群已有消息后删除身份，再重开；仍可从已删除历史入口读消息与成员原始正文；身份保持删除，不能发送、继续回合或借普通会话入口执行 | product `TestCompanionDeletedHistoryReopensWithoutRestoringExecution`；desktop `TestWorkspaceDeletedCompanionHistoryReopensAndLoadsEveryPage`；SQLite `TestCompanionTombstonesRejectAllAttemptClaimsIncludingWaitResume` |
| 较早群历史 | 超过 256 条消息／32 轮后加载更早内容；真实游标页到达最早事实，无重复、无遗漏、无模型执行 | SQLite `TestCompanionHistoryKeysetsCoverAllMessagesAndRounds`；无可选分页端口时沿同一组件的有界观察保留 Stop：`TestWorkspaceOptionalHistoryPreservesGroupOwnerStop` |
| 动态连接推理测试 | 保存连接并手填模型，开发者页选择该 revision 的目标；显式 Run 后本地 HTTP 收到真实生成／工具参数；重开仍可发现 | bootstrap `TestDynamicConnectionsDeveloperInferenceReopensAndFreezesExactRevision`；连接目录 GET 仅验目录，不算推理 |
| 运行中编辑连接 | job 已冻结后修改或禁用连接；已有 job 保持原配置，旧预览不能发起新 job；读取配置阻塞时 Stop 仍可点击，等待真实退出才收尾 | `TestDeveloperDynamicFreezeFailsBeforeAnySelectedRequest`；`TestNativeDeveloperStopRemainsReachableDuringBlockedDynamicResolution` |
| 外部 native 结果 | 子进程零退出但 native 失败、身份冲突或结果后继续输出；均不能伪报成功 | process `TestClaudeNativeResultTruthSurvivesSuccessfulProcessExit`，只运行本地 fixture |

模型选择器39另按Foundation `cmp_alva_modelrow`定向核对：整行hover/selected包含右侧收藏与参数，两项独立点击不选模型、不Send，整行空白仍可选择精确路由；Escape保留稿并回焦点，长目录沿虚拟列表键盘访问。`TestWorkspaceModelRowWholeSurfaceAndIndependentActions`与`TestWorkspaceModelRowKeepsFavoriteSlotWithoutPreferenceStore`补齐原回归未覆盖的范围，明暗/长名称画面见[原生ModelRow审查](./design/alva/exports/archive/model-picker-2026-10-26/report.md)。这不代表全部选择控件或业务模块完成。

从仓库根运行相关产品和进程回归：

```sh
go test -mod=readonly -tags desktop -race -p 1 -timeout 6m ./apps/hub/... ./apps/developer/... ./integration/engine/process
```

发布前仍执行根 AGENTS 规定的全仓 race、vet 与 `go mod verify`。新增公开端口必须同时审阅 `tests/architecture/testdata/public-surface`；分页、只读归档等能力留在可选端口，不扩大执行存储的六方法契约。

## 真实账号与窗口

Codex 的真实验收必须从已批准的显式程序／配置入口运行：新发、实际工具结果、重开同一 native thread、运行中 Stop 并等待子进程退出、停止后再发。记录每个产品 Turn／Attempt 的终态和实际 native 身份；仅退出码或模型文字自述不足以证明执行。

2026-10-26本机0.164已完成回复、同thread恢复与实际中断/join；环境按用户指定的Cindy方式继承本机HOME/工具链、单独设置CODEX_HOME后，再次恢复原thread，以一对真实commandExecution start/end读取新随机测试文件，结果含精确标记且Turn completed，耗时14.286秒。未写mise信任、未绕过hook、未读取/复制认证文件，Claude未调用。此结果不代替真实人类审批/多问题/native子线程交互验收。

Claude 按用户要求不启动真实程序、不查询账号状态、不调用模型。其逻辑只用 fixture 验证，不能写成真实平台通过。

真实 MyGo 窗口另验菜单触发与位置、键盘／IME、滚动和动作可达。无窗口 Tester 只证明控件与布局契约；`frame_pending` 不算成功截图。

## 尚未闭环

以下是已知未闭环事项，尚不是覆盖整个软件的需求总表，因此不能据此计算产品完成率。新增测试只关闭相应回归，不能把整域改标完成。每项须有用户动作、实际事实及失败/取消/重开证据才能关闭；跨平台或真实账户的通过不能由fixture推定。

| 编号 | 未闭环能力 | 当前边界与关闭条件 |
|---|---|---|
| GAP-01 | 外部引擎双向审批／问答 | Codex分型callback、四项owner binding、防迟到、秘密保护及取消/join已有fixture；2026-11-05本机Codex/Pi读取、真实活动后Stop及冷恢复通过。Pi真实CLI+本地HTTP的审批/单自由问答/80KiB安全正文通过；真实模型主动发起的人类审批/多问题及native子线程callback仍分别待验收，process Inspector尚未实现 |
| GAP-02 | 本机Agent来源 | 用户确认复用明确选择的本机Codex/Pi CLI配置/登录状态，不自建账户登录/退出或复制秘值；明确来源、实际调用、来源变化拒绝及同native冷恢复已有回归/账户证据。无参数bundle只消费安装时固定profile，普通CLI不混装；Claude仅fixture |
| GAP-03 | 首页文件与Skills | 首页任意本机文件、受管项目Skill、原子首发/采纳grant与耐久草稿已实施；capture/解析/模型实际可用分别报告。跨窗/源变动/重开已有回归；冻结Alva/localHTTP/MyGo无窗口路线一小时已通过，排除后来Codex/ModelRow改动，真实OS选择器/IME另验；compatibility不隐式枚举 |
| GAP-04 | 信息密度 | 用户确认本轮固定并隐藏选择，不将只保存两档说成布局已切换；保留已有值与组件供后续真实契约。当前不把此缺口扩成无效交互 |
| GAP-05 | 语言资源 | 用户确认本轮固定并隐藏入口；中英文资源统一及切换/回退作为后续模块，不提供失效选择 |
| GAP-06 | 应用级减弱动效 | 已接同一Preferences CAS/Save/Reload、跨窗快照及框架/Loader消费者；系统要求OR应用设置，静态忙碌保留。真实paint像素/无持续请求帧已有回归，持续验收见本轮MIGRATION |
| GAP-07 | 资源拖拽／改宽 | 本轮代码、CAS恢复、真实指针/键盘与明暗布局回归已补齐；实际系统窗口连续拖动/缩放另验，不以无窗口结果代替平台验收 |
| GAP-08 | 系统语音的设备与平台 | macOS代码接线和fixture不证明许可拒绝/设备声学质量通过；其他平台明确不支持，须分别记录实机结果 |
| GAP-09 | 运行中补充 | 用户确认当前仅保留可编辑、可持久化的手动草稿；不自动排队/插话/Send，不把Worker FIFO当普通聊天。其他策略为后续需求，不再标为本轮待决定 |
| GAP-10 | 可信Context上限 | 缺绑定实际模型的可信声明，控件保持未声明；不能拿样例值限制或承诺上下文容量 |
| GAP-11 | HTTP入口覆盖 | 缺连接CRUD/模型参数、目标/typed项目引用、伙伴/群、资源视图入口。共享服务或桌面通过不代表HTTP已曝光；是否进入当前桌面交付范围须另列 |
| GAP-12 | 真实窗口与可访问性 | 2026-11-05按稿复核已实机通过8类设置返回、左栏开合、旧会话、原生目录选择、Codex一次真实只读Send至completed、同名文件区分与根README阅读/Find/右栏恢复。仍记录早一轮AX press后退出未定因，以及一次ScreenCaptureKit错误但进程存活；最后指针文档流程无捕获错误。IME、读屏、连续缩放及其他页面状态继续分别验收，不由本子集代替 |
| GAP-13 | Auto安全命令分类 | 当前只自动批准已知原生工作区文件写入；命令/网络/MCP/未知效果仍问。须有明确、可验证的安全分类后才能扩大 |

自动化当前范围是暂停优先的interval CRUD；Run once未进入现有契约，不把它列作已实现动作，也不为了补表自行增加执行策略。
