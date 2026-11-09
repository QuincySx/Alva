# Alva 实施与验收

Go 是当前唯一受维护实现；Rust 与旧前端已按用户授权存入 Git 标签 archive/rust-final-2026-10-18。
技术栈决策见 [ADR 0012](./docs/adr/0012-go-native-stack.md)，当前结构见 [ARCHITECTURE](./docs/ARCHITECTURE.md)，运行说明见 [USAGE](./docs/USAGE.md)。
目录与源码验收不等于全部功能对等、全部平台或外部账户验收。

## 2026-11-09 第一波业务规则修复（路由、权限、等待）

提交范围 `0c91ccd9..69d25244`（`feat/m1-message-stream`）。

- **路由与分支**：路由身份是引擎、连接和模型三者。改连接名、换 Key、改请求头、开关连接都不再让绑定它的会话、fallback 和自动化失效；每次执行取连接的最新 revision，只在连接或模型已删除、已禁用或协议变化时拒绝。端点变化按路由审计并提示一次。分支按引擎划分，同引擎内换模型沿用原生会话与上下文。Fallback 只在当前一轮生效，下一轮重新尝试主路线；过期候选带可见提示跳过，被 fallback 救回的轮次有灰色提示。
- **权限**：`abi.Effect` 新增 `Billable`，工具自述“会创建或切换计费身份、或新开付费 Worker”；`hub_team` 的修改类动作声明它，任何档位（含 Full）都要询问；子 Agent、MCP、网页仍按当前账号的普通使用处理。Auto 档按效果（带资源声明的写入，仍受 Guard 边界约束）放行，不再看两个工具名；执行与联网仍询问。开关计划模式不再重置权限档。
- **等待与收尾**：新会话默认开启持久等待；新一轮清掉上一轮在原生日志里留下的 Running/Interrupted 标记，失败或取消的轮次不再卡住下一轮。停止落在存储写入中时记为已取消，而不是失败（`65a56946`）。`alva-hub` CLI 创建的会话关闭持久等待，否则 CLI 拿不到最终回答（`69d25244`）。
- 证据：各修复各带对抗测试，见对应提交；`tests/scenarios` 的三档权限、计划模式、等待与跨引擎交接场景同步更新。未在本条重新宣称全量验证数字，以合并时的全量验证为准。

## 2026-11-08 M2 第 1 步、M4 摘要压缩、M5 设置与远程 MCP

- **M2 第 1 步**（`5b70e15a`、`b06bacc5`）：`ConversationService` 与 `TurnService` 成为会话列表/读取/管理与轮次启停的窄产品入口；desktop、HTTP server 与 `alva-hub` CLI 不再直接用 Store、Runtime 或 Repository，准入、执行身份和分支归属只在一处决定；server 的停止必须带执行身份，新增读取当前执行的端点。`tests/architecture/product_interface_test.go` 守护入口，其允许清单只减不增（当时留有 server 十条设置路由与 desktop 两处读取待迁到 `SettingsService`）。引擎侧声明并实现插话与分叉能力。
- **M4 摘要压缩**（`233479f7`）：Alva 引擎的上下文压缩插件取代截断：请求将超过模型窗口的 80% 时，用同一模型总结较早轮次、保留近期轮次原文，摘要作为原生扩展记录并链到前一份，重放与恢复不重新生成；窗口取自供应商目录，其次连接的模型设置，否则 128K。摘要失败或损坏时回到截断并显示状态；能识别的供应商溢出触发一次压缩后重试。子会话暂仍用截断。输入区新增上下文用量与手动压缩。长会话门 `ALVA_LONG_SESSION_GIB` 覆盖压缩后的有界读取。内核收窄第二批仍未做，待用户批准。
- **M5 设置**（`d56f3938`、`02ea4bb1`、`5af10193`）：设置以应用级为主、新会话沿用、会话可覆盖；`HubService.EffectiveSettings` 是唯一解析点；会执行命令或联网的项只有在“精确工作区 + 配置指纹”的批准下才运行，改配置需重新批准而改名不用；schema v9 增加应用设置与工作区批准表；`product.SettingsService` 是 desktop 唯一设置入口（秘值只回占位、按分区指纹比较保存、字段错误为中文），设置页全部是中文表单，不再手写 JSON。MCP 增加 Streamable HTTP（JSON 与有界 SSE、会话与协议版本头、会话丢失不重放、不跟随重定向、不走环境代理，旧 HTTP+SSE 明确拒绝）；`mcp/oauth` 做发现、动态注册、PKCE（仅 S256）、state、resource 与 issuer 校验、刷新与撤销，回调为单次使用的本机回环；令牌存 macOS 钥匙串，其他平台拒绝而不退回明文。模型连接的 API Key 可选。余下：server 设置路由与 app_desktop 读取的允许清单清零。
- **端到端场景**（`4ed7dd3b`）：`tests/scenarios` 用真实 SQLite、工作区和 Alva 引擎装配 Hub，只把模型换成回放 OpenAI 兼容与 Anthropic 流的本地服务，30 个场景覆盖连接、对话与项目工作区及越界、重启恢复、并发会话、流式、停止、供应商失败与崩溃恢复、文件与 shell 工具、三档权限与计划模式、ask_human 跨重启等待、子 Agent，以及 Alva、Codex、Claude、Pi 之间的会话交接。

## 2026-11-07 至 2026-11-08 M1 对话呈现

提交范围 `d2cacc40..ca6261d0`，另含 `1e980a99`（任意主机允许 http 模型端点，用户 2026-11-07 决定）与 `9c9d26b7`（未知工具调用不经审批直接答复）。

- **数据与滚动**：内核记录每个工具结果的真实开始与结束时间；流式增量约 250 ms 合并写一行，最终消息提交后在同一事务删除已合并增量（截断且无 Reference 时保留）；消息流由产品层从已提交历史投影，按消息分页并在滚动到顶部时自动加载；shell 恢复与 AEP 状态读取改为有界读取；`tests/architecture/reads_test.go` 禁止新增整段会话的全量读取。
- **轨迹与工具块**：对话与轨迹两个视图；轨迹含输入、模型、工具三条泳道、步骤列表、概述/预览/原始详情与搜索，`Inspector` 支持向前翻页；各引擎 adapter 把工具事件统一为带脱敏展示信息的 `engine.Event`，界面按类型呈现命令、读文件、改文件、搜索、子 Agent 卡片与待办清单，并跳到对应步骤、右栏文件或子会话。外部引擎的工具块映射六类，而不是 ADR 0013 §1 原文的两类。
- **其余块**：审批与问答卡进入对话（处理后折叠为一行）；回复底部一行淡色 token 用量与缓存命中；回复中的 `chart` 代码块由内置渲染器画成图表（bar/line/area/pie/scatter），格式说明只注入 Alva 与 Claude。
- **验收证据**：长会话门（`ALVA_LONG_SESSION_GIB=2`、`ALVA_UI_BENCH=1`）在 2 GiB 会话上通过：对话滚动单帧 CPU p95 约 1.3 ms（门槛 8 ms），2 GiB 会话打开约 1.5 ms（门槛 100 ms）；这两个数字取自主会话给各工作包的对照基线，本条未重新测量。真实窗口下的帧率与长消息流式重排不在这两个数字内，未单独验收。
- **未包含**：按设计稿精修外观、Tab、归档、两级导航与设置页外观仍属 M6；数据块暂无表格。

## 2026-11-07 工作台正本、居中搜索与开发工作区边界

SmallPen 正本已保存为 Product `2854e314…`，Foundation `f25a54ab…` 保持不变。21 恢复新任务首页，22 保留开发者主版本并增加普通聊天窄窗；72 个 Screen、73 个 Presentation、601 条连线、3 个起点。其余 70 个 Screen 与设置页保留。前台三张预览来自同次保存后的原生导出；详细修订、几何与部分验证覆盖见[设计状态](./docs/design/alva/DESIGN.md)和[预览入口](./docs/design/alva/exports/README.md)。

原生左栏搜索和 Cmd+K 打开窗口正中的共享 Dialog；当前只按真实已存会话的标题或工作目录查询，每页 32 条并使用真实游标。输入未提交时不复用旧结果；替换、关闭及重开通过独立取消与 epoch 隔离回执，选中有效结果才交由既有 owner 导航。打开、搜索、分页与关闭均不启动模型；设置、命令及消息全文搜索尚未接入。

Enter 已改用 MyGo 官方 `OnSubmit` 一次投递，保留同文再次显式提交。新增真实控件与共享 Store 的阻塞读取回归，检查一次 Enter 仅一次读取、持有期间不自我取消、释放后结算，以及同文第二次 Enter 刷新。该测试在旧实现上也通过，不能据此判定原生加载停留的根因；无窗口通过与原生验收分别记录。

当时的普通发布源码 `go test -mod=readonly -tags desktop -race -p 1 -timeout 6m ./...` 实际退出 0，3290 项通过、107 个包；汇总未报告跳过数，不推定为零。全树 vet、模块完整性校验均退出 0；另有 641 项桌面 race、243 项架构及 6 项搜索专项通过。这批汇总属于其后诊断插桩之前的状态，其后源码已变化，不冒充插桩后的全仓再次通过。原混合暂存与工作树保留，本轮未提交。

普通发布（本轮标记 805/K2）已原位安装到唯一 `/Users/smallraw/Applications/Alva.app`，整包严格签名与 deep 校验通过；安装与候选 Go build ID 相同，Info.plist、启动 profile 与数据保持。候选及自有旧二进制备份为 0600，未创建第二个 App。旧进程已正常退出并重开，首次重开后曾实际观察到新窗口；其后已完成诊断版的临时替换并原位恢复，当前普通版进程运行，可见窗口验收另列。

旧安装版本的原生基线（以下为差分 AX 的临时自动化观测，不是独立的产品延迟结论）：目标查询六次中四次观察到加载停留（约 21、13.4、15、至少 115 秒），一次 2.8 秒结算、一次因工具重置计时未知；README 三次均约 2.7–3.4 秒。Raise/End 未证明能唤醒结果，Down 与结算仅为时间巧合；坐标与截图工具亦有独立失败。期间未 Send、录音或新发模型请求。

诊断侧源码只加入稀疏的纯数值与布尔阶段标记，共四个文件；常规构建下 logger 为 nil 即完全惰性，desktop 与 e2e 兜底路径仅在既有 `MYGO_AUTOMATION` 下生效，独立审阅已通过。插桩后的实际源码检查回执为 6 项搜索专项 race、243 项架构、相关 vet、e2e 编译与 e2e 构建均退出 0。上一段的 3290 项/107 包以及全树 vet、模块校验属于插桩之前的状态，普通发布仍是同一个 K2；该 K2 的界面验收也保留在插桩之前：用界面上正确的搜索输入框重跑一次联调，读取采用 `disableDiffing: true`，823–8172ms 明确停在 “Saved session search loading”，只查询一次即 Esc 回到空首页，无 Send 或模型请求；这与其后临时诊断采样是两次不同观测，不与更早的差分 AX 计时合并解读。

`go build -mod=readonly -tags desktop,e2e` 构建出诊断候选 `cebef135…`：主包正确、Mach-O arm64、automation 已链接，候选 0600。在同一个规范 App 内先以普通 AppleEvent 退出（返回 0，旧 PID 70317，进程计数 0），再换入诊断二进制并整包 ad-hoc 签名，严格、deep 与内部校验均为 0，配置哈希与资源数保持；已安装诊断的签名 SHA `721bfe55…`、Go build ID `f7zIVccS…`，新 PID 18412、进程计数 1，自有 socket 0600 且父目录私有 0700。官方原生 API 只自报恰好 1 个窗口、visible true、focused false，这是应用自报，不等于 OS 前台或像素。原生探针的安全基线为 Message 为空、Send 禁用；观察到搜索按钮唯一且可用、按下被接受；轮询 3.013 秒后未识别到可见搜索 Dialog 或输入框，于是在任何 focus、setValue、Enter 之前以 `dialog_not_found` 中止，stdout 与 stderr 均 0 字节、固定前缀阶段计数 0。因此本轮没有产生目标查询，也没有 Store 计时，没有查询数据库变慢的证据，未证明任何诊断结论；未做第二次目标查询，未 Send、录音或调用模型。该临时探针的结果与空态选择器在未执行到的分支里存在已复核的错误，未验证、不可复用，不作为 runbook 链接。

恢复已实际完成：对诊断实例 18412 以普通 AppleEvent 退出返回 0、进程计数 0，自有 socket 随退出释放，无需手工删除；同一 App 换回普通候选 `833a163c…` 并整包 ad-hoc 签名，严格、deep 与内部校验均为 0，测得已安装 SHA `805adcf8…` 与替换前普通二进制逐字节相同，Go build ID 回到 `K2-ZVlE2…`，Info.plist、启动 profile 哈希与资源数 1 保持。随后 `open -a` 不带环境变量与参数返回 0，观察到唯一一个规范进程 24998；候选与自有备份均 0600，未删除用户数据，未创建第二个 App，本轮无 Git 写入。恢复后新的外部 CUA 按规范路径取 App 仍失败于 -10005 `cgWindowNotFound`，只完成一次部件汇总，未做任何 AX、按键、动作或 Return；这不能区分锁屏/环境与产品根因，屏幕状态的澄清问题仍待用户回复。当前普通 App 在运行，下一步需要可用可见窗口再做真实的 guarded Return 探测；本条不宣称原生查询、可见性或修复已验收，也不表示整体产品验收完成。

开发者首发已复用产品服务校验现有规范目录、活跃 Git membership 与 CommonDir，失败保留草稿并拒绝创建会话；原子准入仍属于现有存储事务。SQLite 跨 scope 身份检查仅在新增 tab/draft 身份时扫描，选择、归档及重排复用原身份约束。接口与职责见[产品领域定义](./apps/hub/developer_workspaces.go)和[服务实现](./apps/hub/product/developer_workspaces.go)。

Worktree 会话 Tab 的桌面接入仍未实现：目标保持左侧“仓库 → Worktree”两级，会话位于既有 36px 标题行；关闭归档与 Stop 分离，只有一个活跃观察者。下一步需补普通导航排除与全局搜索的独立查询、Session 到 Tab 的共享读取入口、每个未发草稿的 target/plan/permission 恢复；现有 JSON 可查询，不预设新表。当前剩余验收是新版可见窗口的真实 Return 查询与上述桌面业务接入。

## 2026-11-06 一套正常信息密度

按用户两张同尺寸截图和后续明确纠正，同时缩小字体、按钮与图标；先做好一套正常密度，不增加大/中/小模式。共享UI值集中于`workspace_metrics_desktop.go`：body13/caption11/subtitle14/heading20，普通控件32、图标动作28、图标16/小图标14、导航行28；原生36工具行、左右动作运动方向、三栏16圆角及12外距保留。文本源、输入/菜单/模型/设置/附件与编辑区消费同一角色；已保存阅读字号及系统TextScale继续生效，固定UI标签不随正文设置放大，不修改用户偏好。

任务选择去掉重复大标题/说明，返回位于主头，搜索与列表限宽760；既有范围选择、真实Detail准入、明确Open、分页/取消和原服务/SQLite owner保持。附件56自然双行、引用badge随文字hug、模型字段撤去旧固定65行框，编辑区14/1.6及28操作仍保持原对称内距和浮层锚点。无模型调用、无新业务/数据库路径。

SmallPen公开CLI原位修改既有Token和8组组件、同步原生导航要求，正式Foundation `f25a54ab61abbbff71b7095d84f24599ef46a205709f0468bd857713744ee2bd`、Product `e174b150043139f25591271efbc01006f1709eea007c2fb75faea2b6bb267c1f`；151 Tokens/204值、533变体、72页/599连线不增。旧主题仍同值，不暴露新的密度开关；source默认值同步但未重跑旧生成器覆盖保存包。Sidebar/Input/Composer共46原生目标无报告问题，对比度跳过保留，非全库像素验收。

同一40条内存fixture和原生52/Left86事实，1240×850完整可见任务行从11增至16，1547×932从14增至19；旧源用Go overlay，不安装旧客户端、不写用户数据。新旧PNG分别保存，独立查看两张当前实际MyGo整页。最终全仓desktop-tagged race 98个有测试包通过、0失败，全树vet与生产构建通过。初轮一条旧偏好测试期待界面标签随阅读字号变大，修为验证固定UI13标签及两窗阅读theme16/system24，SQLite真实Save/reopen、零会话/执行约束保留，完整重跑通过。

唯一`/Users/smallraw/Applications/Alva.app`已正常退出、原位更新及重开，20个Mach-O section/Go build ID与严格签名通过，原Info.plist/全部Resources/固定profile不变，候选/备份0600；119份桌面生产源/资产在构建后无漂移。实际Mac已看到新首页与打开的空右栏；后续指针动作被CUA用户状态变化守卫拒绝，刷新树后保持窗口，不把fixture范围选择冒充实机操作通过。系统/其他平台、全产品和长期内存验收另记；原混合索引未操作，未整体提交。来源、对照与检查见[实施证据](./docs/design/alva/exports/archive/density-2026-11-05/implementation-evidence.json)。

## 2026-11-05 开关位置与运动方向修正

用户明确纠正上一版的运动关系：左开关展开时应在左栏右上，收起时移到红绿灯右侧；Main外框同时向左扩展，标题排在开关后面。右开关应始终固定在窗口右上，只有资料栏、标签、Add/More随开合出现/退出。此前“两态左开关固定同锚点”的解释已被本节和原生requirements取代，不从旧证据恢复。

实际实现把左右两个控件统一交给不参与面板动画的窗口壳，保持同一控件身份；仅左控件位置参与160ms过渡，右控件每帧固定。主Header/Tabbar只预留32动作槽，不复制另一个按钮。标题用稳定key及局部位置过渡，与Main同向移动，避免父框移动时内距突变导致标题先向右跳。侧栏向左退出，资料栏从右侧有界展开/退出，保留三栏16圆角及12间隔。减弱动效、直接跟手调宽、原观察/保存/草稿/执行owner均保留。

正式Foundation `1268268b4445af4409543fe20164cdd255160cb9e4e2031a094446f0dd91e94c`、Product `d8fb60ba7192050c123b3fc6c095e37a4eff5af280a214dca086d958507eca1b`经公开guarded批次保存：Sidebar恢复NativeSafeArea→Grow→Toggle，navigation/resources原生要求同步窗口壳owner和运动含义。无新Token、组件、变体或页面。旧候选PNG和上轮安装/测试记录保留旧修订；最新证据见[开关修正](./docs/design/alva/exports/archive/pane-controls-2026-11-05/direction-evidence.json)。

修正版本已原位安装到唯一`/Users/smallraw/Applications/Alva.app`并正常打开，20个Mach-O section/Go build ID和严格签名校验通过，Info.plist/全部Resources及固定profile不变，候选和备份不可执行。实际Mac窗口已走左折叠/展开、右展开/收起；左开关两端位置、Main/标题终点、右侧固定入口和附属动作的显隐均观察到，最终左栏展开、右栏关闭，空草稿且未发送模型请求。静态窗口截图不充当动画逐帧计时；方向/起始帧无跳变/右按钮各采样帧固定/面板不越界/快速反向/结算停止/减弱动效由真实Build与原生时钟测试分别覆盖。最终全仓desktop-tagged race 98个有测试包通过、0失败，全树vet通过；初次仅资源拖放提示fixture在新入场动画尚未结算时取坐标，改为与其他几何测试一致的结算帧adapter后，专项race与完整重跑均通过，真实动效测试仍启用。精确日志/初次失败保留于最新证据，不借用上轮结果；系统红绿灯仍被共享提示覆盖，系统按钮点击和其他平台实机另验。

## 2026-11-05 三栏圆角、独立开合与原生过渡

用户改为“直接改”运行端，随后明确要求左中右统一圆框和左上跟随动效。三栏复用现有16圆角：左右subtle、中间surface，外距/区域间隔仍12。开合使用MyGo原生160ms位置/尺寸过渡，左开关匹配身份支持反向；减弱动效遵循已提交应用偏好与系统要求，拖动直接跟手。退出快照绘制仅捕获构建时的值，避免动画跨帧读取过期Element；原生时钟测试验证中间帧、反向、减弱动效与最终停止请求帧，不将这次局部修复宣称为旧31GB事故定因或长期内存验收。

左开关两态保留同一原生安全区锚点；macOS保留HiddenInset和系统按钮，显式TrafficLightPosition=(16,19)，实时TitleBar事实决定布局。右栏关闭时主页头仅一个展开图标；打开时成为Main同级面板，主外框实际收窄，36工具行承接Tabbar/Add/More/Hide，弹出/收回仍走原owner。窄窗资源专注保留主页头唯一同位收起。首页打开空右栏，明确“选择任务”才导航；首次打开不自动加项目文件页或扫描。原有标签、滚动、宽度/开合CAS、草稿和执行归属保留，进入其他业务页只临时退订/隐藏观察，不Stop或清空持久视图。

正式SmallPen经公开guarded typed批次保存为Foundation `70af776ed99b3c850101a87df92a8e28516a7f86a99168ceec3bb65aca4f2a59`、Product `9bd7faa906637ef268536289fa3993d2985b471f7e42d5b8de9983b62f1b0f26`；共享组件及22代表布局、原生navigation/resources要求同步。151 Tokens、533变体、72页/599连线不增；较早候选PNG保留真实旧revision。最终Sidebar四变体及22页面共5目标无报告问题，透明背景对比度跳过仍列明，不将自动检查等同视觉通过。入口见[DESIGN](./docs/design/alva/DESIGN.md)与[实施证据](./docs/design/alva/exports/archive/pane-controls-2026-11-05/implementation-evidence.json)。

最终完整desktop-tagged全仓race **98个有测试包通过、0失败**；desktop整包race（203.141s）、全树vet、模块完整性和生产构建通过。本轮使用非JSON包级输出，不借用历史个别测试数量。初次全仓仅旧首页测试仍期待隐式任务选择，改为先断言空右栏、再明确选择后，专项和完整重跑通过；失败原日志保留。既有几何/业务测试通过共享adapter核对结算帧，独立动效测试直接运行真实Build/原生时钟，未以关闭生产动画换取通过。

唯一`/Users/smallraw/Applications/Alva.app`已正常退出后原位更新并严格签名；Info.plist、全部Resources、固定profile及同一数据目录保留，20个Mach-O实际section与Go build ID匹配。最终当前源码重建与安装前候选逐字节一致；候选/备份均0600，不另开客户端。实际Mac窗口已验证首页空右栏、Main收窄、三块圆角面板，以及左栏折叠/展开、右栏收起/重开各自独立；最终停留三栏展开、空草稿。未发送模型请求。系统共享胶囊遮挡红绿灯本体，虽AX存在三个系统动作，仍不宣称其像素中心已验收；系统按钮点击、整窗拖动、其他OS和完整产品业务仍分别验收。原有混合暂存未操作，本轮未整体提交。

## 2026-11-05 原生内嵌标题栏与统一顶部行

按用户指定的 [Hergo 固定源码](https://github.com/Maidang1/hergo/tree/f4ba09d397d45e163c9897c027e1c7dacce4b538)核对原生 inset 标题栏、侧栏开关与收起布局，先通过 SmallPen 公开 typed 操作更新正式稿，再实施 Go；保留 MyGo v0.7.4、Alva 风格及原业务 owner。设计保存为 Foundation `25e27c5ec18d7f7f9ff6f42cf002d4bc38869f3a6ebb30eabb8891b6ecd56cc3`、Product `2dbf464cac3cfc7a9a61435ffc2690760a5227cd802be39e1e4d3f53e96605f7`，新增原生 requirement `req_alva_navigation_native_shell`。详见[逐图来源与保留记录](./docs/design/alva/exports/archive/native-shell-2026-11-05/report.md)：17个实际 validate targets 无报告问题，72页/599连线保留，151 tokens 不增；窄窗隔离 candidate 与正式页修订分别标明。

macOS 使用系统 `TitleBarHiddenInset`，保留原生系统按钮，撤去自画按钮和独立32px条。侧栏 topRow、主 Header、设置及连接返回共用36px内容行/32px动作，标题16与来源12横排；位置消费实时 `Context.TitleBar()` 的 Height/Left/Right，避让左右系统按钮。52px高、Left86的样例 top8、中心26，收起后展开按钮x94；无native事实时top12，右侧系统按钮延伸到52时正文继续避让。左栏展开仍220–432，收起整棵隐藏且无72px rail，已有宽度/CAS/草稿/观察语义保留。资源页头仍52，conversationMin432及窄窗资源专注保留，不为静态压缩图放宽运行约束。

设置及新建连接将实际返回移到主 Header，取消重复卡片返回；保存/加载守卫及真实审批/问答不变。独立审查发现960×680来源选择卡片与真实pending共存时后部供应商不可达，现仅为原卡片增加有界外层滚动：正常720×469居中和所有选择 handler 保持，真实pending下五种供应商均可滚达、选择及返回，未隐式回答、批准或保存。原生左右safearea、设置/表单/资源几何、SQLite布局重开及 owner 回归通过；独立代码复审无剩余P1/P2。

唯一 `/Users/smallraw/Applications/Alva.app`已原位替换并通过严格签名，普通无参数 `open -a` 启动PID35387；Info.plist、全部Resources、固定profile及同一数据目录保留。候选与安装二进制的22个实际代码/数据section和Go build ID一致；最终全仓检查重新构建后候选SHA仍与安装回执一致，冻结1162份Go/模块/桌面资产无漂移，仅此一个canonical进程，备份/候选不可执行。

真实窗口已走原completed Codex会话与根README双栏、侧栏收起/展开、测试草稿跨布局与设置返回保留，以及设置→来源→空OpenAI表单→顶部返回原工作台；未填表、连接测试、保存或新发模型请求。补验按截图点More→新鲜AX菜单项“设置”→截图点顶部返回成功，初始/返回后Panel与More均可见。最终恢复左栏展开、同一历史/README、空草稿、无新running。AX确认系统close/full screen/minimize；截图左上被紫色系统胶囊覆盖，不能声称红黄绿本体已像素验收。系统按钮点击、整窗拖动和其他OS仍未实测。中间两张截图曾缺Panel/More，后续恢复且补验未复现，原因继续核查，不据此宣布绘制问题已修复；原生菜单未进入主窗口截图是另一捕获边界，不能直接等同两次按钮缺图。

针对缺图又做唯一2×无窗口窄复现：实际page.Build/README与Store种下的completed展示，经输入草稿→收起→展开→设置→返回的立即/结算帧，7张2480×1700逐图查看，两按钮32px几何及20dip图标裁片SHA全程一致，资源区在y44下未遮Header；专项race通过。公开Tester无AX SetValue，使用真实Click/Command/Type编辑路径；软件渲染未覆盖原生GPU/系统捕获，不将未复现改写成缺图原因已解决。原生窗口后续More往返通过与这个fixture结果分别记录。

最终完整 desktop-tagged 全仓 race **3214项含子测试通过、6项跳过、98个有测试包通过、0失败**；全树vet、相同manifest的模块完整性及生产构建通过。首轮失败为旧General几何断言，改为明确原生事实下的正式新坐标，保留严格相等检查；旧失败日志仍保留。检查点 `/Users/smallraw/.codex/checkpoints/alva-agent/20261105-integrated-window-shell`记录正式依据、初次失败、最终检查、安装/运行哈希和窗口验收边界；原混合暂存内容未操作，本轮未提交。不据此宣称全部产品、平台、真实账号或长期内存验收完成。

## 2026-11-05 工作台按稿还原复核

用户指出运行版缺少左右栏与完整工作台结构。此前首页、局部控件与首帧通过，不能证明正式21/22整页已经还原；本轮保留原Foundation/Product修订，修正实现与验收范围，不为迁就实现修改设计稿。

左栏补齐常驻自动化、已保存工作区及其独立分页子会话、最近记录和本地工作台页脚；页头提供Panel/More。设置恢复正式8类导航，应用级偏好与绑定已保存任务的配置分开。项目索引仅从普通会话已存路径读取，排除产品受管私有目录是位置过滤，不冒充目录选择来源事实；没有文件系统扫描、空项目或身份凭据发现。可选WorkspaceNavigationStore/AutomationBrowseStore由SQLite实现、产品服务校验，六方法ExecutionStore保持不变；浏览不Create/Send/Tick。

正式22会话采用右对齐用户消息、32px头像与12px回复间距，聊天列表独立top16与行距24，历史身份仅由同Attempt已存native事实匹配，不从当前模型选择器补写。右侧资源改为432px圆角阅读面、52px页头、32px路径行与24px文档内距；有界Markdown、当前载入范围查找和显式原始内容入口复用原读取服务，标签/宽度/位置仍走原CAS。关闭或退订不Stop。新任务复用正式Secondary/Leading共享变体，图标模式不插空文字节点；默认surface/border与现有hover/disabled/focus统一消费。

Markdown完整有界源只解析渲染一次，引用式标题链接、脚注及锚点共享文档上下文。当前MyGo公共API使标题继承正文行框，不能分别精确指定设计的1.5标题行高；保留正式字号/颜色/间隔并明确这处差异，不用标题切片或第二套链接渲染换取表面几何。工具异常收尾独立保留同Attempt/产品会话的原native owner，无结果引用只读原owner索引，未知归属不猜当前分支。

新增分页成功后一起采纳page/cursor/back，失败保留原请求以精确重试；候选选择校验当前成员与加载owner。真实Detail回执绑定session/branch/generation，普通轮询不撤销已验证范围，隐藏读取结算不抢导航。设置壳及连接表单复用同一审批/问答builder；表单外层按剩余高度滚动，窄窗问题与表单底部动作仍可达，无pending保持原553或viewport-48内容尺寸。真实permission/question与三种表单、两个尺寸的16项race回归及独立复审通过。

真实窗口检查发现同名文件key缺陷：多个README按basename建控件身份，实际只显示最后一项，headless会报duplicate key。现按File.ID/branch等真实来源建key，文件操作与Tab的可访问label/tooltip使用相对路径，visible basename和几何保留。真实整页分别读取三个同名README并选择/关闭其中一个的回归通过。共享扫描原来已是BFS且包含根README，没有改扫描器、排序、预算或绝对路径搜索语义；4096项partial范围回归单独通过。

新明暗/宽窄四张实际page.Build组合已独立查看，宽窗右栏984/64/432/872，用户消息y80、消息间距24；fixture内容与真实账号分别记录。实际Mac窗口已验证完整导航、8类设置及返回、左栏收起展开、原会话消息对齐；显式原生目录选择本repo、选择Codex后仅一次真实只读Send，经历commandExecution至completed并回复Alva。后续更新未重发，打开同一已完成历史，README相对查询显示六个独立路径，根README可读，Find在当前载入范围显示3处匹配，收起/重开右栏恢复同一文档且不执行。

当前唯一`/Users/smallraw/Applications/Alva.app`通过无参数`open -a`正常启动，PID7019，Info.plist、全部Resources、固定启动profile及同一数据目录保留；22个代码/数据section和Go build ID与候选一致，严格签名通过，备份/候选均不可执行。最终停在真实Codex会话+根README双栏、左栏展开、空草稿。真实自动化仍有未定因异常：早一轮AX press模型按钮后旧PID76809退出，源码未找到误派发Close证据；重启后指针选择成功。另一次ScreenCaptureKit -3812时PID86115仍存活，最后一轮四张关键窗口图与文档交互无捕获错误。这不等于所有AX/读屏、平台或页面状态均已验收。

最终完整desktop-tagged全仓race **3168项含子测试通过、6项跳过、98个有测试包通过、0失败**；全树vet、模块完整性与生产构建通过。冻结1103份Go/模块来源及58份桌面资产，检查/构建后SHA无漂移；最终候选SHA与已验收安装回执一致，当前PID7019的签名二进制SHA及build ID再次匹配，无须重启已打开的双栏。RSS单次观测160832KiB，仅记录本轮窗口，不冒充长期泄漏已解决。

检查点`/Users/smallraw/.codex/checkpoints/alva-agent/20261105-ui-parity`保留初次失败及收口、正式修订、冻结来源、组合图、唯一安装回执和两轮真实窗口记录。首次完整检查暴露10项desktop失败（旧入口测试、设置壳遗漏待处理交互及过时几何），第二次仅一条测试fixture尚未完成初始Settings读取导致20px提示变化；修为等真实读取回执后保留严格断言，未以sleep或放宽相等条件处理。最终完整命令通过，不用旧结果代替；原暂存内容未操作，本轮未提交。

## 2026-11-05 日用路径、左右布局与 MyGo 后台验收

当前官方 MyGo 最新发行版仍是v0.7.4，根依赖已固定该版本，本轮补齐 `.tool-versions` 的Go1.27.3。无窗口优先使用官方 `ui.NewTester` 运行实际页面、存储与输入；`desktop&&e2e` 才导入官方 automation，显式私有 Unix socket 提供真实运行窗口检查，普通发布不包含插件。用法见[USAGE](./docs/USAGE.md#构建和验证)。没有增加自制自动化协议或另开客户端。

安装启动新增固定 `Contents/Resources/alva-launch.json` 明确来源：无参数 bundle 启动读取，终端显式 flags 不合并；version/字段/大小/PATH/无 stdin secret 在存储前验证。三个入口均接 Pi 配置来源及 RPC 选择，不读取或复制认证文件。三包普通及race回归各32项、e2e vet和smoke编译通过；唯一安装入口已原位更新，实际窗口边界见下文。

左栏新增220–432有界调宽、72图标轨和展开/折叠；左右共用8px指针/键盘splitter，右侧保留432主阅读及320资源区约束，不足时显式资源专注并保留返回。布局意图经现有 PreferencesService 稀疏 CAS/防抖保存；width/collapsed分别记录手势版本，只提交实际修改的字段，冲突重读不能以旧宽度覆盖另一窗的新宽度。首次观察不覆盖本地手势，旧回执不消费新调整，缩小窗口不覆写保存宽度；关窗等待实际保存，取消观察不陷入阻止关闭循环，不 Stop 执行或重置 General 草稿。新增9项持久化和3种确定性双窗冲突回归，相关专项race、vet和分形检查通过。真实系统鼠标连续拖动仍单独验收。

Pi 采用独立原生 RPC owner、固定内容哈希扩展和nonce握手；真实私有session/file/cwd确认后才提交prompt。bash/edit/write在真正执行前经既有产品callback审批，自由问题走独立问答；旧响应、Stop和重开不能借下一次owner。显式exec保持只读，不在协议失败时回退。agent_end不是终态，只有实际settled和进程清理完成才能结算。捕获文件复用共享SHA/大小验证，原件/native session与省略重复base64的有界journal分别保留。队列只留媒体投影，64条之外再限制总计8MiB；投影超限在再次解码前拒绝，3处消费立即释放。问答后的完整回复先整体脱敏，20KiB保持完整Message，80KiB用诚实Delta并以已发前缀长度/SHA防重复；结束/错误立即释放累计正文。process完整race123项、真实Pi+localhost审批/问答/大回复3项及CLI真实持久历史全文读取通过；没有新增未接通reader的Inspector或Reference。

本机Codex0.165.0和Pi1.4.0各完成真实账户的新发工具读取、运行中Stop、同native身份再发，以及关闭Application后重开原数据目录再读；每次新随机文件读取均有1对实际ToolStart/ToolEnd，Stop实际收尾为cancelled，冷恢复身份相同。Codex另外用仅明确HOME/USER/PATH的Dock近似环境重开验证通过，不靠继承认证环境变量；Pi实际provider/model元数据为magpie、opencode-go/deepseek-v4.1-flash。证据是当前程序经公共bootstrap/产品Runtime产生的SQLite事实，不是模型文字自述。现有产品目录安全连接metadata计数为0，因此Alva本轮采用实际native harness、本地HTTP及无窗口页面验收，没有远程Provider账户可验证。Claude/Anthropic未调用。

共享呈现开始消费当前正式Foundation的`ui.color.*`中性灰/蓝紫角色，Primary改为text/surface，导航静态选中使用文字角色；Input/TextArea/Password/Select普通填充subtle、焦点surface，弱边界及原对称内距/几何保留，Secondary Default/Loading/Hover分别沿实际变体。Foundation/Product保存包未改变；这不表示活动卡片、所有阅读容器或全部页面已经完整像素对齐。最终集成、无窗口画面与唯一安装入口回执保存在`/Users/smallraw/.codex/checkpoints/alva-agent/20261105-daily-agent`。

正式壳新增统一12外距/12区域间距、surface/16圆角与40px Agent欢迎标记。1440×960无窗口主面Home=(244,12,1184,936)、General外观=(256,88)，与保存包原生solve一致；右侧可用宽实际扣除36壳占用，窄窗约束只影响显示。共享Disabled颜色/禁用语义在同一helper实现，含连接包装器的79个消费者只移动原禁用条件；MyGo原生禁用透明度0.5与设计0.6仍有明确差异，没有以shader或重画文字伪装一致。最终11张无窗口亮暗图独立查看无明显阻塞，49项关键几何通过；不扩大到全部页面与平台。

本轮60秒实际Alva harness/本地HTTP/SQLite/MyGo无窗口检查完成27个循环、54次发送、27次Stop、3次应用重开、6次草稿冲突和3次PDF解析；HTTP及物理资源均实际收尾。观测最大Go heap 8,500,176字节、自身RSS 179,896,320字节、PDF子进程RSS 30,801,920字节。仅是该60秒路线的观测，不能推定一小时稳定、真实GUI资源或此前31GB事故已定位。

上轮已知的SDK取消缺陷已按原机制修复：默认loop仅在已有执行错误时合并owner取消/原因，保留实际后端错误，再走原Dispose/OnRunEnd/Flush，不匹配SQL错误字符串，不重试副作用，成功完成后晚到cancel不反转成功。真实SQLite准备阶段中断与插件不执行/不重放反例通过，相关完整套件608通过/1跳过，独立审查无阻塞。最终全仓结果另记；原暂存内容保留，按路径提交被本机检查要求确认而未提交，没有绕过该检查。

最终冻结1092份Go/模块来源，完整desktop-tagged全仓race **3103项含子测试通过、6项跳过，98个有测试包通过、0失败**；全树vet、模块完整性与生产构建通过，检查/构建后来源SHA无漂移。两次真实账户Stop另等到本轮native工具活动才取消，各有一对ToolStart/ToolEnd且最终cancelled、owner已join，未只凭旧branch native ID判定启动。

已正常退出旧进程并原位更新唯一`/Users/smallraw/Applications/Alva.app`，保留Info.plist、原Resources及同一数据目录，最终普通发布进程PID84940、无参数`open -a`启动。签名会改变Mach-O签名段，安装校验已修正为比较22个实际代码/数据section及Go build ID，再严格验证签名；备份/候选均0600不可执行。临时e2e仅替换同一入口验收，随后已正常退出、还原普通构建，私有socket已删除，无额外客户端。

运行中官方automation实际报告content窗口visible=true并成功取得2480×1700自身首帧PNG，独立确认当前中性色/圆角/品牌/输入样式；CUA仍报cgWindowNotFound，CG同时报告onscreen=false。有限idle超时、侧栏动作回执虽为`{}`但树/画面未变化，故真实交互未标通过，General/模型菜单后续动作未执行；未将首帧抓图或headless回归代替这些实机验收。后台UI流程优先继续用NewTester；实际窗口未消费新帧的原因未在本轮确诊。

## 2026-11-05 共享输入稿与原生自绘窗口栏

用户确认的输入组件外观已合并到正式SmallPen Foundation：普通输入/搜索/下拉/文本域使用浅填充和弱边界，标签减重，大编辑器去阴影；原尺寸、对称内距、焦点/错误、变体及功能节点保留。Foundation revision `843480b8e9e0ca197c929aff59707cb88c3f0d7d601d302cdb1bbf468a35349d`，Product仍为 `0c285b1d5e710f87559a6b84c6bb8b312ba1cbe26a73981a6ed23e8e488411f0`。正式两包与已审副本逐文件字节相同，3张原生图和110变体主题目标保留原修订，43128 Web已重新打开正本；这不是生产输入样式全面更新。

按用户明确要求，实际Hub窗口改用32px自绘窗口区域，并隐藏系统标题栏及系统按钮。MyGo固定升级到v0.7.4，Go最低版本和CI同步1.27.3；使用 `TitleBarHidden` + `HideWindowButtons` 保留系统圆角、阴影及缩放边缘。关闭仍走原保存/清理守卫；macOS自绘关闭/最小化/全屏，其他平台最小化/最大化还原/关闭并保留Windows Snap标记。窗口状态不进入Agent/Hub执行契约。

升级的相关收尾保持原功能：命令菜单以等宽Grid表达两栏，无详情/加载/空/错误时仅一栏；独立SDK示例最低Go版本与根一致。CLI在bootstrap返回错误且owner已取消时同时保留原错误和取消身份，不匹配SQLite错误文本、不重试或重放副作用。窗口/菜单相关回归与新增四种无详情布局检查通过，CLI取消回归连续三次通过。最终全仓race有97个测试包通过，`extension/loader.TestPluginCancelledPreparationDoesNotExecuteOrReplay`报告会话消息读取返回SQLite interrupted而未保留DeadlineExceeded；此前整包曾通过，本轮最终仍按未解决记录，不宣称全仓通过，未为本次窗口维护修改SDK/存储或压掉该失败。最终全树vet、模块完整性及构建通过；详见检查点 `final-checks.json` / `known-regression.json`。

唯一 `/Users/smallraw/Applications/Alva.app` 已原位更新并严格签名验证，Info.plist和原数据目录/启动参数保留；最终进程PID99317。实际窗口确认标题为Alva，无独立系统灰条及Go native抬头；自绘最小化与同PID恢复、工作台/通用设置进入和退出全屏、自绘关闭/重开均已观察，最终是普通可见窗口。拖拽/标题区双击受CUA `noWindowsAvailable` 限制未通过实机验收；实际系统栏高度未单独测量，32px由原生无窗口检查验证。其他平台实机、完整生产视觉、外部账户和持续性能验收独立记录。

检查点 `/Users/smallraw/.codex/checkpoints/alva-agent/20261104-window-chrome` 保留来源修订、失败及修正后的检查、源指纹、精确安装/签名与GUI文字回执；该路径按启动日期命名。1074份Go/module来源在最后检查期间未变，CLI/示例收尾后的原生构建与已安装构建的未签名字节完全相同，无需再次打断当前窗口。旧二进制和验证构建只以不可执行恢复材料保留，没有额外客户端。工作树含既有混合改动，本轮未整体提交。

## 2026-11-04 原生 UI 重新对照正式稿

用户实际打开当前构建后指出还原偏差，本轮按正式21/22、Foundation及原生窗口重新检查。修正首页24字号、透明Ghost建议/8间距、导航顺序/完整名称、页头8/4分组，以及输入框内部描边/四边8、标签间距8和麦克风20。对话滚动视口保留全宽，仅每条消息和输入框限宽760；私有工作区不再显示生成目录ID。

工具消息采用紧凑活动/明确展开和详情读取，typed引用独立保留，移除拼接在正文里的引用JSON。详情沿原branch的真实Inspector元数据取得URI，再复用现有有界读取；异步结果须匹配会话/branch/行/视图身份。共享虚拟列表、阅读锚点和Markdown保留，direct原输入表面独立。

同时修正原生SPEC中残留的“标题32”为现有heading Token引用；公开CLI dry-run/apply均0 warnings，仅该说明文件改变，图形/Token/组件不变。Product revision `a2260b0516feb62a425cd773d17a6047dcaf7f124603cfa5872d2ce8792031db`。字体原件、权重映射与实际CoreText选字核实相符，不通过逐标签偏移或注册重排掩盖观感差异。

本轮最终检查和实际更新回执待收尾，检查点 `/Users/smallraw/.codex/checkpoints/alva-agent/20261104-ui-alignment`；历史测试数量不作为本次通过结果。旧格式数据保持，新Go客户端使用明确的独立Go数据目录，本轮没有真实外部账户调用。

## 2026-10-27 按模块闭环：交互、资源、连接与协作

本轮沿已确认的 SPEC、共享组件、产品服务、SQLite、失败/取消/重开逐模块实施；没有新增执行模型或扩大六方法 ExecutionStore。以下是具体关闭的流程，完整产品、真实设备和跨平台仍分别验收。

- 问答与审批：native 响应成功后立即隐藏已接收请求，保留原 pending 事实直到真实 resolved 事件提交；重复回应与旧 binding 拒绝，失败仍可重试。长正文有界滚动，动作行独立，Loading 保留尺寸与焦点；持久等待的空白 Other 不提交。
- 资源工作区：逐会话保存拖拽/键盘重排、宽度、分页与查询，宽度默认复用 Foundation 432，临时视口夹取不覆盖偏好。关闭/隐藏/弹出/返回只迁移观察视图，不 Stop；返回优先已有来源窗，来源已关闭则在当前窗恢复。页头复用单行52、36标签与32动作，选中底色、完整关闭、双分页按钮和拖动提示均有实际布局/像素回归。
- 模型连接：保存核对完整配置回执；缺失连接不借用其他对象。新建通过可选 RequestID 端口与 SQLite v7 脱敏回执账本恢复未知结果；同请求精确重试只有一个连接，已编辑/删除目标不能被旧请求覆盖或复活。未保存表单及 UI RequestID 仅本窗保留，不宣称应用重启后自动找回草稿。删除确认绑定 ID/revision。
- Team/Worker：发送捕获实际 Worker 会话的文件/Skill，复用普通发送的冻结与归属检查，进入既有耐久 FIFO。编辑排队正文不修改冻结引用；启动前复验原件与派生字节，变动零 HTTP 请求。重开冷读；保存等待答案不执行，明确继续同 Turn，后续队列也须明确 Resume。
- 伙伴、群与目标：迟到 CRUD/私聊回执不覆盖新选择或导航；已停用成员可从群移除但不能重新添加。Goal 创建与读取验证精确身份、范围和原始请求，未知结果以原 RequestID 显式恢复，不自动重放。
- 自动化与工作树：删除确认固定审阅 revision；实际 Git 已完成的原 owner 能结算，即使 Lead 后来准入或 Team 结束。Waiting 阻止新变更准入，证据读取仍可用；不增加 Run once 或自动合并策略。
- 工具、插件与设置：冷读及变更回执绑定会话、generation 与预期 pane，旧结果不覆盖后来表单或把页面拉回。共享服务持有实际保存/停止收尾，页面离开不伪报完成。
- Gateway 与停止操作：窗口冷读真实 listener generation、closed/stopping；旧启动回执不能展示新服务的旧 token。Gateway、普通会话、伙伴与明确 Worker 执行的停止控件绑定显示时的不可变 owner，按住旧按钮再替换任务不会停止新 owner；全局 Worker Stop/FIFO 语义保留。Codex 发送文件前补原件/派生大小、SHA、regular-file 身份及取消校验，不声称消除随后文件系统竞态。

SmallPen 原生 SPEC 已补资源、伙伴/群与独立 Worker FIFO 文字契约，并更新连接/原生交互边界。公开 CLI dry-run/apply 均0 warnings，Product revision `15d39a49499d6df9ee69cd53c8f00b3506e110e2670f0680981946c35a7037d1`；Foundation、页面、组件、Token及原型连线未变。

独立审查发现并修复 v7 账本主键/全局唯一约束校验、资源拖动提示被覆盖，以及 Gateway/编辑器旧按压 Stop 跟随替换 owner 的问题；原始复现均在修复后通过。公开可选端口与 Gateway 生命周期面已显式刷新快照，SDK 内核职责不变。

最终全仓 desktop-tagged race 通过98个有测试包、2981项测试/子测试，5项按设计跳过，0失败，事件起止316.924秒。全树 vet、go mod verify、gofmt 与原生构建通过；1068份 Go/module 来源在检查、构建与安装期间保持不变，公开面已显式更新，原暂存区哈希未变。混合既有改动没有整体提交。本轮没有真实 Claude/Codex/付费模型请求。

唯一 `/Users/smallraw/Applications/Alva.app` 已原位更新并签名校验，当前PID34170；Info.plist、Resources及明确启动参数保留，旧备份和候选均不可执行。只确认一个 canonical 进程；CUA 在激活同一 bundle 后仍报 `cgWindowNotFound`，未成功挂接或执行资源弹出/返回/分页操作，因此系统窗口数量和这些真实交互均不记为通过。资源三张明暗/宽度无窗口画面及来源见[本批证据](./docs/design/alva/exports/archive/native-module-closure-2026-10-27/resource-headless-review.json)，保留捕获源码指纹；之后拖动提示实现与回归变化单列，不改标旧图。

完整检查点为 `/Users/smallraw/.codex/checkpoints/alva-agent/20261026-module-closure`，入口 `final-checks-summary.json`、`state.json` 和 `native/resource-window-acceptance.json`。工具配置模块辅助差异包归档曾被自动审批以疑似密钥路径、确认超时拦截，未重试；源码与验证已完成，该辅助归档保持缺口。前轮一小时压力测试不算本轮新增逻辑的压力验收，31GB事故也未据此宣称完全定位；IME、设备/声学、其他平台、账户交互及尚未曝光的HTTP入口仍分别列账。

## 2026-10-26 首页编辑器、真实本机Codex与耐久草稿（本轮验收）

本轮按用户完成的逐项确认实施首页/会话编辑器；不代表整个产品或所有模块完成。Production仍为Go/MyGo，SDK内核及AgentLoop/Graph/EngineRuntime职责不变。后续伙伴/群、Team/Worker/自动化等保留实现，继续按域审查。

- 逻辑home/session草稿由独立CAS服务和SQLite v6保存，同一会话跨窗共享；旧回执只消费捕获revision。首次Create与PendingSessionID原子提交，session完整副本保存后才清空home；重开不自动Send。普通Save不能伪造PendingSessionID，实际home→session文件采纳保存不可变grant，跨会话DTO不授予原件读取。
- 首页可先选电脑任意位置/类型的文件、项目内受管Skill；原件流式存私有附属目录，catalog/原件/派生各有SHA。短文本4KiB/文件、16KiB/请求；长文本经既有context_read/search有界读取，后续同会话引用仍可用。Office为有损文本派生；macOS PDFKit为独立owned进程，32MiB/512页/2MiB/30秒，256MiB/20ms观察watchdog，明确不是内核硬内存沙箱。选中PNG/JPEG/GIF复用media的原件/派生/selection/receipt，只给明确视觉路由的一次请求，不启用全部media Tool或重放历史像素。
- typed文件/Skill与原始用户正文随同一SQLite Turn准入；Runtime执行存储继续六方法。所选Skill固定workspace/原文SHA，只暴露本轮所选目录，不全局启用。能力不符的自动fallback被跳过，不丢附件后执行。
- Codex默认为app-server，复用明确指定的本机CLI配置/登录状态；无产品登录或授权秘值复制。按用户要求对照Cindy固定修订继承本机环境/HOME、独立设置CODEX_HOME，不写mise信任或绕过hook。继承环境只提取已知敏感键作有界脱敏，公开HOME/PATH保留；run-local原始尾片跨帧过滤，完整大工具结果仍可引用读取。人类秘密答案保留更严格的raw/delta省略，typed问题控制字段不被字符串政策破坏。native审批/问题绑定execution/attempt/engine/native/request，响应消费后发送失败会关闭并真实join原进程。Claude只跑fixture，不调用账户。
- General只显示真实主题/字号/减弱动效，语言/密度入口暂隐藏并保留旧值；应用减弱与系统要求取或，保存前不改外观。按Foundation修正38×24 Switch/16px滑块、General平铺800限宽/24外距/16区块距、Commands640双316栏和37px图标行；6px输入卡片锚点和32圆形Send保留。
- 用户追加指出模型选择器还原偏差后，按既有ModelRow改为整行hover/selected/点击，右侧收藏与参数独立32px动作/20px图标，8/12内距、8间隔、14/600标题与subtle选中底；不再添加设计没有的勾号。正式设计不变；21项定向race及8个明暗/长名称无窗口画面通过，证据见[ModelRow审查](./docs/design/alva/exports/archive/model-picker-2026-10-26/report.md)。

独立审查已发现并修复文件越域、Pending伪造、秘密回显、响应取消后pending丢失、连续秘密问题控制字段误脱敏；harness烟测又发现并修复首发移动引用时误改home版本的问题。后续环境审查的短秘密反复脱敏替换标记、Windows混合大小写环境丢失两项P2均修复，并由原复现和独立复核确认。

最终全仓desktop-tagged race命令通过98个有测试包、2807项测试/子测试、5项跳过、0失败；vet、mod verify及原生CLI/desktop构建通过。1067份Go源码/module哈希在最终检查与构建期间保持不变。最终同源码首轮仅因公开面快照记录尚未包含两个私有helper的imports失败；审阅确认没有导出签名变化，显式刷新后复验，未变包复用本轮有效Go缓存。先前选图fixture改为真实原子采纳，没有放宽归属。Git原暂存区保持不变，混合既有改动未被整体提交。

一小时v2 race验收实际3600.644秒：1477周期、2954次本地HTTP Send、1477次运行中Stop/join、148次应用重开、296次草稿冲突恢复和148次PDFKit提取，物理清理完成、临时数据移除、HTTP活跃0。Go堆观测峰值8.950MiB，关闭后GC堆3,200,024→3,245,944B，协程始终10；进程RSS峰值588.25MiB且继续增长，未建立平台或长期有界结论。10分钟无race对照RSS峰值75.172MiB；独立原生客户端1755秒闲置RSS约108MiB保持稳定。此小时只覆盖冻结Alva/localHTTP/SQLite/MyGo无窗口/PDF路线，不包含后来Codex环境或ModelRow修改，也不能据此宣称31GB事故已完全定位。v1因测试错误假定CAS获胜顺序而停止，不与v2累加或改标通过。

真实Codex0.164已完成回复、同native恢复与45秒取消后的真实中断/join；合法旧Turn usage通知回归已修复。按Cindy环境修正后，再恢复原thread，以一对真实commandExecution start/end读取新随机测试文件，工具结果和最终答案均精确匹配，Turn completed，耗时14.286秒。没有修改配置/信任文件或调用Claude。

唯一`/Users/smallraw/Applications/Alva.app`已原位更新并签名验证，PID66728；1067份来源哈希匹配，Info.plist/resources保留，旧备份不可执行。真实macOS窗口已核对模型菜单、收藏/收藏筛选及独立参数面板，收藏恢复、选择未改、无Send；实际hover仍由无窗口指针测试证明，CUA无纯hover接口。IME、读屏、语音许可/设备、完整窗口和其他平台继续单独验收。完整回执见检查点`/Users/smallraw/.codex/checkpoints/alva-agent/20261026-core-composer`的`post-review-final-checks-summary.json`、`stability/one-hour-v2`、`real-codex/env-tool-product-facts.json`与`native/model-row-update.json`。不宣称整个产品完成。

## 2026-10-26 正式设计稿14页更新（设计范围）

已通过公开SmallPen typed操作保存21/22/23、27/29、31–38、40正式页面，并修正3个共享组件集的10个既有变体。收紧设置分组和阅读宽度，模型工具栏合组且列表随内容收缩，补General明确保存/重新加载，移除无真实数据的耗时/速率/Context样例。旧节点/页面/变体身份、主题Token和599条原型连线保留；General新增两条annotation引用原SPEC。详细页面、修订及证据见[设计记录](./docs/design/alva/DESIGN.md#正式页面稿更新--2026-10-26)。

18组最终原生导出为resolved、0报告问题；初稿14张像素未变、另4张经独立复查，窄宽度孤字已修复。三组受影响组件的原生validate通过，自动对比度仍有跳过项。保存包与候选逐文件一致；现有43128服务已重新载入Product修订d22789819c74715624526554bee9fbd91f881e986b6c8c121bbe090ff8f14f38。没有改生产Go代码、启动客户端或调用真实账户；静态设计和HTTP载入不代替真实窗口或业务验收。检查点`/Users/smallraw/.codex/checkpoints/alva-agent/20261026-formal-pages`保留完整前后基线、候选、检查和回执，未触碰原暂存区。

## 2026-10-26 按模块对齐：共享控件、输入与设置首批

建立[逐模块对齐顺序](./docs/HUB-FLOWS.md#逐模块对齐顺序--2026-10-26)，按功能 SPEC、共享组件、交互、产品服务、存储、失败/取消/重开与实际画面逐层核对；首批实施覆盖共享 Button、工作台输入、连接操作、General 保存和语音状态，不代表上述整域或其余模块完成。

- 原生控件对齐既有 Foundation Loading：Text 保留文字测量、居中 Loader，Leading 沿用原图标槽；真实在途操作保留名称、位置和焦点，重复激活不再调用 handler。Ghost/Danger 与焦点环使用已有角色色，Full 确认复用同一控件。Loader 使用共享 Lucide 图形，仅重绘、不重建页面或新增计时 goroutine，系统减弱动效时静止并停止请求帧；这不等于此前 31GB 内存事故已定位或修复。
- 编辑器增加平台 ⌘/Ctrl+Enter，使用当前 TextKey 正文并与发送按钮共用可发送判断。普通 Enter 保持换行／菜单选择；候选菜单、IME、录音、正在提交、运行、等待和加载不能绕过准入。迟到接收回执仍只清理匹配草稿。保存权限/Plan 独立显示“正在保存设置”，真实回执前不改变已提交策略或发送；执行、审批与 SQLite 契约未另造一套。
- 连接获取/测试/保存、General 保存采用共享 Loading；获取失败后的“重试”保留原名和宽度。General 移除重复在途描述行，实际存储未返回时不再把保存按钮往下推，也不重复写入。录音显示状态点和计时，转写保留文字与 Loader，录音与 Agent 的停止继续由各自 owner 真实收尾。
- 公开 SmallPen typed batch 仅更新 `home-composer` 的5条既有说明，对齐触发边界、焦点、快捷键和独立保存状态；10条 requirement、20条 annotation 的 ID/标题/链接、全部图形、Token、组件/变体和页面保持。Foundation revision `608325d27c9c3647e024a43d91cd4c3ea82413e069966646f96f1d4b15406e35` 不变；Product 元数据 revision 更新为 `7195d58b760c54c8bcfac0156ac582646feb8dd41cfd13f8906307f0f38115fb`，0 warnings。

最终全仓 desktop-tagged race 通过97个有测试包、2682项测试/子测试，0失败，耗时356.114秒；1005份 Go 源码/模块在检查期间未变。全树 vet、go mod verify、原生构建通过，无新增公开契约。

四张[原生无窗口样本及来源](./docs/design/alva/exports/archive/native-module-alignment-2026-10-26/report.md)已核对：真实 gated 存储/localhost 目录请求保持按钮几何与焦点且只执行一次；提交/停止/转写是明确标注的合成展示状态。系统窗口、IME、读屏、真实设备和外部账户另行验收；本轮没有启动客户端、麦克风或真实 Claude/Codex 请求，也没有替换已安装应用。完整检查结果与冻结源码证据见同目录验证记录及检查点 `/Users/smallraw/.codex/checkpoints/alva-agent/20261026-module-alignment`；原有暂存区保持原样。

## 2026-10-25 既有客户端原位更新

用户明确接受在未保存窗口状态未知时关闭重启；据此解除前轮安装等待。核对1006份Go源码/模块与前轮通过的冻结哈希完全相同、候选SHA匹配后，原位更新唯一`/Users/smallraw/Applications/Alva.app`并通过严格签名校验。构建ID为`Mi5dB2h6Dj7Q1hJib3d_/d8zvA4l3GzPUr9EPKqIb/uzenG9Z-9A9gaRr5vosA/s_jERrETMGpnyHvZTCor`；旧二进制以不可执行权限移到废纸篓，原数据目录保持不变，当前仅PID69585运行，诊断服务已关闭。

真实Codex经同一原生窗口owner完成Send→Stop→再Send；只读核对本次新增QA session，三轮为completed/cancelled/completed，两次精确回复、同一native身份及零工具调用，Stop等待实际CLI子进程回收。首验因维护脚本把移动的latest别名与0.163.0内部目录拼接而启动失败；本机latest已指向0.164.0，原路径不存在。维护脚本现固定到此前验证过且仍安装的0.163.0绝对路径，再验通过；未改账户配置，未读取凭据，保留首验失败事实。没有真实Claude执行、认证或账户查询。

回执、首验路径诊断和最终持久化核对保存在前轮`20261024-product-completion-map`检查点的`native-ui`、`native-ui-retry`与`validation`中；原暂存区保持原样。capture仍frame_pending，不把业务回归通过写成compositor/AX或全部视觉验收通过。已知未闭环能力继续见Hub业务账，不宣称整个产品完成。

## 2026-10-24 正常导航、群编辑与未完成功能核查

本轮继续核对设计意图、真实桌面handler和SQLite事实，确认并修复三处已有测试遗漏的业务错误。没有完整需求总表和逐域验收之前，不给产品完成率；[Hub业务账](./docs/HUB-FLOWS.md#尚未闭环)列出13类已知未闭环能力及各自关闭条件，仍不是完整需求清单。

- 正常selectSession/loadTail/poll此前虽读Settings，却只采纳Plan/Permission，导致菜单与Send预检忽略已保存会话参数；独立参数对话框测试手填p.settings掩盖了真实路径。现独立保存已提交会话参数观察，复用已有读取，按session/branch/generation/epoch与单调revision采纳，Save/clear/Reload与跨窗轮询统一更新。未知/读错不退成默认，存在实际重试入口；显式空覆盖和缺省保持不同。行内effortDirty、参数表单和高级配置草稿保留，不改已冻结Attempt或等待续轮。回归从真实导航开始，另以纯Go pi fixture验证全局generation被会话空覆盖后实际选择/发送；没有启动Pi或真实账户。
- 群设置改用独立groupDraft，Cancel/返回/跨群导航不污染已提交回复模式、成员或工作区，失败留稿。成员/工作区保存产生新scope后，清除旧正文/轮次/cursor/恢复批准并冷读新事实；metadata编辑保留原范围历史。异步读取和保存按ID/scope/epoch隔离，A→B→A不采纳旧回执，不改变原执行/等待owner。两项实际UI/SQLite回归在旧实现下先失败，修复后含原真实本地HTTP等待/历史/Stop的18项Companion race通过。
- Density当前正式ui.*两档几何未区分，生产布局不消费该字段。原生选择暂禁用，保留既有值及存储/HTTP契约；说明放在enabled可聚焦行供hover/键盘读取，不用禁用Select自身的无效tooltip。不改设计尺寸来制造差异；缺口继续保留GAP-04。Theme/字号照常应用；系统theme的疑似问题经MyGo每帧reset源码反证排除，没有无根据改动。

最终全仓desktop-tagged race通过97个有测试包、2663项测试/子测试，0失败；1006份Go源码/模块在执行期间不变。全树vet、go mod verify、原生构建、公开面/架构/分形检查及diff检查通过；本轮无新增公开契约。6处旧测试fixture改为真实selectSession观察，保留原业务/迟到/owner断言。独立最终审查未发现本轮范围剩余确证问题；禁用说明的tooltip问题已修复并有hover/键盘回归。

10月24日原生候选构建后，因CUA仍cgWindowNotFound、未能核对草稿而暂缓替换；该安装等待已在10月25日用户明确接受关闭重启后解除，原位更新与真实Codex回归见上节。视觉验收仍未通过，没有真实Claude执行、认证或账户检查。

检查点：`/Users/smallraw/.codex/checkpoints/alva-agent/20261024-product-completion-map`。保留实施前基线、当前增量、审查/测试/构建和待更新状态，原暂存区保持原样；仍缺外部双向回调、账户管理、首页项目引用/Skill、语言/密度实际行为、资源拖拽和真实平台验收等，不宣称软件完成。

## 2026-10-24 业务域职责、引用归属与执行边界

按业务owner而非页面划分职责，更新[业务域/事实/入口覆盖地图](./docs/ARCHITECTURE.md#hub-页面与业务落地)与[可重复流程](./docs/HUB-FLOWS.md)。明确desktop/server负责交互与传输，product负责共享业务政策，各执行/调度owner负责认领和真实收尾，sqlite负责事务事实，bootstrap一次装配。共享服务已实现与HTTP入口已曝光分别列出，不新建每页一套Service或合并独立runtime。

- 路由完整账户/计费/能力准入收敛到product/routing，同供桌面、HTTP、普通Send、Worker、自动化和伙伴使用。显式Switch/Fallback必须是当前配置，fallback另需逐项批准；保留旧branch供冷看历史不授予新执行。实际adapter仍独立校验Provider/revision绑定，直接core消费者不能借过期配方执行。
- 附件/项目引用每次准备有独立DraftID，Send/Worker/HTTP明确传版本集合；空集合不带附件，成功只消费本次捕获集合，不再隐式取整会话草稿。本窗保存自己的选择，关窗等待真实在途操作返回后释放未消费暂存；取消picker与迟到成功回执均可释放未采纳项，保留原件。暂存总额仍是每会话共享8条/40MiB，不承诺每窗独立额度。
- 原生审批首次读取Detail前冻结execution/attempt/engine/native四项PermissionBinding，Runtime在原control→mu锁序内原子重验后派发；替换owner及同run fallback不能接收旧审批。实际owner替换用同一测试在旧Service响应体overlay下稳定失败、修复后通过。相同native字符串的外部引擎不借Alva broker。问答/审批防重、独立读取错误和迟到poll/回执分别隔离，旧操作不能覆盖新页面或恢复已答请求。
- Worker Stop冻结下次FIFO认领，再取消并等待捕获owner真实退出，覆盖原pump之外的同Turn等待续轮；超时仍保留实际清理，迟到旧executionID不取消替换owner或停掉原FIFO。Automation仍是暂停优先的interval CRUD，没有新增Run once契约。

最终全仓desktop-tagged race通过97个有测试包、2646项测试/子测试，0失败；1005份Go源码/模块在执行期间不变。首轮发现两项bootstrap旧测试仍期待失效路由先创建失败Turn，现保留产品准入前拒绝和直接core实际adapter拒绝两层覆盖，定向通过后完整重跑。全树vet、go mod verify、原生构建及diff检查通过，公开快照显式更新；独立最终审查未发现本轮范围剩余确证P1/P2，另有3包14项独立定向race通过。

唯一Alva.app原位更新并通过严格签名校验，旧二进制移到废纸篓，保留原数据目录。真实Codex经同一窗口owner完成Send→Stop→再Send；只读核对本次新增QA session，三轮为completed/cancelled/completed，两次精确回复、同一native身份及零工具调用，Stop等待实际子进程回收。当前仅PID92498运行，诊断服务已关闭。capture仍frame_pending，未取得有效窗口像素，不宣称compositor/AX或完整视觉验收通过。

本轮检查点位于`/Users/smallraw/.codex/checkpoints/alva-agent/20261024-external-interactions`，保留实施前基线、逐项测试/审查与增量；原暂存区保持原样。按用户最新约束不启动真实Claude、不查询认证/账户。外部交互DTO/contract源码经一次同工具重试仍被自动审批以workspace-name-heuristic拒绝，未修改这些文件、未换工具绕过；Codex app-server双向审批/问答仍未实施。HTTP尚缺连接CRUD/参数、目标/typed项目引用、伙伴/群及资源视图入口，真实设备/可访问性等边界继续单列，不宣称产品完成。

## 2026-10-24 目标重用、伙伴历史与连接推理闭环

按真实用户流程并发补齐三个领域，并由独立审查发现操作层缺口后回归修复；没有新建执行runtime、扩展六方法ExecutionStore或迁移用户数据。可重复的操作、服务/SQLite事实与测试入口集中在[Hub业务验收](./docs/HUB-FLOWS.md)。

- 同一目标Dialog支持项目/当前会话冷浏览、真实keyset分页与重选；选择只准备同一目标原件的typed引用，明确Send才执行。首页在Create之前重验目标canonical范围及命令；项目A目标切到B、或外部引擎选择Plan，均不创建空会话并保留草稿。旧scope异步结果隔离，换范围同时清理旧分页游标。
- 独立可选CompanionHistoryStore提供已删除身份索引与消息/回合游标页；SQLite和实际产品/无窗口UI覆盖258条消息、129轮及删除后重开。归档可看私聊、群摘要和成员原始正文，不恢复身份、账号批准或新执行。SQLite beginAttempt事务重验当前tombstone，覆盖Submit、direct Runtime、等待续轮和已批准fallback；已有owner仍真实收尾。可选端口缺席时保留同一组件的原CRUD/私聊/有界群观察，明确不能加载更早历史，实际Stop仍可取消/join owner。
- 已提交Connections实时发布精确revision的Developer目标，重开可发现；显式Run在独立owner内先冻结全部目标，再发出真实模型请求。编辑/禁用撤销未来目标，已运行job保持旧配置。读回私有配置复用既有纯政策重验，仅实际消费者构造Provider；DTO/计划/报告不含凭据。解析移到owned goroutine，读取阻塞或所有目标消失时Stop仍可触达，真正退出前不伪报完成。真实本地HTTP验证生成参数、工具schema/参数评分、首请求前失败零调用与配置冻结；目录GET不冒充推理。
- 按用户新约束，本轮不启动真实Claude、不查询账号或认证状态、不调用模型。仅用项目fixture子进程补齐native result判定：success同时is_error仍失败，冲突session和result后执行输出不能伪报成功，纯system尾随通知可读至真实进程结束。新增回归在本轮前mapping overlay下有三项稳定失败，当前通过。

最终全仓desktop-tagged race通过97个有测试包、2594项测试/子测试，0失败；999份Go源码/模块在执行期间未变，耗时406.67秒。全树vet、go mod verify、原生构建及diff检查通过。Goal/CompanionHistory/Developer可选公开面已显式更新快照，独立审查发现的四处P2和一处游标P3均修复并复核。

唯一已安装Alva.app原位更新并通过严格签名验证，旧二进制移入废纸篓，原数据目录保留。真实Codex经同一窗口owner完成Send→Stop→再Send：仅本次QA session的SQLite核对两次精确回复、completed/cancelled/completed、同一native身份及零工具调用；取消时实际CLI子进程已回收。仅PID54404保留运行，诊断服务已关闭。CUA无法匹配窗口，系统窗口截图失败，诊断capture仍frame_pending；只读CG元数据曾确认旧PID确有onscreen窗口，因此不能把CUA匹配失败说成不存在窗口，亦不宣称新UI已通过compositor/AX视觉验收。

本轮前基线、实施增量、测试/审查/原位更新回执保存于`/Users/smallraw/.codex/checkpoints/alva-agent/20261024-codex-business-flows`；原暂存区保持原样，未把其他既有改动混入提交。外部双向审批/问答仍未接通：首次普通engine交互DTO源码写入被PreToolUse以workspace-name-heuristic拒绝，未写入文件、未换工具绕过，等待用户明确确认。Codex app-server文档及本机schema仅研究，不算适配完成；上轮资源拖拽/改宽阻塞亦未重试。其余平台和产品未闭环项继续按业务验收表单列，不宣称整个产品完成。

## 2026-10-24 显式本机 CLI 配置与真实请求验收

- Hub CLI/桌面新增显式`--codex-config-dir`/`--claude-config-dir`，仅配对应已批准程序使用。默认私有配置不变；Alva只验证canonical目录，由CLI读取完整配置，不读取、复制或链接认证文件。对应vendor的目录可复用，HOME及其他profile仍私有；持久route绑定目录SHA，adapter在执行前拒绝不匹配的非空来源。指纹不识别同目录内部登录者变化。Codex新建与恢复都在exec父层固定只读sandbox；没有新增忽略规则或绕过审批参数。
- 原生诊断新增有界、opaque SHA路由列表与`select_route`，选路走模型菜单同一handler；首页/会话`set_draft`、Send走可见输入区的真实Create/Switch/Upload/Submit，controls/execution分别授权。草稿只给长度/SHA，路由键不泄漏端点/账号来源；隐私回归经独立审查复核。
- 全仓race发现伙伴Run/Continue完成后的刷新被已有group/roster读取挡掉，旧快照随后让UI误显空闲。新增待刷新标记，旧读不提交已被替代的快照，释放读owner后读取完成事实；不放宽running或自动执行。publication间隙及两种旧读窗口有确定性回归，旧实现overlay下后两项稳定失败。
- 真实Codex窗口停止验收进一步发现：retained native恢复时，在CLI回报header前取消，journal引用却用空native ID，被SQLite严格校验拒绝并误记failed。现仅在resume binding核实后为引用保留已知来源，实际SessionStarted仍由CLI header确认；未放宽引用校验、foreign身份或强错误。真实fixture贯穿Runtime/SQLite验证header前取消为cancelled，随后同branch/native可继续。
- 本机Codex CLI 0.163.0已通过实际Hub CLI三轮：指定正文、真实工具读取测试文件、重开后续聊，均exit0；原始journal确认同一native thread、两个MCP工具完成且读取结果含测试标识。没有读取认证内容。Claude 2.1.310的所选/默认目录公开状态均未登录，未声称真实Claude模型或订阅验收。

最终全仓desktop race通过97个有测试包、2558项测试/子测试，992份Go源码/模块在执行期间未变；全树vet、go mod verify、原生构建通过。同一已安装Alva.app原位更新后，真实原生窗口owner操作完成Send→Stop(cancelled、CLI子进程已回收)→再Send；SQLite仅按本次测试session核对两次精确答复、completed/cancelled/completed与同一native身份，诊断服务随后关闭。仅PID74316的Alva保留运行；数据目录未迁移或删除。当前帧PNG仍返回frame_pending，未以旧scene或Tester冒充真实画面；OS compositor/AX与完整视觉验收仍未通过。

执行、审查、冻结源码和单客户端验收回执集中于`/Users/smallraw/.codex/checkpoints/alva-agent/20261024-workspace-polish`。外部进程当前仍无双向审批回调，不能将原生Ask/Auto/Full显示等同外部引擎审批；Cindy采用的Codex app-server交互协议属于后续适配工作。资源标签拖拽/分隔宽度的新增补丁曾被本地PreToolUse以protected-credential-file拒绝，未完成部分已退回本轮前基线并保留在检查点，未更改审查策略。本轮不宣称所有产品流程或平台已完成。


## 2026-10-24 伙伴、资源工作区、会话参数与系统语音

本轮按独立业务owner并发实施，复用现有Hub Runtime、实际审批/问题和native事实；共享工具/存储政策只维护一份。桌面继续Go/MyGo，未引入前端框架、第二执行runtime或默认存储fallback。

- 独立`CompanionStore`/`CompanionService`持久化Profile、群、群×成员会话、固定成员/Profile快照及有限回合。私聊与群隔离，成员/工作区改变产生新scope，保留原历史。Skill以来源session/branch/name/原文SHA冻结，群只用明确共享项；实际repository的List/Load/Resources/Resource均按允许列表重验，空scope不读全部目录，受限scope拒绝compatibility导入。
- 群等待保留原Round、成员Turn、Stage和交付游标；重开/审阅/保存答案都不自动执行。明确继续复用原ResumeWait，已完成成员只补幂等回复，尚未开始者按原快照推进，未知效果不重放。进度/发布/终态失败允许显式核对已知事实，不以UI成功掩盖数据库running。Stop绑定Round及不可变operation，并用准入同步返回的实际executionID取消/join，不发现并取消替换owner。
- `WorkspaceViewsService`/独立`WorkspaceViewStore`保存逐会话标签、来源、查询/分页/滚动/专注位置。原件/文件/任务/Inspector读取实际资源并验证来源，取消读取由同一owner收尾；标签关闭/同app弹出不Stop、不另起客户端。重排为菜单操作，拖拽/拖动改宽未实现，媒体播放与递归Inspector仍走已有pane。
- 当前会话模型参数按稳定账户/模型键独立CAS，明确Save/Reload/Clear及当前会话/模型默认范围，失败保稿/过期路由拒绝。下一新Turn读取覆盖，运行中或Waiting恢复使用原Attempt参数；配方hash排除可编辑的下一轮参数表，保留实际冻结参数和Skill scope。修复新库空模型偏好被Clone成nil后误判同revision冲突。
- macOS系统语音通过已有purego调用Speech/AVFoundation：仅明确点击才请求麦克风/语音许可，要求本地on-device可用并验证请求开关，默认zh-CN/最长60秒；不创建音频文件或云fallback。停止录音与任务Stop独立；离开聊天停止并保留待采纳文字，首页首次持久创建随该草稿迁移；取消/超时/关窗等待真实task与回调退出，迟到结果不覆盖其他会话。其他平台明确不支持。真实设备、系统许可拒绝和识别质量尚待点击验收。
- SQLite v4→v5迁移在同一事务新增独立伙伴/回合/消息及资源视图表；会话参数仍用原Settings JSON。迁移和完整schema校验先于恢复，失败回滚版本/新增表且保留连接/目标/模型偏好，不重置或换库。崩溃恢复只标记未决群回合，不执行。

设计只用公开CLI/revision守卫更新既有home-composer/providers的行为元数据，Product为`7e2af36356bfb58acc2276853bcf8ef0615b190be52c2cce4de0fee649f6cebf`，Foundation保持。原生headless核对浅色960×680/1240×850，修复群长成员不可选择、资源选中浅底白字和新库参数误冲突；长转写的采纳动作置前，多引用/5行正文下Send及任务Stop保留。

最终全仓desktop-tagged race为97个有测试包、2524项测试/子测试，0失败；990份Go源码/模块在最终执行期间无变化。首轮发现一项旧键盘回归假设Permission紧邻Send，新增独立麦克风后该假设失效；改为按语义焦点查找后单项及最终全仓通过。最终全树vet、go mod verify与原生构建通过；Linux仅构建speech不支持stub，不代表平台实测。完整测试、隔离验证、原位客户端更新与源码冻结回执保存在`/Users/smallraw/.codex/checkpoints/alva-agent/20261024-business-workflows`。先前历史记录不代替本轮验收。本机Agent认证、可信Context上限、语言资源切换、应用级减弱动效覆盖及运行中插话/自动排队策略仍缺或待定，不能称整个产品全部完成。

## 2026-10-23 模型连接、权限与单客户端收口

并发接入模型连接共享领域、正式原生设置页与既有设计交互契约；另由独立Agent审查业务边界、无窗口权限画面及唯一预览的真实macOS更新。未新增SDK政策、执行runtime或默认数据库fallback。

- `hub.ConnectionsStore`、`product.ConnectionsService`与SQLite v3共用一份编辑/CAS政策。metadata和credential分表、私有文件权限保护，不宣称加密/系统钥匙串。列表与普通DTO/错误无Key/header值；空Key编辑保留，显式Clear/Auth=none删除。失败/冲突保稿、明确启停/二次删除、手填模型都进入实际存储。
- 已启用连接发布冷路由快照，明确选择后每Attempt按`connection:<id>@<revision>`解析实际Provider/模型/账户/计费/视觉声明；禁用、删除或修订使旧路由明确失效，不切账号、不取消已运行快照。桌面/HTTP/Team/Automation共用已提交事实，Team Tool用实例冻结绝对索引与独立8KiB路由分页。默认仍由程序明确配置。
- 原生Ask/Auto/Full只影响下次明确Send；Full先确认现有范围，首页确认绑定workspace/route并在迁移后消费。Auto仅已知原生文件写入自动批准，命令/网络/MCP/unknown仍问；Full不扩大Guard根目录/敏感路径或启用独立能力。Plan清除潜在Full并恢复Ask；运行/waiting/未放弃checkpoint冻结，普通Settings导入不升权。
- 独立审查发现的多模型Team索引超预算、Gemini转义模型路径丢失、真实连接仍标为demo三处P2均已修正，并以原复现复验闭环。Gemini修复保留既有operation切换与显式escaped segment。

全仓desktop-tagged race首轮96包、2285个测试/子测试通过，0失败，143.90秒；执行结束后923份源码/模块哈希与开始一致。三处审查修复后，桌面/HTTP/architecture/provider四包完整race492项通过，41.71秒，执行期间源码未变；product/bootstrap两包完整race116项通过。仅本任务差异的隔离导出分别通过连接七包595项、权限/Plan/连接/公开面五包65项race，确认不依赖原有Developer/CLI改动。全树首轮vet、go mod verify及修复后相关vet/原生构建通过。真实本地HTTP验明path/model/Key/header、重开同native续轮、禁用后零provider调用；临时SQLite覆盖CRUD/并发CAS/失败事务/v2升级与回滚，不代表外部账户验收。

两个旧预览应用包已按用户要求移入废纸篓，数据目录保留；仅有Alva Layout Preview.app按原app/bundle/data-dir正常退出后原位更新，PID59581→68503。更新前空稿、无附件/执行任务；真实macOS核对设置→模型与连接、空列表/刷新/添加和左上返回，最终回工作台。未Send、调用外部模型或保存凭据/设置。原生无窗口权限明暗四图在960×680核对菜单/Full确认、6px间隔与32px圆Send，关闭/确认不执行。

设计仅更新既有requirement与实施状态，Product revision `1210803cfc4bc7ad81cbf2cec60ac36833cf9b2ffaf01e34fe7753d5e2137966`，Foundation保持；Token/组件/72页面未扩张。连接测试/自动模型目录、本机Agent账户认证、目标完整领域、常驻伙伴/群、语音及右侧标签工作区仍未完成，见[设计实施表](./docs/design/alva/DESIGN.md#功能实施状态与契约收口--2026-10-23)。其他平台、完整OS输入/IME/读屏与真实设备仍另验。证据指针`/tmp/alva-product-closure-path`。

## 2026-10-23 输入区浮层与底部对齐修正

`/`与`@`结果浮层优先置于整个输入卡上方6px、左边对齐，空间不足时在整卡下方翻转并限制滚动视口；不再从文字框下沿展开遮住工具栏。保留输入框作为MyGo浮层的焦点/关闭owner，几何变化时仅补定位帧，Tab/Shift+Tab仍连接输入与搜索。添加、权限和模型菜单优先向上打开，打开时关闭输入搜索浮层。

输入卡改为8px对称内距，底部控件统一32px，Send/Stop为32px圆形、18px图标；权限/模型按钮保持对称4/12内距。设计同步既有14个Composer变体及2条交互契约，不新增Token；SmallPen嵌套实例局部padding的限制单独记录，不把静态布局宣称为完整运行行为。

最终原生桌面与architecture两包race共339个测试/子测试通过、0失败，24.69秒，冻结源码执行期间未变；相关vet与正式原生构建通过。原生无窗口几何/交互覆盖明暗、首页/会话、短长输入、上方不足时翻转及Tab顺序，最终960×680、8项附件与5行输入实测浮层间隔6px、发送按钮右/下内距各9px（含1px边框）。独立demo数据的Alva Layout Preview.app已签名启动，PID59581，最终只保留该客户端；两个旧窗口确认无草稿、附件或运行任务后正常退出，原数据目录保留。真实OS窗口视觉/IME与其他平台实机不由无窗口检查替代。证据指针`/tmp/alva-composer-alignment-path`。

## 2026-10-23 输入区引用、命令与只读计划

在上一轮业务校准基础上并发接通三个领域，仍使用Go/MyGo、既有共享产品服务与原始Artifact，不新增SQL schema或执行runtime。

- `@`与“项目文件或目录”读取已存项目的有限metadata页；选择保存文件原件或有界目录manifest，按branch/native/workspace固定，和附件共享8条/40MiB预算。首页不为搜索自动创建会话，目录不是递归正文快照。
- `/`与“命令与Skills”提供内置plan/review及已管理的`.alva/skills`；选择只准备typed草稿，明确Send在共享服务锁内重验来源/SHA和分支后复用原准入。变更、禁用、归档、篡改或跨分支均拒绝并保留稿；兼容来源Skills、任意shell命令不在本轮范围。
- PlanMode由已有JSON设置耐久保存，专用setter保留其他配置与批准，空闲native/checkpoint策略只有一份。实际Guard只读并收敛本轮进程/网络集成；统一SQLite Attempt准入拒绝Plan转外部引擎，覆盖fallback/Worker/自动化。普通配置保存不能默默退出Plan；退出不批准操作。
- 原生输入复用IME、粘贴和多行语义；浮层只响应真实输入或明确操作，异步搜索按作用域/请求身份取消和拒绝迟到结果。捕获typed选择和首发迁移、错误重试及旧回执清理与原有文本/附件链路一致。

最终全仓desktop-tagged race共96个有测试包、2252个测试/子测试通过，0失败，100.12秒；906个Go源码/模块文件执行期间未变。全树desktop vet、go mod verify、正式原生构建和diff空白检查通过。独立审查发现并修正Plan的external fallback/冻结checkpoint入口及准入锁阻塞UI，分别由真实HTTP、临时红例overlay和普通/typed发送延迟回归验证。普通Worker空选择回执也已修复，原有真实队列测试通过。

原生无窗口画面覆盖1240×850、960×680及明暗主题，菜单参数/详情/分页、8项chips和发送移除可见；深层路径已优先显示文件名，并在完整路径tooltip中核对无溢出。新版Alva Composer Preview.app以独立demo目录启动，进程PID8503；仅确认构建/签名/进程启动，真实OS窗口与选择器另验。此前章节的缺口列表按其当时状态保留；本节以上述限定范围更新当前能力。证据指针`/tmp/alva-workspace-capabilities-path`。真实OS窗口/选择器、外部模型账户及其他平台实机未在本轮验收；正在使用的旧预览进程未被重启。

## 2026-10-23 工作台业务行为校准

按Product的home-composer/navigation原生requirement核对，纠正首轮只验布局/少量准入、测试反而锁定临时限制的问题；未把未实现的业务改写成设计无需支持。核心mechanism/SQLite/共享服务不另造副本。

- 首页可先选附件，页面只保存本窗待导入路径。明确Send后Create→可选Switch→逐项Upload→Detail→捕获prompt的Submit。部分失败保留实际session、成功引用、失败/剩余文件及编辑稿，重试不重建或重传成功项；原件不删除。分支/native不兼容提前显示并禁发。
- 提交/导入期间输入仍可编辑；普通首发、已有会话与Worker使用点击时的文本，真实接收回执只清理仍匹配的旧草稿。新输入不被旧回执擦除，不自动发送下一条。
- 本窗按session与不可变物理owner身份保留未完成Stop，切走再回仍Stopping，实际owner替换时清除旧标记、条件取消不影响后续执行（含同Turn恢复），真实终态才结束；Cancel报错也不把尚在清理的owner当成已结束，只允许用户明确重试。运行/等待/完成/失败/已停止与按钮提示按观察事实表达。
- 模型菜单使用真实configured routes、方向键/Enter选取、有限视口滚动和焦点恢复；设置有独立分类/左上返回，分类切换共用原来源。Inspector定位和草稿保留，媒体离开仍释放正文并真实join播放，返回只恢复冷索引。

Ask/Auto/Full的完整单选政策、真实计划模式、新建目标、长期伙伴、@引用搜索、/命令执行、语音录制/转写、模型连接目录与右侧标签工作区仍未实现。当前原生guard固定Ask；SDK AcceptEdits不等于设计Auto，不能从菜单直接授予Full或借工具元数据冒充命令服务。具体行为→owner→用例映射见docs/ARCHITECTURE，使用边界见docs/USAGE。

最终Hub全包、shared/direct UI、桌面入口及architecture的12包race共581个测试/子测试通过、0失败，84.04秒，执行期间Go源码/模块未变；其后仅修正模型选中项文字色，Hub原生及architecture再验316项通过。仅本任务差异的独立源码overlay最终6包196项通过；desktop全树vet、go mod verify、gofmt与diff空白检查通过。独立审查两处停止owner边界均已以实际fixture/覆层反例修正复验。

最终原生无窗口控件及2×画面检查验证待导入/移除保留原件、模型方向键/Enter/Escape、实际Ask/不可用策略、独立设置/返回，并重查选中模型文字可读；没有在此验收中Send或Save。真实原生窗口绑定返回cgWindowNotFound，系统选择器开/取消、可见首页及实际OS焦点未验收。预览旧进程在未确认其内存草稿前被重启，数据库保留但不能确认未发送草稿；最终磁盘binary已更新，当前PID34764仍是最后文字色修正前的运行版，未再次重启。证据指针`/tmp/alva-workspace-business-path`；其before保留开始时原始文件，既有Developer/CLI/设计工作不覆盖、不混入本次提交。

## 2026-10-23 正式工作台与输入区首轮落地

正式SmallPen21/22的220侧栏、52页头、居中限宽首页、建议草稿、多行输入与对话阅读已进入实际Go/MyGo原生产品，不再只在设计包中展示。语义表面色、Alva UI Sans SC400/600及固定Lucide1.61.0资源复用现有设计，包含来源SHA和完整许可；任意文字保留平台字体回退。正文仍为有界虚拟列表/Markdown，direct原输入区共用列表状态。

明确首发才创建SQLite会话、选择实际路由、读初始Detail并Submit；输入、建议和新任务导航不创建或执行。创建/准入失败保留草稿和已创建身份，首读/同会话重载禁发，准入使此前观察失效，旧详情不能回盖已提交运行状态，停止等待真实owner。项目目录有原生明确选择入口；已有会话/插件/等待/Inspector/协作/自动化保留，保存分支与账户/计费fallback从高级菜单查看、批准或禁用，未完成播放保留清理动作。

新UI只列当前装配目标与真实权限政策；完整Provider目录、常驻伙伴/分组、语音转写和部分命令编辑仍待后续，不能因菜单或设计存在而宣称可用。

本批按侧栏/输入区/设计/测试分工并发实现，并经独立代码和视觉审查。真实macOS原生.app已打开并经CUA查看2×画面：明暗首页、选中导航文字、输入启用/清空禁用及上对齐用户气泡无阻断；未在实机发送模型任务。最终相关完整race覆盖Hub原生、shared chat、direct原生、Hub桌面入口及architecture，共315个测试/子测试通过、0失败，27.72秒，执行期间Go源码/模块文件未变。仅本任务差异的源码overlay独立通过4包80项回归，确认不依赖原有Developer/CLI改动；全树desktop vet、go mod verify与本批源码diff空白检查通过；OFL许可原件保留上游一处行尾空格。
密度反馈同步到设计与原生实现：220侧栏、52页头、24外边距、输入区12内边距/8间隔，对话组间隔16，保留阅读字号与两行输入。最终明暗×首页/会话×1000×720及1240×850共8组原生无窗口画面/几何检查通过；重建后的真实2×窗口核对深色首页及草稿启用/清空禁发，未发送或改变设置。浅色与会话的最终紧凑画面由无窗口原生渲染覆盖，不冒充真实OS交互验收。
证据指针`/tmp/alva-workspace-ui-path`指向本机Codex持久检查点，运行预览指针`/tmp/alva-workspace-live-preview-path`指向本仓库.git内隔离预览；使用独立明确demo数据目录。设计密度同步增量修订；原始线稿、稳定身份与既有其他工作树改动保留。

## 2026-10-23 Hub 应用外观与页面业务接线

本批先接通General settings的主题、信息密度与正文字号；完整页面与现有业务owner/存储/缺口的对应关系见[架构映射](./docs/ARCHITECTURE.md#hub-页面与业务落地)。SDK、harness、会话设置与Runtime六方法依赖未变，桌面仍使用固定MyGo。

- `hub/preferences.go`是唯一允许值、默认值、patch合并及revision政策；独立PreferencesStore贯穿共享服务与程序选定的组件。不会为UI、HTTP、SQLite各写一份业务规则，不向会话SettingsStore塞应用偏好。
- PreferencesService串行真实读写，发布非阻塞的已提交快照；原生窗口和HTTP共用一个owner。读写失败、无效/旧后端快照不覆盖已保存状态；显式Close关闭准入、取消并等待真实I/O，超时保留owner和数据库供重试。迟到成功按事实发布，不重放写入。
- 产品SQLite以事务从v1升级到v2，只新增`hub_preferences`单行表；新建/迁移才写默认值。CAS写入全成功或全不变，旧会话/分支/native身份/历史保留，独立native库不变。未知版本、缺行/损坏或迁移故障明确失败，不换库或重置默认。
- 原生General页无需聊天会话，实际Select只编辑本窗草稿；明确Save才应用到所有窗口，Reload重新读取。冲突、失败及导航保留草稿，实际保存返回前阻止关窗；系统深浅色、文字缩放、高对比及减少动态效果继续有效。语言与应用级动态效果覆盖未实现，未提供假开关。
- 认证`GET/PATCH /v1/preferences`复用共享owner，严格JSON与现有Host/Origin/Bearer校验；无效400、冲突409、故障脱敏500。页面打开、读取/保存不创建聊天或调用模型。使用与请求格式见[USAGE](./docs/USAGE.md#hub-通用外观设置)。

原生控件、SQLite迁移/回滚/竞争、共享快照与关闭等待、HTTP输入/故障及opaque选型已通过专项race回归和独立代码审查。仅本任务差异的隔离源码overlay亦通过6包58项偏好测试，确认不依赖工作树中原有开发者选项改动。
本机最终验证（macOS arm64 / Go1.27.2）：整仓desktop-tagged race共96个有测试包、2169个测试/子测试通过，0失败，889.23秒；869个Go源码/模块文件执行期间未变。专用子进程helper的无参数入口按设计skip，其真实进程用例已执行。全树desktop vet、go mod verify、本批gofmt与diff空白检查通过，独立审查无遗留问题。
证据指针`/tmp/alva-hub-preferences-path`保存完整race JSONL/冻结hash、静态检查及仅本批差异的源码overlay；SDK/extension/harness/platform及根模块文件相对HEAD未改动。真实OS窗口/AX/IME、其他平台实机与外部模型账户仍未在本批验收。
首轮签名提交曾因仓库配置的1Password返回`failed to fill whole buffer`而失败；本批35文件的精确差异已单独核对，原有工作树改动保留。

模型连接目录、Provider/模型增删改查与常驻伙伴/分组仍待后续实现；既有会话/权限/历史/Team/自动化/工作树继续复用当前共享服务，不以SmallPen页面存在作为已接通证明。

## 2026-10-21 产品开发者选项：独立测试与模型评估

按产品职责接入Hub原生侧栏Developer options及同一个CLI的`alva developer test/eval`；共享实现位于apps/developer。
SDK内核、Plugin/Registrar、harness、共享存储插口与QuickJS未改，没有新增独立产品程序。

- test使用真实interaction声明做五类单响应工具选择/参数检查，支持同条件工具名对照，只输出通过/失败；eval使用独立版本化任务集、exact_text/exact_json与明确权重/重复trial，输出加权任务分、所有trial通过的任务数、覆盖/错误、端到端耗时/首输出与实际usage。测试失败exit1，评估低分正常出报告，执行故障/取消仍失败。
- 数据集ID/版本/含期望和权重的完整SHA与每请求SHA共同归因。API错误不混进质量分母，未执行无分；小型基础任务可被显式自有数据集替换，不当作通用模型排行榜。
- UI无需聊天会话即可选择明确模型route、预算、测试名称或评估任务集；预览不请求，修改后重预览，明确Run才调用。跨窗口共享唯一Developer job owner与最新结果；关闭普通observer不Stop，最后窗口有任务时先Stop。Application.Close取消并join真实请求，超时保留owner可重试。
- 默认live绑定当前配置模型，程序可显式注入额外DeveloperRoutes；不借外部Agent CLI的账户/订阅。CLI严格显式route/秘密、默认冷计划，完整矩阵/全部凭据先校验；无发现/重试/fallback。不会创建聊天session、发送聊天历史或执行广告工具；最新报告仅进程内保留。
- 初轮回归后自查修正一处测试system提示词与天气工具调用要求的冲突，并统一CLI/UI工具名称子集校验，避免测例设计或端间校验差异被记作模型能力问题。

本机验证（macOS arm64 / Go1.27.2，无真实付费模型或外部账户）：

| 检查 | 实际结果与范围 |
|---|---|
| 相关完整竞态回归 | apps/developer、全部Hub与cmd、全部LLM集成和tests，共22个有测试包、869个测试/子测试通过，0失败，329.07秒；1071个源码/文档/配置文件执行期间未变。3个专用子进程helper无参数入口按设计skip，实际进程用例执行 |
| 最终测例/校验修正复验 | 上轮后仅5个apps/developer文件变化；最终developer、Hub bootstrap/native UI、完整CLI及architecture共5包、393个测试/子测试通过，0失败，216.60秒；1071个文件执行期间未变。2个无参数helper按设计skip，真实跨进程用例执行 |
| 原生控件/owner | 1240×850实际MyGo无窗口控件完成无会话打开→冷预览→编辑使预览失效→测试→独立评估→另一窗口观察/Stop；真实Application.Close取消实际HTTP且不创建聊天Turn。故意不响应取消的transport在Close超时后仍由owner持有，实际退出后再完成Close |
| 实际CLI二进制 | 新二进制仅LANG环境、无PATH/HOME/模型凭据：两种冷计划0调用，名称矩阵15次本地请求全部通过；独立4trial评估得75分并exit0；SIGINT再1次请求返回部分报告且真实socket关闭。合计20次local HTTP，不是真实模型成绩 |
| 跨目标构建与静态 | cgo-free CLI和Hub桌面在darwin/arm64、linux/amd64、Windows amd64/arm64共8组构建通过；全树desktop vet、go mod verify、gofmt与diff空白检查通过。CLI依赖图无Hub/engine/MyGo |
| 保护文件 | SDK、extension、harness、platform、公开快照与根模块共545个文件保持本批开始时原字节；暂存区为空 |

证据指针`/tmp/alva-developer-options-path`保留两次完整JSONL/冻结hash、精确5文件修正差异、owner/控件与实际二进制、构建/静态回执。
最终复验之后只补文档记录，不以首轮范围冒充最终全树再次执行。普通CI仅本地fixture；真实模型需从产品入口显式运行。
当前测试与评估均为single_response：尚不覆盖多轮Agent任务评分、Other回填/真实权限/持久恢复的模型评估；这些功能的实现回归仍归所属组件。
尚无持久评测历史、真实模型默认成绩或原生OS视觉/IME验收；无窗口控件和跨编译不替代对应平台实际操作。

## 2026-10-21 CLI 选项、Other 与独立权限审批

补齐L6 direct CLI问答装配及终端输入；复用既有interaction Tool/native Plugin、durable Controller与权限管理器。SDK、扩展、harness、存储插口和执行模型未改。

- 普通`--repl`调用ask_human时原地等待，同一轮获得真实答案后继续；单选/多选、无选项自由文本和额外Other均支持。Other独占下一行，返回text，不伪造新的协议选项；空、越界、重复或错误数量不提交，不预选答案。
- 问答与审批共用一个可取消sole-reader gate，覆盖Other第二行；沿用32KiB/行、1024行/1MiB预算。权限只接受当前请求的`allow ID`单次批准，deny/其他输入拒绝；问答文字、Other或旧ID不授权，Plan/accept_edits政策保持。取消后真实join，EOF不虚构回答，终端控制字符只在显示边界转义。
- 有native会话时使用既有审计/同会话引用；无数据目录使用普通Tool/进程内引用。durable同时接入question handler，保存等待后退出，冷Resolve只保存选择或text，显式恢复后遇到写入仍独立审批。只有明确交互/durable才广告问答工具。

本机验证（macOS arm64 / Go1.27.2，无付费模型或外部账户）：

| 检查 | 实际结果与范围 |
|---|---|
| 完整CLI与相关竞态回归 | cmd/alva、全部tests、interaction及durable共8包、366个测试/子测试通过，0失败，178.32秒；1057个源码/文档/配置文件在执行期间未变。3个专用子进程helper无参数入口按设计skip，实际进程恢复用例均执行 |
| 实际进程与二进制 | 跨进程问题/Other保存与恢复、回答后独立approval通过；新CLI二进制在无PATH/HOME/模型凭据环境，以真实终端pipe完成三选项→Other→显式单次写审批→同轮完成→退出，4次本地模型请求且文件实际写入一次 |
| 平台构建与静态 | CGO_ENABLED=0，CLI在darwin/arm64、linux/amd64和Windows amd64/arm64构建通过；CLI desktop-tagged vet、go mod verify、gofmt、diff空白与架构/分形闸通过 |
| 保护文件 | SDK、extension、harness、公开快照、根模块与固定QuickJS资产共357个文件保持本批开始时原字节 |

证据指针`/tmp/alva-cli-questions-path`保留完整JSONL、冻结hash、实际终端会话与二进制/构建回执。此批按CLI及其真实消费者范围验证，未重复全树测试；跨编译不等于对应OS运行。整Agent WASM REPL/问答和Windows交互console仍未实现，Windows现有file/pipe路径保留。

## 2026-10-21 可选项目规范兼容与同名去重

新增L4 `extension/compatibility`，显式配置后才装配；CLI、Hub共享bootstrap、原生Skills面板和认证HTTP均接入同一个插件。SDK、Plugin/Registrar与存储插口未修改；不增加另一套执行模型。

- Agent Skills标准与Alva原生Skill各用明确格式解析，合到既有渐进目录和一个skill工具。项目规则仅加载workspace到显式scope的祖先链；Claude基础Agent Markdown转为现有子代理模板，正文只给实际子代理，工具仍受直接父真实能力和独立审批约束。
- 去重身份为类型+name；同路径/同格式来源只读一次。原生数组与标准字符串先归一；完整元数据、调用方式、工具名集合、正文及Skill资源路径/媒体类型/字节SHA一致才合并，native仓库优先，其余用显式来源顺序。差异报告双方来源，不合并权限；不同名称和不同目录作用域的规则分别保留。
- 本轮规则、Skill文档和Agent模板冻结，来源有界、目录pinned、拒绝symlink/逃逸；Agent与真实子任务join后关闭。CLI与Hub等待配方绑定指令指纹，变更在模型/外部插件启动前拒绝恢复；资源仍按需读取当前数据，不执行、不自动送模型。
- 默认关闭，不发现HOME/旧配置，不展开import/include。厂商Hook、条件frontmatter、permissionMode、独立model等执行政策拒绝；支持范围逐项列在[使用说明](./docs/USAGE.md#可选项目规范兼容插件)。整Agent WASIp1 CLI暂不装配这个宿主文件插件，不影响可移植SDK。

本机验证（macOS arm64 / Go1.27.2，无付费模型或外部账户调用）：

| 检查 | 实际结果与范围 |
|---|---|
| 整仓desktop-tagged race | 去重修正前冻结版本：94个有测试包、2058个测试/子测试通过，0失败，836.13秒；1053个源码/文档/配置文件执行期间未变。4个专用子进程helper无参数入口按设计skip，实际进程用例均执行 |
| 最终去重复验 | 完整Skill协议/兼容插件/Skill扩展/architecture及CLI、Hub bootstrap/原生控件/HTTP/SQLite兼容用例，9包共269个测试/子测试通过，0失败；1055个文件执行期间未变。覆盖副本合并、资源冲突、实际Skill调用/独立子代理和变更指令拒绝等待恢复 |
| 独立CLI | 新构建二进制在空PATH、不存在HOME且无模型凭据环境下，help/冷检查/demo实际成功；原生+标准同名副本仅一个Skill目录项。依赖图无Hub、EngineRuntime或MyGo |
| 跨目标构建 | CGO_ENABLED=0，CLI在darwin/arm64、linux/amd64、Windows amd64/arm64构建通过；不冒充对应OS实机执行 |
| 静态与保护文件 | 全树desktop vet、go mod verify、gofmt与diff空白检查通过；包含此前Graph修正的SDK、现有公开快照、根模块文件和固定QuickJS资产共166个保护文件保持本批开始时原字节 |

整仓与最终去重验证分别记录，不以专项结果冒充最终版本再次全树运行。证据指针`/tmp/alva-compat-path`保留完整JSONL、冻结hash、保护文件比对、实际二进制和跨目标构建回执。原生控件使用MyGo无窗口验收，真实OS视觉/IME、Windows实机、外部账户和远程CI仍按各自状态记录。

## 2026-10-21 SDK 欠账收口：Graph 取消与执行预算

本批从底层欠账回查，修改限定在L3 Graph内部实现、回归与CI接线。L0–L2.5内核、Plugin/Registrar、共享存储插口、根模块锁定和固定QuickJS资产未修改；Graph公开类型/签名的`go doc -all`输出与修改前逐字节一致。

- 真实编译图的clone、node、merge、router和checkpoint边界重查取消。回调取消后不再启动后续节点、路由或外部checkpoint；已开始节点仍等实际退出。保留实际节点/路由失败及取消原因，重试等待取消也不吞前次节点错误；完成事件回调取消不能返回成功。
- 仅实际取消才合并取消原因；无取消的router、merge、checkpoint load保留原始错误身份，显式Retryable仍收到节点原始错误，支持调用方原有判定。对应回归也在原实现上执行通过，避免为修复取消引入兼容退化。零延迟及负初始延迟对任意attempt均常量时间；已取消的零延迟Wait、空TransformPipeline不能成功。
- Resume拒绝负checkpoint step，执行步数先比较有限预算再递增，外部checkpoint的最大整数不能溢出后继续执行。Compaction从剩余预算扣减，负数或溢出估计保守要求压缩；关闭时不调用用户counter，恰好等于预算仍允许。
- CI原有Node和WASIp1实际执行步骤加入Graph，不只跨目标构建。新增回归实际执行图、外部checkpoint记录器和模型消息，未增加另一套runtime、默认重试或自动副作用重放。

归档源码对照保留必要差异：Go不沿袭旧并行嵌套Send丢任务、按完成顺序合并和checkpoint保存失败后继续执行的行为。Rust的GraphRun部分配置仅被保存、没有执行入口，不能把这类scaffold当成已经实现的功能。归档测试索引已排除文档示例，但索引和测试数量均不代表逐项语义对等完成。

本机验证（macOS arm64 / Go1.27.2 / Node24.19.0，无付费模型或外部账户调用）：

| 检查 | 实际结果与范围 |
|---|---|
| 整仓desktop-tagged race | 93个有测试包、2039个测试/子测试通过，0失败，764.46秒；1037个源码/文档/配置文件在执行期间未变。4个专用子进程helper无参数入口按设计skip，其实际用例均执行；另9包无独立测试 |
| 最终SDK与消费者复验 | 自查补充原始错误身份兼容及专门回归后，全部SDK、extensibility和architecture共15个有测试包、623个测试/子测试通过，0失败，33.14秒；1037个文件在复验期间未变。实际独立SDK消费者与Graph编排Agent的用例包含在内 |
| 最终Graph跨运行环境 | Node的JS/WASM与wazero的WASIp1各31个测试/子测试实际通过，均0 skip；完整SDK与codec在两个目标构建通过 |
| 原生跨目标构建 | CGO_ENABLED=0，完整SDK在Linux amd64、Windows amd64/arm64构建通过；Graph测试二进制在Windows两架构编译通过，不冒充对应OS执行 |
| 静态与范围 | 全树desktop vet、go mod verify、gofmt、diff空白检查与workflow YAML解析通过；127个其他既有SDK Go文件、全部现有公开面快照及其地图、根模块文件和固定QuickJS资产共144个保护文件保持原字节 |

整仓结果对应最后一次错误身份兼容修正之前的冻结版本；最终复验覆盖所有SDK与实际Graph消费者，其他产品源码在两次验证间未变化。原CLI暂存区保留，Hub工作树改动未回退。证据指针`/tmp/alva-lower-debt-path`保留失败回归、最终完整JSONL、两次冻结hash、公开输出比对和跨目标执行/构建回执。

仍未销账：Windows交互console、pinned-directory host shell和worktree模式/ACL策略的实现与实机验收；全量Rust fixture逐项语义核对；完整原生视觉/AX/IME/音频及外部账户验收。原生窗口控制通道本轮仍报`Sky Computer Use native pipe startup failed`。仓库既定SSH身份的远程读取因代理`communication with agent failed`被拒绝；最终验证后重试CLI签名提交仍报1Password `failed to fill whole buffer`，未产生新commit，暂存区保留。当前新代码没有远程CI通过结果，不以旧失败运行或本机跨编译替代。

## 2026-10-21 Hub 持久问答原生控件

持久问题直接消费既有interaction.Question/Answer与ResolveWait；新增仅为L7呈现，不增加SDK、Plugin、等待协议或存储接口。

- 单选、多选和自由文本均无默认回答，明确保存才提交；选择和文本互斥，按JSON实际编码字节检查16KiB上限，过期请求禁用回答及审批。external仍使用JSON输入。
- 保存后仍暂停，已保存回答关闭重开后只读展示；显式继续仍用原Turn。真实本地SSE、共享存储和bootstrap关闭/重建回归证明：回答不会授权后续write_file，续跑仍产生独立approval且文件未写入。
- Refresh/Continue固定于面板顶部，完整请求/已存结果/放弃审阅位于滚动区；1240×818无窗口控件按实际边界滚动后点击。每个问题以请求ID隔离控件容器，另一问题消失后剩余输入的焦点与草稿保留，避免重复控件key共享状态。
- 最终desktop/命令/architecture三包竞态测试274个测试及子测试通过、0失败，19.07秒；vet、desktop+smoke测试、mod verify及darwin/arm64、linux/amd64、Windows amd64/arm64构建通过。1036个源码/文档/配置文件在最终验证期间未变。
- 证据指针`/tmp/alva-hub-wait-questions-path`保留原问题的失败回归、并存问题key冲突反例、最终完整JSONL/冻结hash/构建回执及差异清单。原CLI暂存区保持原字节；SDK、公开快照、模块文件与其他产品源码未修改。

本批未运行真实OS窗口点击、完整画面/IME/声学或外部付费账户验收；无窗口控件与跨平台构建分别报告，不替代这些未完成项。

## 2026-10-21 Hub 等待面板跨窗口观察

本批仅修改L7桌面呈现及其回归，不变更SDK、等待协议或执行owner。

- 等待面板复用原250ms观察时钟读取已提交checkpoint；其他窗口保存结果、继续或放弃后更新显示，不自动执行。完整已保存结果可审阅，Turn ID提供只读文本字段。
- 未提交回答只保留给同branch/Turn、同请求ID与binding的未解决请求；checkpoint revision变化清除放弃确认。明确操作或刷新后的旧读不能回盖新checkpoint；读失败禁用操作，成功读后恢复。
- 1240×850 MyGo无窗口回归覆盖两个独立页面的保存观察、草稿保留/清理、读取故障时Continue禁用、延迟旧读、新Turn及放弃观察。桌面/命令/架构3包竞态测试共268个测试及子测试通过、0失败；vet、desktop+smoke测试、darwin/arm64、linux/amd64及Windows amd64/arm64构建通过。
- 验证期间1034个源码/文档/配置文件未变，原CLI暂存区逐字节保留。证据指针`/tmp/alva-hub-desktop-flow-path`包含完整JSONL、冻结hash、构建回执及本批差异清单；此前整仓测试仍按其原验收版本记录，不冒充本批再次整仓运行。
- 实际临时Hub窗口的本地demo及Inspector导出通过，但当前Build截图仍为frame_pending；系统窗口控制通道报`Sky Computer Use native pipe startup failed`，辅助只读AX观察也未得到可用窗口控件树。真实点击、完整画面、IME和音频验收仍未完成；临时进程均已回收，未更改系统权限。


## 2026-10-21 Hub 同Turn持久等待接线

沿用harness/durable和原生共享journal；SDK内核、Plugin/Registrar和ExecutionStore六方法不变。外层Engine明确区分本次执行结束与任务完成，Hub UI/HTTP共享一个产品服务和原有Runtime owner。

- Alva仅实际option-aware factory声明`durable_waits`；`Completed(waiting)`必须有真实SessionStarted及有界revision/请求数/到期回执，且实际Run和资源Close先完成。取消、fatal/invalid回执及收尾失败不得伪报waiting。
- 持久开关默认off；原生面板和认证HTTP提供冷列表、完整有界请求审阅、revision/binding结果提交、显式同Turn继续和检查事实后的Abandon。保存结果不执行，不接受native ID/host path/新resume prompt。UI暂停Send/设置但不伪造active执行，草稿保留。
- 同Turn继续建立新Attempt，保留native身份、Turn序号/原输入和Worker Task/Automation归属；无新handoff/附件，continuation禁止自动fallback。新输入/route/工具插件设置在waiting时冻结；队列和timer不消耗下一轮。
- 配方hash绑定canonical workspace、route、已存设置和显式process配置；变更拒绝恢复。factory先认领实际native lease再加载插件；真实进程/VM/回调完成后才放claim，失败收尾仍由Application持有并可重试。
- crash edge保留已提交waiting回执；真实续跑进程在授权写入后被杀，重开为interrupted且保留native running检查点，不自动请求模型或改写文件。过期/陈旧/跨branch决定拒绝；Abandon留审阅事实，不宣称撤销副作用或外部取消。

本机验证（macOS arm64 / Go1.27.2，无付费模型或外部账户）：

| 检查 | 实际结果与范围 |
|---|---|
| 整仓desktop-tagged race | 93个有测试包、2018个测试/子测试通过，0失败，794.48秒；871个源码/配置文件在执行期间未变。4个无参数跨进程helper按设计skip，其实际用例均执行；另9包无独立测试 |
| 最终产品边界复验 | 整仓验收后仅修改product/wait.go及service_test.go：其他engine声明等待能力也不能借Alva journal。Hub、三个Hub命令与engine适配14包/308个测试及子测试通过，104.35秒；按反向依赖又补验MCP入口1包/4项，3.96秒。合计15包/312项、0失败；最终871个源码/配置文件在各次复验期间未变 |
| 静态与保护文件 | 231项architecture、全树desktop vet、go mod verify、gofmt、工作树/暂存区diff检查和workflow YAML解析通过；129个SDK Go源码、14份现有公开快照、go.mod/go.sum及固定QuickJS资产共146个保护文件保持原字节 |
| 跨平台构建 | cgo-free、desktop-tagged全部8个命令在macOS arm64、Linux amd64、Windows amd64/arm64通过；受影响的5个Hub/adapter测试包在Windows两架构编译通过；WASIp1 Worker构建通过，共15组构建/编译检查 |
| 实际Hub Server可执行程序 | 新构建二进制真实开启loopback认证服务→显式启用→挂起→正常退出→重开→只保存结果→显式同Turn继续→正常退出；两次本地模型请求、一次原始产品输入，结果实际交付一次。不是仅调用HTTP handler或内存模型 |

证据指针`/tmp/alva-hub-waits-path`保留整仓与最终Hub完整JSONL、两组冻结hash及精确两文件差异、静态/构建/实际二进制回执。MyGo无窗口控件通过不等同真实OS窗口视觉验收，跨编译不等同Windows实机执行；本地结果不冒充远程CI。签名提交仍因1Password `failed to fill whole buffer`失败，未更换签名/发布身份，原CLI暂存区与本批Hub工作树改动均保留。


## 2026-10-21 Direct CLI 插件与持久等待产品接线

复用既有AEP loader与durable Controller；修改限于CLI装配/资源所有权、测试、CI与文档。SDK的129个Go源码、现有公开快照和根模块文件保持原字节。

- `--inspect-plugins/--plugins/--enable-plugin/--disable-plugin`提供显式冷控制；配置文件严格JSON、原生解释器显式、WASIp1禁用解释器/args/env。检查保存disabled code/review pin，批准包括代码和启动设置；源文件/代码变化拒绝运行。原始环境/参数不写入设置历史。
- Native CLI复用一个进程owner/会话，每轮新建Agent与外部插件，持久memory或进程内默认memory/引用服务保留，第二轮可检索第一轮保存而原文件已删除的易失引用。实际会话claim先于插件启动；部分初始化与失败快照均保留owner。先join进程/VM/回调并写入真实资源收尾事实，再释放claim和关闭存储；未完成通过可继续Close的错误owner保留。
- `--durable`显式接入持久审批与await_external；挂起输出waiting回执并退出REPL，不经普通host.Controller伪报完成。`--waits/--resolve-wait/--resume-wait/--abandon-wait`复用同一个日志插口，结果绑定ID/payload、abandon绑定明确revision（含0）。普通运行和整Agent WASM也不能绕过未处理等待或未知执行。
- 恢复绑定模型route、canonical工作区、权限、memory和启用插件配方；未解决等待不启动模型/插件。Resolve不执行，Resume不给新prompt/附件，原子交付结果后让模型继续，精确审批只消费一次，不自动重放工具。
- 单独持久化插件启动意图/物理收尾，覆盖Agent循环开始前的崩溃窗口。未收尾启动由waits明确显示plugin_startup_unknown，后续执行拒绝；检查后按revision解除只是承认未知事实，不宣称撤销副作用。恢复的下一turn会重新启动已批准插件，原生启动风险仍需由使用者审查。
- 新回归实际启动Node和独立WASIp1模块，覆盖冷操作无执行/篡改/禁用、两轮新peer、秘密不进facts、真实CLI进程退出重开、精确审批后工具一次、进入工具后取消与真实清理、未完成收尾不释放storage和启动未知状态；Windows CI增加相同CLI流程。

本机验证（macOS arm64 / Go1.27.2，无付费模型或外部服务）：

| 检查 | 结果与范围 |
|---|---|
| 整仓desktop-tagged race | 93个有测试包、1983个测试/子测试通过，0失败，805.91秒；858个源码/配置文件在执行期间未变。3个无参数跨进程helper按设计skip，其实际用例均执行；9包无独立测试 |
| 最终CLI修正与完整复验 | 整仓验收后，跨轮易失引用用例实际复现reference unavailable；修正仅涉及CLI owner/test及Windows CI selector。最终完整CLI race 43个测试/子测试通过，0失败，140.35秒；源码在复验期间未变。实际删除源文件后仍可在第二个Agent读取第一轮引用；重新检查清除批准、配置字节篡改也已覆盖 |
| 最终静态与接口 | 231项architecture测试、全树desktop vet、go mod verify、gofmt、diff空白检查和workflow YAML解析通过；129个SDK源码、全部现有公开快照、根模块文件和固定QuickJS WASM资产保持原字节 |
| 最终跨平台构建 | cgo-free desktop-tagged全部命令在macOS arm64、Linux amd64、Windows amd64/arm64通过；WASIp1 Worker通过；CLI测试二进制在Windows两架构编译通过。本机跨编译不冒充Windows实机执行 |

证据指针`/tmp/alva-cli-workflow-path`保留整仓与最终CLI各自完整JSONL、冻结hash、精确源码差异和构建回执。最终修正只在不可被其他包导入的CLI入口，不以重复运行未变动包代替针对性复验。这批CLI验收不覆盖Hub等待UI/HTTP；后续Hub接线见上方独立记录，不把CLI闭环当作所有产品已完成。远程CI尚无本批运行结果，SSH签名代理不可用时不切换发布身份。


## 2026-10-20 JavaScript作者SDK与独立WASIp1插件补齐

按用户“补齐”要求，补上JS作者入口/真实互操作和独立`.wasm`加载。继续保留既有Tool/Plugin/Middleware、共享存储插口与SDK依赖方向；129个SDK Go文件、根go.mod/go.sum和既有SDK公开快照未改。

- `integration/protocol/aep/javascript`提供本地零npm运行时依赖的Node20+作者SDK、TypeScript声明、七类hook/action和完整echo bundle；异步反向回调不死锁，事件handle到期后不可复用，帧/队列总字节/请求/回调有硬限。默认消息页匹配实际宿主上限，工具RPC与hook result各用本来正确的协议字段。包尚未发布npm。
- `platform/sandbox/plugin`只依赖标准库/wazero，执行独立WASIp1 command模块，fresh实例、实际memory/双向stdio/总期限限制，无preopen/env/network/custom host import。不把浏览器js/wasm冒充此ABI；此入口使用时间预算，未新增instruction fuel。
- AEP协议增加owned `Peer`与显式`Config.Start`，native Process和embedded peer二选一，不重连、不fallback。初始化/关闭失败时，非nil返回值保留未完成owner供Close重试；物理CleanupComplete与保留错误分开。loader统一审批/实际Tool校验/引用/journal与七钩子，未新增内核runtime枚举或Plugin系统。
- Hub产品验证manifest/runtime和显式解释器绑定；WASM要求interpreter/args/env全空。检查与批准不执行；下个Turn加载，pin变化拒绝。bootstrap保留部分初始化owner；UI说明原生与WASM真实授权差异。
- `examples/aep-wasm`是独立go.mod、标准库协议作者示例，不链接host/Agent runtime；实际测试编译后通过同一loader和Hub装配。JS/WASM工具成功/失败、effective args/模型/工具结果修改、进入后取消并join、不可继承环境/宿主文件均有真实回归。
- 新增AEP/WASM peer公开面快照与leaf依赖反例；CI显式执行Node SDK测试及JS/WASIp1真实验收，Node缺失不得伪装全通过。普通Go构建不要求Node/Python/外部WASM runner；QuickJS-NG继续服务既有受限脚本能力。

本批验证（macOS arm64 / Go1.27.2 / Node24.19.0，无真实模型或云账户调用）：

| 检查 | 结果与范围 |
|---|---|
| 整仓desktop-tagged race | 93个有测试包、1973个测试/子测试通过，0失败，882.94秒；853个受验收Go/JS/声明/脚本/模块/协议配置文件在执行期间未变。两个无参数跨进程helper按设计skip，其实际用例均执行；9包无独立测试文件 |
| JS作者SDK | Node内置test及package test命令均7项通过，0 skip；反向回调、过期状态、工具错误、取消join、帧/总队列字节与重复ID均实际执行。TypeScript声明已提供，未额外安装TS编译器 |
| 实际插件/Hub | JS/WASIp1实际model/tool修改、远程工具成功/失败、进入后取消、真实执行/快照收尾、宿主环境/文件不可继承；Hub检查/批准不执行、下一Turn实际metadata回调与shutdown证据均纳入完整race |
| Go portable实际执行 | Node与WASIp1各7个测试包、42个测试/子测试通过；原生目录用例明确skip。plan/question/schema消费者及独立SDK消费者run/cancel两种模式在两目标均实际运行成功 |
| 跨平台构建 | cgo-free desktop-tagged全部命令在macOS arm64、Linux amd64、Windows amd64/arm64通过；WASIp1 Worker通过。新增插件涉及的6包测试二进制在Windows两架构全部编译通过，不等于实机执行 |
| 静态/接口/文档 | 全树desktop vet、go mod verify、gofmt与diff空白检查通过，workflow经Ruby YAML解析；依赖/公开快照/分形闸包含在完整race。129个SDK源码、11份既有公开快照、根模块文件与固定QuickJS资产均为原字节 |

证据指针`/tmp/alva-aep-completion-path`保存完整JSONL、冻结源码hash和跨目标执行/构建回执。本机验证不冒充远程CI、Windows原生运行、其他语言WASIp1工具链或真实OS窗口验收。


## 2026-10-20 SDK外部接入、存储契约验收与CI平台修正

本批继续保留既有内核/Tool/Plugin/Middleware与共享存储插口，SDK的129个既有Go源码保持原字节。

- 对照实际主分支[CI运行39431189813](https://github.com/QuincySx/Alva/actions/runs/39431189813)：Ubuntu成功；Windows在DACL继承owner假设、路径别名、SQLite URI和匿名pipe deadline处失败；macOS完整race尚未结束即触及20分钟job上限。旧运行的失败不因本地构建通过而改写。
- Windows原始路径先于Abs检查，拒绝会被规范化抹掉的尾随点/空格、设备/UNC和drive-relative别名；不创建被拒别名的规范目标。
- 保留严格current-user owner检查。继承DACL不等于继承owner，管理员token可有不同默认owner；测试分别验证继承的user-only ACE与严格owner准入，不把Administrators放进生产允许集合。
- SQLite使用escaped opaque file URI，固定驱动的实际VFS收到原始native filename，不把盘符当authority或额外加前导slash；真实驱动probe在每种OS验证Windows路径，实际磁盘测试覆盖空格/#/%/Unicode及Unix合法反斜线/问号路径并重开读取。
- 匿名pipe测试不要求Windows不支持的SetWriteDeadline，仍验证1MiB实际drain、保持producer打开时取消、借用文件复用和后续字节；失败路径关闭并join全部fixture worker。
- Linux/macOS job预算45分钟、Windows25分钟、每测试包6分钟；保留完整race、-p1、vet与后续portable gates，未通过删除测试解决超时。
- examples/sdk-agent是独立module，只用公开SDK。程序注入模型/日志/服务，Plugin注册Tool与BeforeModel；同源码在native/Node/WASIp1实际执行正常model→tool→model和工具内取消。模块身份、依赖图、已提交输出与取消outcome均验证；空GOMODCACHE、GOPROXY=off也能列出完整依赖图，无第三方实现包。
- 公开storagetest.RunRepository/RunBindings及blobtest.Run供外部实现复用fresh factory。共同验收owner/authority、原子CAS+交付、固定高水位、有界历史/引用、执行claim、父子因果、身份竞争绑定和不可变竞争发布；复用既有SDK sessiontest，不增加运行时层或云backend。
- SQLite经opaque wrapper、bindings、memory/directory blob运行同一套语义；持久性/损坏/中途故障/跨进程锁/Close重试仍由具体adapter验收。架构闸禁止生产直接或传递导入testing/这些helper，新增两个公共测试入口快照，现有SDK公开契约不变。

本批验证（macOS arm64 / Go1.27.2，无真实模型/云账户调用）：

| 检查 | 本批结果与范围 |
|---|---|
| 整仓desktop-tagged race | 92个有测试包、1932个测试/子测试通过，0失败，674.98秒；运行期间800个Go源码/模块文件无变化。两个无参数跨进程helper按设计skip，真实恢复/崩溃用例均执行；另9包无独立测试文件 |
| 外部SDK消费者 | 独立module身份/公开依赖图及native正常/工具内取消通过；同一源码Node/WASIp1各两种模式均实际成功。空模块缓存、关闭proxy/sumdb的依赖图检查成功，仅六个公开SDK包与消费者 |
| 存储契约验收 | opaque Repository和bindings共17个测试/子测试通过；memory/directory共用公开blobtest，竞争CAS/绑定、原子投递、有界读取与越权拒绝均通过；整仓race已包含全部用例 |
| portable实际执行 | JS/WASM与WASIp1各7个测试包、42个测试/子测试通过；原生目录用例明确skip，3包仅构建。原plan/question/schema消费者与新SDK消费者均实际运行 |
| 跨平台构建 | CGO_ENABLED=0、desktop tag全部命令与storage/privatefs在macOS arm64、Linux amd64、Windows amd64/arm64通过；WASIp1 Worker通过。Windows两架构各5个相关测试二进制编译通过；不等于Windows实机通过 |
| 静态/文档/接口 | 全树desktop vet、go mod verify、gofmt、diff空白、workflow YAML解析通过；分层/公开快照/分形闸包含在整仓测试中。新公开面仅两个测试helper包，129个SDK Go文件和根模块文件原字节保留 |

证据指针`/tmp/alva-sdk-acceptance-path`包含完整JSONL、源码hash、实际WASM回执和各目标构建结果。
本批本地验收不包含新改动的Windows实机/远程CI复验；前述旧主分支运行仍按失败/超时记录。外部module的v0.0.0+相对replace仅是当前checkout接入，不声称已发布版本。


## 2026-10-20 统一存储插口贯穿修正

用户明确要求：不是增加内存/云盘实现，而是从内到外只依赖同一插口，由程序/组件/插件装配决定放入什么。
本批修正此前只抽artifact字节而消费者仍依赖具体Repository的缺口。

- platform/storage根包集中共享端口、Session/ConversationTail接口、DTO与错误；原生provider实现它，消费者只组合所需能力。
- harness/各类证据插件/Inspector/direct/Hub/界面全部移除具体会话类型；SessionPlugins/SessionViews和journal等待适配不选择数据库。
- Direct必传Storage；Hub必传StoresFactory，Memory/Bindings按启用需求显式供应。stock Local配方只在外层选择，缺失/失败无fallback。
- 外部process adapter不再自己创建身份数据库，改接BindingStore；原engine.sqlite实现只迁移归属，不变table/数据。
- Hub required native路由resolver成为显式消费契约；缺失对象使用hub.ErrNotFound，不让Tool/HTTP识别sql.ErrNoRows。
- 共享脱敏脱离具体session包；私有路径操作归system/privatefs，避免创建目录传递引入SQL。
- 构造部分资源、Close失败与实际join后存储关闭继续保留exact owner；尚未清理完的Open返回Application及error供重试。
- Direct与Hub实际执行测试隐藏具体provider/session/reference类型；外部引擎fixture同样只交接口。架构闸拒绝具体provider/SQL泄漏与同包绕过stock配方。
- SDK契约没有因存储替换扩大；新增共享storage/blob公开快照，不增加云后端或隐式迁移。

本批最终验证（macOS arm64 / Go 1.27.2，无真实模型、外部engine账号或云账号调用）：

| 检查 | 本批结果与范围 |
|---|---|
| 完整desktop-tagged race | 91个有测试包、1902个测试/子测试通过，0失败，794.05秒；运行期间Go源码和模块文件无变化。2个无参数子进程helper按设计skip，实际恢复/崩溃用例均执行；另8包无独立测试文件 |
| 接口贯穿验收 | Direct/Hub全套fixture隐藏具体provider/session/reference类型，外部process fixture隐藏binding实现；实际Turn、父子会话、审批、引用、冷Inspector、无fallback及failed-close/部分构造owner均通过 |
| JS/WASM、WASIp1实际执行 | 两目标各7个测试包、43个测试/子测试通过；普通工具/telemetry/durable/内存blob及三种独立消费者模式均成功。原生目录用例按平台skip，3包仅编译无测试 |
| 共享契约可移植性 | storage根契约、纯redaction与durable/journal在两种WASM目标构建通过；SDK未引入平台依赖 |
| 跨平台构建 | CGO_ENABLED=0、desktop tag全部命令和storage实现/privatefs在macOS arm64、Linux amd64、Windows amd64/arm64构建通过；WASIp1 Worker通过；Windows privatefs/SQLite测试二进制编译通过。跨编译不等于Windows/Linux实机验收 |
| 公开面与分层 | storage/blob新增显式快照、具体provider/SQL泄漏及配方位置的正反例、全仓分形文档已纳入整仓结果；相较本批修改前131个SDK源码/模块文件保持原字节 |
| 静态与文档 | 全树tagged vet、go mod verify、gofmt与diff空白通过；本批47个修改Markdown的82个本地目标有效 |

完整JSONL、源码hash、portable和跨平台构建结果保存在本机证据目录，指针`/tmp/alva-storage-contract-path`。
本批完成的是统一插口与装配选择；未增加真实云provider、后台迁移或产品存储选择UI。以下较早批次结果独立保留。

## 已确认约束

- 每个抽象先保留，包或接口合并须给出独立职责、真实消费者、收益、损失供用户讨论。
- Go 不依赖 Rust/Tauri/React 的运行桥，不隐式读旧配置、历史或凭据。
- 保留 SDK 与默认 harness 两入口、单 Agent loop 与多 engine Hub 两套不同 runtime。
- 保留原始事实/投影区别、窄会话契约、同名默认 Plugin 替换、版本化扩展记录和权限边界。
- 先有可执行和反例，再报告完成；不拿界面出现当作整个产品迁移完成。
- 用户确认开源后，Go实现采用AGPL-3.0-only，许可/归属见LICENSE/NOTICE；当前吸收的Magpie协议代码只由integration/llm/codec使用，不进入SDK事实类型/权限/存储。

## 2026-10-20 逐层审计后的边界与耐久等待

- 修复SubAgent成功输出使用stream草稿、子资源setup/cleanup失败丢owner、host重放共享ProviderState、已注册SubAgent schema随run改变四个已复现缺陷。实际AfterModel commit优先，未提交文本只作为失败partial证据；未完成物理资源留给原owner Close重试，Hub attempt registry包含SubAgent。
- 历史近期turn选择/省略提示移至harness；storage提供固定高水位、metadata copy、有界hash/protocol读取，不包含模型提示。Hub Runtime只依赖六方法ExecutionStore，设置/浏览保留typed组合端口和原SQLite事务。
- 指纹/cache/诊断记录编码移至可选extension/telemetry；harness按名默认安装，纯SDK默认无诊断保存。保存失败显式报告后继续；执行事实、恢复事实与Flush仍严格失败。实际Stream前保留独立wire请求完整性检查，不能依赖指纹校验。
- 已提交业务/已接受答案提供Recovery DTO与schema1 RecoveryRecord，默认harness在Join/timeout外保存恢复证据；Error文本不是稳定JSON协议。
- 新增可选harness/durable Controller，Backend只有实际Log/Claim/Load/原子CAS Commit四方法。普通Tool/hooks支持等待退出、精确审批/问题、外部结果和一次原子投递。native适配复用同一facts repository，不新增DB；MemoryBackend明确volatile。
- 跨进程external/approval恢复与副作用写入后exit42的unknown/no replay已由真实测试子进程验证。过期、错binding、旧revision、未知版本、不完整收尾拒绝继续；running/interrupted需检查后明确Abandon。普通CLI/Hub未自动启用该可选API，外部job调度和UI/HTTP接线不冒充完成。
- L1与Plugin/Registrar未为持久等待新增hook/enum；新增kernel/core公开快照审阅可选通用DiagnosticObserver，扩展编码在外。
- 存储按consumer保留ConversationLog/ReferenceBackend/durable.Backend/Hub stores；新增两方法blob.Store，原生OpenWithBlobs真实接入目录/有容量上限内存。会话授权、hash、分页/搜索共用，typed facts仍同SQLite事务。schema v3只增binding/object catalog，不搬迁/删除既有数据；ID/retention换绑或缺失拒绝打开。volatile字节明确标记，上传不确定失败不提交引用、不重试/删除。云账号/provider与产品CLI设置未自动装配，不宣称普通网盘支持原子日志事务。

本轮验证（macOS arm64 / Go1.27.2，无真实模型或云账户调用）：

| 检查 | 结果与范围 |
|---|---|
| 完整desktop-tagged race | 89个有测试包、1873个测试/子测试通过（1962个pass事件含package），0失败，738.77秒；运行期间源码未变化。2个无参数子进程helper按设计skip，实际跨进程用例已执行；另5包没有独立测试文件 |
| 收尾目录边界补测 | 整仓通过后补充clean路径再Lstat，拒绝末尾slash的symlink根；blob后端共用验收及实际harness model/tool/model补测14个测试/子测试通过，0失败；其后无其他Go改动 |
| JS/WASM与WASIp1实际执行 | 两种目标各7个有测试包、43个测试/子测试通过；纯内存blob、普通工具、telemetry和durable实际运行；仅原生目录用例按平台skip。plan/question/schema独立消费者两目标均通过 |
| 跨目标构建 | 完整SDK、codec、普通工具、telemetry、durable与blob/memory在JS/WASM及WASIp1构建通过；WASIp1 Worker构建通过 |
| 原生命令及blob适配构建 | CGO_ENABLED=0、desktop tag全部原生命令在macOS arm64、Linux amd64、Windows amd64/arm64均通过；blob全部适配器也在四目标构建通过。跨编译不等于对应OS或挂载服务的实机验收 |
| 静态与文档 | 最终tagged全树vet、go mod verify、gofmt、diff空白和81个修改文档本地目标检查通过；架构/公开面/分形闸已包含在整仓结果中 |

完整JSONL、最终补测、WASM执行和跨平台构建结果在本机证据目录，指针为`/tmp/alva-layer-fix-path`。
该批验收时真实云provider、产品CLI存储设置和CLI/Hub等待交互尚未装配；后续CLI/Hub显式等待接线见上方独立记录，可选API与默认产品行为分别验收。
下方2026-10-19表是此前批次记录。

## 2026-10-19 工具安装粒度与失败回执整顿

用户确认对照 Pi/Eino/ADK/Deep Agents/Pydantic AI 后的实施建议。本轮沿现有 Tool、Plugin、
Middleware 边界调整外围能力，不新增内核 hook、统一 runtime 或另一套插件系统。

- Planning 的 `NewTaskTools(TaskToolsConfig)` 只消费四方法 `TaskService`；`NewPlanTool(write)`
  独立安装 todo_write，不要求任务 CRUD 或引用存储。原生完整 `Service` 继续组合任务/笔记，
  native Plugin 明确组合六任务与一计划工具。通用任务列表不要求计划读取；原生组合包继续在输出适配中保留 plan_notes 引用。
- 通用任务详情、人类问答、选中工具 Schema 默认返回完整有界结果；配置 `RetainDetail`、
  `ToolConfig.References` 或 `SearchConfig.RetainSchema` 才使用引用输出。JSON 回执最多16KiB，
  超界明确失败，不截断冒充完整；已配置存储失败绝不退回全文。原生插件始终显式配置原有引用政策。
- `planning.CommittedError` 区分已知业务提交后的详情保存/编码/呈现/取消失败，保留实际 call/task ID
  与只查询恢复的提示，原始原因可 Unwrap。默认 native snapshot 确认 append 后的 flush 失败同样标记；
  一般后端错误不被解释为零副作用。实际默认循环保留此错误与 conversation outcome，不自动重放。
- `interaction.AnswerReceivedError` 保留已接受答案的 question ID，输出失败不自动再次询问。
  native question_outcome 分别记录 answered 与 answer_retained，保存失败不把已答写成未答。
  同步等待保持真实取消/join；本轮没有把内存等待或 journal 冒充跨进程暂停恢复。
- Utility 的 config 与 search 改为独立实现，私有公共 helper 不再持有两类工具的混合服务字段；
  偏好下一 request 投影、计时器、真实 live registry、Guard 与原生 CAS 政策继续保持原职责。
- 新增独立 `tests/extensibility/testdata/portable-agent` 消费者：同一程序通过公开 Builder.Tool 和
  默认循环在 native CLI、JS/WASM 与 WASIp1 执行 plan/question/schema，零 Plugin、数据库、
  引用存储或读取工具。确定性模型/答案明确属于 fixture，不改变 Alva CLI/worker 的产品默认工具集。
- CI 新增 Node/wazero 的真实普通工具测试和独立消费者执行；Node 只接收空环境，避免 Go JS
  启动区继承宿主环境导致长度超限。wazero runner 从现有固定模块依赖构建，无新增第三方依赖。
- SDK 源码、六份公开快照、go.mod/go.sum 与固定 QuickJS 资产未修改。外围构造签名显式更新，
  没有旧入口 alias；Plugin.Name、工具名及已有记录 namespace 保持。

本轮验证（macOS arm64 / Go 1.27.2，无付费模型或外部账户调用）：

| 检查 | 结果与范围 |
|---|---|
| 相关扩展 race | 七个普通/原生能力包通过，最终所有新增回归均已纳入下列整仓结果 |
| JS/WASM 实际运行 | Node 执行四个普通能力包：25项通过，0 skip；三种独立消费者模式全部通过 |
| WASIp1 实际运行 | wazero 执行同四包：25项通过，0 skip；三种独立消费者模式全部通过 |
| 跨目标构建 | 完整 SDK、codec、planning/interaction/utility/timer 在 js/wasm、wasip1/wasm 均通过 |
| 完整 desktop-tagged race | 1811项通过（含子测试）/85个有测试包，0失败，1033.75秒；1个无参数durable helper按设计skip，实际跨进程恢复实验通过；另3个包没有测试文件 |
| 独立原生 CLI 消费者 | plan/question/schema 三种模式均通过真实子进程和默认SDK循环执行 |
| 原生命令构建 | CGO_ENABLED=0 + desktop tag，macOS arm64、Linux amd64、Windows amd64/arm64均通过；跨编译不代表对应OS实机验收 |
| WASIp1 Worker 构建 | 实际 cmd/alva-worker-wasm 构建通过；该命令产品工具集未因消费者fixture自动扩展 |
| 静态与完整性 | 全树及独立消费者tagged vet、go mod verify、全树gofmt、diff空白及本轮修改的66个本地文档目标检查通过 |

完整日志和逐项结果在本机 alva-extension-review 证据目录，指针 `/tmp/alva-extension-review-path`。
新增消费者验证可复用能力的接入；产品 CLI/worker 未自动新增工具，不据此宣称新增产品渠道已经装配。

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

2026-10-23至24日产品增量：私有产品库以同一事务升级至v4，保留既有连接/凭据并增加不可变Goal事实与独立模型偏好。目标创建不执行；明确Send时绑定真实会话/原件引用，未知回执以同RequestID显式重试。连接目录GET有四协议适配、取消/分页与来源隔离，不代表付费模型推理验收。收藏不改连接revision；默认参数在实际Attempt准入时冻结，真实Provider请求消费温度/输出token/reasoning/system/轮数，waiting恢复沿用旧快照。不支持的内置目标在产品事务内拒绝，额外引擎仍由实际Descriptor声明能力。

原生UI按现有SmallPen Tokens、组件变体和页面逐层核对；Foundation与Product的图形定义保持，内部共享controls映射已有尺寸、字体、状态与布局。无窗口同尺寸导出与业务回归分别记录，不能据此宣称全部页面、平台、IME或外部账户已验收；伙伴/群、语音、右侧标签工作区等仍按设计状态表列出缺口。

本批全树desktop-tagged race为96个有测试包、2434个测试/子测试通过，0失败；最后两处UI修正后原生包159项race通过。仅任务差异隔离导出另通过后端9包679项及UI/architecture两包391项；全树vet、最终原生包vet、go mod verify与原生构建通过。定点视觉只覆盖Light、1440×960的21/23/25/31/39指定元素；最大引用组合另验证960×680下正文/Send完整及滚动移除，不代表所有页面或真实平台验收。恢复指针`/tmp/alva-workflow-completion-path`保留精确任务补丁、设计版本与检查证据。2026-10-24再次现场核对时旧预览已退出；最新版已安装到唯一Alva.app并实际启动，构建ID核对与真实单窗观察通过。旧测试客户端和可执行副本移入废纸篓，沿用原数据目录；运行回执见同一恢复指针。

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
