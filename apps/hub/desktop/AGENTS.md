# L7 · Hub native UI product
> 独立Hub产品UI，复用基础自绘controls但不污染直接harness入口的依赖图。
## 地位
仅通过 apps/hub/product 共享 UI 无关业务；共享 apps/ui/chat 与 apps/ui/modules 控件包不依赖 direct、Hub 或执行 owner。
## 逻辑

正式工作台复用21/22设计并落B3统一壳：根四边/列间8，Header36在圆角面外，内容面从y52开始；左右辅助面subtle+16圆角+1px border，中间纸张surface+16圆角无描边。220可完全隐藏侧栏，SidebarTools在SidebarSurface外，Conversation标题行在ConversationSurface外，ResourceTabs/动作在ResourceSurface外；普通聊天/目录单标题，工具按钮/Tab28、icon16、正文13。MyGo原生窗口按钮由OS/window owner持有；macOS使用原生TrafficLightPosition，真实TitleBar事实决定顶部、左右安全区和右侧高于常规band时的底部guard，native band只消费一次。160ms原生布局过渡、壳层稳定控件身份，减弱动效立即切换、拖动直接跟手；左开关从侧栏右上移到红绿灯右侧，右开关始终固定于窗口右上且不随资源宽度移动，More只在打开资源栏后出现；窄窗专注仍保留同一窗口壳收起动作。首页右侧入口只打开空视图，不跳任务选择、不创建会话；实际选择任务须明确操作。首页Build/建议/新任务导航不创建会话，明确首条Send才经既有Create、可选Switch、所选文件逐项Upload、初次Detail和冻结prompt的Submit准入；初次及同会话重载完成前禁发，准入隔离此前旧观察；失败保留草稿和已提交身份。项目目录仅通过原生显式选择用于新会话，不改变已有会话或授予权限。已有高级面板/故障切换/播放收尾仍有明确入口。

应用设置拥有独立导航壳和同一返回来源；分类切换不覆盖来源，返回保留会话/Inspector定位及草稿，媒体离开仍真实join并释放正文，返回只恢复冷索引位置。Stop按本窗session和不可变执行owner身份记录未完成请求，跨会话导航不丢失，实际终态才清理；提交回执仅清理匹配的捕获草稿，不覆盖后续输入。

General是会话外的应用偏好页；原生Select只编辑本窗草稿，明确Save/Reload复用共享PreferencesService与有版本的原子保存。Build/观察仅读已提交内存快照，theme/body size在所有窗口生效；Density正式两档布局未区分、语言暂固定，两者按确认隐藏入口并保留已有值。外部修改、冲突与存储失败保留草稿，实际保存返回前阻止关闭其窗口。Reduced motion复用同一CAS偏好；已提交应用开关与系统要求取或，框架动画及共同Loader同时消费，编辑未Save不改变动效。

Developer options是会话外页面，Integration tests与Model evaluation使用不同用例/报告，冷预览后显式运行。保存的Connections实时成为精确revision目标；编辑/禁用跨窗撤销旧选择/预览，已冻结job仍可观察/停止。所有窗口观察同一Developer owner；不借用聊天历史/工具，关闭非最后观察窗不Stop，最后窗有开发者请求时须明确Stop并等待实际收尾。

AEP pane区分原生JS/Python的完整host访问与嵌入式WASIp1的无目录/环境/网络运行，二者都先检查disabled pin并单独批准下一Turn；空interpreter仅选择manifest=wasm。
apps/hub/product.HubService拥有产品认领与原始文件耐久保存；本窗仅持有自己的上传/项目准备回执，发送捕获明确DraftID/版本集合，成功仅清理匹配集合，不借用或清除其他窗口或HTTP暂存引用。关闭本窗释放其未提交暂存claim，实际prepare/upload/Submit返回前保留在途引用；晚到回执仍由原窗口owner登记并收尾，已取消且未被UI采纳的项目准备释放claim，原件保留。UI只提交引用清单与读取有限产品历史；本包仅拥有页面交互、轮询与展示转换。
消息流只经HubService.Transcripts（Tail/Newer/Older）读取，按块累积在product.TranscriptWindow并转成共享行（ID=Block.ID）；上一帧可见首行<5且MoreBefore时自动加载更早内容，Key保证可见行不动；每次应用后按是否停在底部Trim，窗口裁掉最新端（MoreAfter）时暂停跟随新文字，滚到底部附近再重新加载最新。不再直接调用Store.History*，菜单只保留“最新消息”。
中间窗口页头有「对话/轨迹」切换，两个视图各自保持滚动位置。轨迹只经HubService.Traces读取并累积在product.TraceWindow：泳道时间轴（chart.Plot+Painter自绘当前窗口步骤，点击选中），时长/轮次/调用/子调用排布切换，ui.List虚拟步骤列表（Key保持可见行，顶部按钮或滚到顶自动加载更早，窗口未到最新时滚到底向前翻页，运行中轮询Newer），详情「第N轮·第M步」与概述/预览/原始内容（2KiB分页加载更多），字面搜索跳到步骤，子Agent步骤进入子会话并保留父窗口返回；引擎无Inspector时只显示说明。对话里工具块的“在轨迹中查看”按工具自身native分支切到轨迹并选中步骤（带Block.Turn/History），替换原打开旧Inspector面板的入口；旧Inspector面板仍由设置页「数据与存储」、页头菜单与调试命令使用，未删除。
原生窗口独立250ms观察时钟最多一个排队UI closure，进度/审批与终态查询不依赖Build或compositor重绘；关闭observer取消时钟，过期closure不查询或Stop。自绘Build仅呈现当前状态。
等待pane复用同一观察时钟冷读已提交checkpoint；其他窗口保存/继续/放弃会更新本窗，只为同branch/Turn、同请求ID与binding的未解决请求保留草稿。revision变化撤销旧放弃确认，明确操作/刷新后的旧读不得回盖；读失败禁用checkpoint操作，成功后恢复，不自动执行。
持久问答复用同一binding草稿与既有interaction.Answer，提供无默认选项的单选/多选/自由文本；显式保存后仍暂停，不授予文件/进程/网络权限。结果按实际编码后的16KiB限制；到期请求只展示不再接受回答或审批。
每个问题用请求ID建立独立控件容器，输入标签带问题header；同一检查点的其他问题消失时，剩余问题的草稿/焦点不得串到别的请求。
即时问答与native审批分别保存提交中状态和观察epoch；请求在途禁重复提交，回执绑定session/generation/branch，成功后移除实际请求，旧轮询不得恢复它。两条冷读独立失败、分别清空失败链旧控件，不能阻断另一链或真实执行终态刷新；校验失败保留问题草稿。
普通会话Send走既有Submit；选中真实Worker会话时，Send捕获该Worker的文件/Skill及手动正文，经共享冻结器入既有FIFO，再请求原pump继续。入队本身不执行，提交后才消费捕获的匹配回执；dispatch失败保留已提交task并明确不得重发。重开不自动恢复执行，Waiting仍须先显式继续同Turn，再明确Resume队列。
等待面板顶部固定Refresh/Continue，完整请求、回答与放弃审阅位于独立滚动区；长问题/已保存结果不能把唯一继续按钮挤出视口。
会话侧栏在selected detail的UpdatedAt不旧于当前列表项时投影其真实状态，避免正文已完成而侧栏卡running；不修改冻结列表的顺序、成员或分页cursor，也不拿较旧detail覆盖较新列表。
输入区浮层按实际边界定位：/与@在整卡上方6px，控件菜单优先在各自触发器上方；不足时在视口内翻转/滚动。MyGo AttachTo同时绑定焦点与几何，picker保留Message作为owner，以实际frame差值定位整卡外沿；仅几何变化时补一帧，不能加入额外Tab停靠点或持续重绘。输入区8px对称内距、工具栏32px，Send/Stop为32px圆形。

## 约束
工具、AEP、Hooks/diagnostics、MCP/SubAgent/Memory、受管Skills/compatibility、browser与decoder配置均显式冷读；Build不装配能力或启动执行。异步读取与mutation回执只发布到捕获的session/generation/预期pane；实际后台写入照常收尾，迟到成功或失败不拉回旧页、不覆盖新草稿或错误。Gateway只呈现冷观察，Bearer与Stop控件绑定监听generation；一次指针手势也不能跨owner复用，诊断等待对应新帧。

SmallPen正式设计是视觉依据：先核对Foundation Tokens与组件variant，再装配Product页面；复用本目录shared controls，不能直接用框架默认外观替代已有设计。实现改动以同主题/同尺寸导出和控件几何对照验收；业务测试不代替视觉核对，不能为迁就实现反改设计。未实现的产品能力必须如实标识，静态样例数据不能成为运行事实。
用户预览只保留一个明确的客户端与数据目录；更新前核对未发送草稿、待导入文件和真实运行状态，保全后正常退出再更新，不逐轮另起预览客户端。自动化画面检查优先使用独立无窗口宿主。
沿用唯一Alva.app安装入口原位更新；checkpoint保留回执、源码差异与检查证据，不再逐轮留可启动的旧客户端或构建副本。启动后核对实际进程路径与构建ID，再报告最新版已运行；只完成build不能报已更新预览。清理旧程序用废纸篓，保留数据目录。
无WebView/npm/React。用户显式文件选择才读取本地原件，20MiB/文件、每会话共享8条/40MiB暂存上限，单次捕获集合另重验8条/40MiB；尚非每窗独立暂存预算。原始媒体不自动送模型，不假称压缩/抽帧。消息呈现不是模型历史权威。
媒体evidence只开4条metadata页，显式manifest/selection/receipt点击才读有界JSON，Preview/显式Play才读全SHA核验派生BLOB；一个bitmap、最多一个pending帧UI closure。仅单独批准decoder后Play播放已验证静音MP4；已保存canonical WAV可独立显式播放，MyGo拥有共享系统output，window只拥有一条voice；声音故障不报成功。Pause/Seek/导航须join自身voice或真cancel/reap decoder，未完成保留owner并阻止关窗；无autoplay/WebView/模型投递，5fps播放clock不冒充source PTS，diagnostics非持久receipt。真实OS窗口可观察QA仍待完成。
## 业务域清单

路由选择/故障切换消费product/routing同一政策；既存branch按产品ID导航历史，过期账户不能因此新建执行。`workspace_routing_desktop_test.go`覆盖高级入口的动态停用与历史可访问，原普通模型菜单共用同一准入。
| 文件 | 职责 |
|---|---|
| workspace_automation_index_desktop.go / workspace_automation_index_desktop_test.go | 首页常驻自动化真实全库冷索引、稳定ID虚拟行与明确原owner选择；实际CRUD仍走原会话服务，不创建空会话/不启动调度 |
| workspace_density_desktop_test.go | 同尺寸真实MyGo多任务容量/字号/按钮/限宽选择器验收与可选fixture PNG；不发模型、不读取用户目录 |
| workspace_metrics_desktop.go | 单一正常密度的Foundation映射：界面13/辅助11/小标题14/标题20，普通控件32、图标动作28/图标16/小图标14、导航行28；阅读字号仍由原偏好控制，不增加密度模式 |
| workspace_layout_desktop.go / workspace_layout_desktop_test.go | B3根四边/列间8、Header36与y52纸张、三栏共享16圆角及有界原生布局过渡、左移开关/标题与固定右开关；左右辅助面subtle+1px border、主面surface无描边；Foundation220导航展开/完全隐藏与220–432有界宽度，左右共用8px指针/键盘splitter、方向键/Home/End；实际扣展开24/隐藏16壳宽且窄窗只约束显示不改偏好，右侧保留432阅读/320资料，专注常驻返回且折叠不Stop；无窗口控件验收与OS鼠标验收分开 |
| workspace_layout_preferences_desktop.go / workspace_layout_preferences_desktop_test.go | 左栏布局复用共享应用偏好逐字段稀疏CAS与250ms防抖；宽度/折叠独立gesture版本与旧回执隔离、未改字段投影远程回执，并发外观合并、失败再调重试和真实保存收尾后放行关闭；SQLite重开/跨窗与无窗口控件验收，不Stop执行owner或重置未存设置表单 |
| window_chrome_desktop.go / window_chrome_desktop_test.go | MyGo0.7.4使用macOS HiddenInset及TrafficLightPosition=(16,19)/其他平台Hidden、保留OS按钮与原生窗口边缘；按实时TitleBar.Left/Right/Height放置同中心36内容行，52/Left86得top8、y52纸张、collapsed开关x94；Right band统一补`max(0,Height-(top+36+8))`的列底guard，60/80高band同步下移所有列且不重复加native band；原Close守卫不变，无窗口几何与真实OS拖动/窗口按钮验收分开 |
| workspace_connections_desktop.go / workspace_connections_desktop_test.go | 连接来源/表单/手填模型与CRUD；表单外层共享实际剩余高度并有界滚动，pending审批/问答不被挤成零高，保留553基础高度并受原生安全36返回行后的实际剩余Body约束；RequestID创建/明确恢复保留后来编辑，删除审阅锁定ID/revision，失踪编辑不借其他账号metadata；表单自动填的模型目录随请求地址换源，用户手填的不覆盖；秘值掩码、失败保稿；sources/form真实返回移到同一native-safe页头并保留原handler/保存禁用，无卡片重复返回；sources固有卡片继续居中，pending挤压时仅外层viewport收缩并滚动完整来源列表，native左右安全区下逐项选择/返回与问答可达且零隐式回答/保存 |
| workspace_models_desktop.go / workspace_models_desktop_test.go | Foundation ModelRow整行hover/选中、28px收藏/参数独立动作、真实路由筛选/搜索与16384项虚拟键盘列表；失效路由及不兼容参数拒绝 |
| workspace_model_preferences_desktop.go / workspace_model_preferences_desktop_test.go | 收藏、默认Agent及参数独立Save/Reload；真实会话导航/重开/跨窗参数观察、旧poll与保存回执隔离、显式空覆盖和dirty草稿保护；取消/冲突保稿、保存期间关窗守卫，不选择或执行 |
| workspace_model_options_desktop.go / workspace_model_options_desktop_test.go | 39行内参数组件与主选择器外侧12px浮层；独立已提交会话参数观察按session/branch/revision/epoch采纳，失败不退默认；Agent/思考草稿保留，Context未知禁用与已声明Vision只读 |
| workspace_controls_desktop.go / workspace_controls_desktop_test.go | Foundation Button/Input/TextArea/Select/Checkbox的内部原生映射；共用32px控件、保宽/保焦点与防重Loading、只重绘共享Lucide Loader及系统减弱动效、Ghost/Danger和焦点环；Disabled意图构建前传入，统一subtle填充/描边与secondary字（Ghost透明），真实Disabled保持MyGo整树0.5，正式设计0.6未声称精确复现；Loading优先且保焦点防重；无窗口键盘/几何/像素/重绘回归，不依赖框架默认外观 |
| workspace_switch_desktop_test.go | Foundation Switch的38×24/16px滑块真实像素与pointer/键盘checked语义回归，避免裸SwitchBase不可见 |
| workspace_companions_desktop.go / workspace_companions_desktop_test.go | 独立伙伴/群真实CRUD、明确账户/计费、稳定Skill与有限回合；已删除身份冷入口及成员原文只读、消息/轮次真实keyset继续加载；可选浏览缺席保留live CRUD及带有界警告的原观察/真实owner Stop；保留完成后pending reload |
| workspace_companions_wait_desktop_test.go | 真实本地HTTP等待、SQLite重开及原生继续群回合按钮的同Round/Turn补投回归 |
| workspace_companions_edit_desktop_test.go | 实际群设置取消后仍按已提交回复策略发送、失败保留独立表单、scope变更清除旧正文/轮次/审阅并冷读新事实；旧异步回执与A→B→A导航隔离 |
| workspace_resources_desktop.go / workspace_resources_desktop_test.go | 与Main同级的subtle+16圆角+1px border资源面，ResourceTabs/动作在面外、36px Tabbar与原生右侧避让、28px关闭及More内弹出/收回；首页空态、无服务明确提示、零标签显式开合和高级导航暂时隐藏观察；32px路径/当前载入内容查找、文档阅读内距；实际4KiB Markdown/原始内容/校验入口、有界页与实际空错态，真实bootstrap/SQLite组合回归；来源身份消歧、排序/改宽CAS、关闭/选中/拖放及ABA隔离，不Stop |
| workspace_voice_desktop.go / workspace_voice_desktop_test.go | 显式系统本地录音、独立Stop/Cancel和真实Done收尾；结果绑定原草稿/会话，晚到或已改保留待采纳，32KiB预算，不自动发送 |
| workspace_goals_desktop.go / workspace_goals_desktop_test.go / workspace_goal_submission_desktop_test.go | 共用目标Dialog、幂等回执与项目/当前会话的冷cursor分页浏览重选；旧范围回调隔离，重开后显式首发引用真实原件 |
| workspace_permissions_desktop.go / workspace_permissions_desktop_test.go | 原生Ask/Auto/Full、Full范围确认、首页仅意图/保存会话真实持久、Plan互斥及失败不伪报成功 |
| workspace_picker_desktop.go / workspace_picker_desktop_test.go | 原生输入/显式菜单触发的命令、Skills及项目/最近/绝对路径文件候选和目录导航；取消/迟到结果隔离、Enter选取仅准备草稿；平台Cmd+Enter复用Send准入，菜单/IME/运行等待不误发，旧回执保留新草稿 |
| workspace_plan_desktop.go / workspace_plan_desktop_test.go | 首页本窗计划意图与已存会话真实下一Turn策略，成功保存才更新branch缓存 |
| workspace_prepared_desktop.go / workspace_prepared_desktop_test.go | 捕获typed/DraftID版本选择、首发Create前复用目标范围/Plan校验；本窗引用owner在真实在途操作返回后释放关闭或未采纳claim，失败保留与匹配消费，不覆盖新草稿 |
| workspace_shell_desktop.go / workspace_shell_desktop_test.go | B3导航顺序：SidebarTools36在SidebarSurface外，只留开搜索对话框的28图标与28新任务按钮，旧侧栏内联搜索行已移除，欢迎/会话页同一入口；壳层同一左开关在侧栏右上与原生左区右侧之间移动，Main/标题同步向左、PanelLeft28真隐藏侧栏/分隔器；右开关独立于动画父容器保持每帧同位，主页头只预留28槽；普通聊天/目录单标题、位置只作tooltip，会话页头右侧放「对话/轨迹」视图切换，窄窗专注不换owner，More菜单由资源Tabbar呈现；真实本地模式Footer、40px品牌/24字号限宽首页及Ghost建议；首页Panel只打开空右栏，保留草稿/会话/业务pane，不Create/Send |
| workspace_search_desktop.go / workspace_search_trace_desktop.go / workspace_search_trace_e2e_desktop.go / workspace_search_desktop_test.go | 居中DialogBase搜索已存任务：真实`Store.PageSessions` 32/页游标前进后退、Enter经MyGo官方`OnSubmit`一次投递（一次Enter只发一次共享读、被hold的读不自我取消、同文再次Enter是合法刷新）、typed query未提交不沿用旧页、替换/Escape/Cmd+K重开以epoch取消迟到读、失败显示真实错误并按原请求重试、结果行仅从当前有效页经既有selection owner导航且保留草稿；结果行中心落在有界滚动视口内，无窗口验收不冒充OS窗口或已模拟队列迟到回调；一次已验证等待的稀疏数值诊断：同一epoch覆盖初次/翻页/重试的request与store-start/done耗时与错误布尔/计数、ctx-skip、dispatch-enqueued、UI dispatch内epoch-drop、applied及Build侧epoch/loading去重的render快照，只记epoch/字节长度/布尔/耗时/计数，不记查询文本、路径、会话ID或原始错误；普通build仅在已有diagnostic owner时输出否则静默，e2e仅复用MyGo既有MYGO_AUTOMATION opt-in回落到共享stderr logger，无新设置/env flag/协议/timer/polling/repaint/retry或行为变化 |
| workspace_panes_desktop_test.go | 左右独立开合、左控件/Main/标题同向无起跳及右控件每帧固定、Main实际收窄、窄窗资源专注同位返回、首页零创建/执行、SQLite零标签开合及高级pane/草稿保持；三栏圆角像素、真实动画中间帧/反向/结算停止与减弱动效回归 |
| workspace_navigation_desktop.go / workspace_navigation_desktop_test.go / workspace_navigation_boundaries_desktop_test.go | 独立共享项目索引8条/展开任务16条及最近页的单一虚拟导航；新增pager请求的cursor/query/back成功一起采纳、失败保留原重试目标；冷Build与project epoch/子state隔离，父刷新失败保留map时旧子读仍结算加载且拒绝旧页；真实Detail回执按session/branch/generation验证，不以当前页冒充全库或读取文件正文 |
| workspace_picker_layout_desktop_test.go | Foundation Commands等宽双栏/References单栏、28px图标行与搜索页脚、窄窗纵排及General800px左对齐扁平布局的无窗口几何回归 |
| workspace_composer_geometry_desktop_test.go | 首页/对话短长输入的浮层上方锚点、工具栏不被覆盖、28px圆形操作与8px对称内边距、紧凑菜单触发点回归 |
| composer_stability_desktop_test.go | 默认跳过的本地墙钟 soak：实际 bootstrap/SQLite/MyGo 无窗口 Send/Stop、文件读取搜索、跨窗 CAS 草稿与 Close/reopen；独立记录内存/RSS、PDF worker 与真实清理，不以短测冒充一小时或 OS 窗口验收 |
| workspace_composer_desktop.go / workspace_composer_desktop_test.go | 多行输入、真实附件/路由/权限说明及发送/停止；内部描边保持四边8、28工具栏与8标签间距、16麦克风；IME、32KiB、浮层、草稿和双轴对齐 |
| workspace_drafts_desktop.go / workspace_drafts_desktop_test.go | 同逻辑home/会话跨窗同步的耐久手稿、250ms防抖CAS与导航/关窗flush；严格准备/文件回执冷恢复、冲突保稿和版本绑定消费，首次Create原子绑定身份后重开不发送 |
| workspace_composer_files_desktop.go / workspace_composer_files_desktop_test.go | 首会话前@与原生文件选择立即捕获私有原件、所有引擎共享文件回执/草稿关联；真实失败来源卡片与重试/移除、原scope与SHA保留，不在选择时发送或授予目录权限 |
| workspace_attachments_desktop.go / workspace_attachments_desktop_test.go | Foundation Attachment与Badge、长路径Wrap/共享126px引用滚动区；首页待导入、明确发送时复用Upload、部分失败只重试剩余、原件/引用与分支准入不变 |
| workspace_settings_desktop.go / workspace_settings_desktop_test.go | 设置复用B3共享壳：导航Header36在左subtle+16圆角+1px border面外，主表面y52无border；8类SettingsNav使用native-safe行，唯一返回位于主面Header36，应用级与实际会话级配置分开；connection sheet使用8 inset且保留原表单内容；会话类复用原MCP/插件/工具/Skills/权限/导出入口，数据目录只读，不伪造清理功能；同一审批/问答在设置壳及连接表单仍可操作，原会话/视图与媒体冷索引恢复，草稿/运行/播放收尾回归；真实pending与展开连接表单在1240×850/960×680及原生左右safearea下均可滚达动作且不隐式回答/保存 |
| workspace_settings_scope_desktop.go | 设置/资源/自动化共用有界已存任务选择器及exact owner冷验证；选择统一校验当前结果成员/加载owner，显式select经原导航保存草稿并等匹配Detail回执再Open；隐藏读取按epoch结算和缓存、不抢导航，普通后台poll不撤销已验范围；不Create/Send或扩大权限 |
| workspace_lifecycle_desktop_test.go | 真实取消收尾期间跨会话保持Stopping与防重Stop、延迟准入期间新输入不被旧回执擦除 |
| workspace_actions_desktop.go / workspace_actions_desktop_test.go | 明确首发的真实Create/Detail/Submit、部分失败身份保留、草稿导航、同会话重载禁发与持久fallback可取消回归 |
| workspace_view_desktop.go / workspace_routing_desktop.go / workspace_project_desktop.go | 高级面板分派、保存分支与账户/计费归属的fallback批准/禁用、原生项目目录选择 |
| workspace_assets_desktop.go / assets/ | 一次注册有平台回退的设计字体、固定Lucide原始SVG与语义表面色；包含来源哈希、完整OFL/ISC/MIT许可 |
| preferences_desktop.go / preferences_desktop_test.go | 无会话General页、共享选项的实际原生Select与明确Save/Reload、保留冲突/失败草稿；跨窗theme/body size、未定义布局的Density禁选和只读系统reduced motion，SQLite/bootstrap重开、冷Build与保存期间关窗守卫的无窗口回归 |
| developer_desktop.go / developer_desktop_test.go | 无聊天会话的开发者测试/评估入口、动态Connections目标刷新/旧预览失效、明确模型选择/预算/任务集、冷预览绑定与运行/停止/分开报告；1240×850实际保存连接至推理、跨窗禁用及阻塞解析时Stop可达/实际join的无窗口控件回归 |
| wait_desktop.go / wait_desktop_test.go | 默认off等待面板、完整请求/已保存结果与可复制Turn ID、跨窗观察/绑定草稿/旧读排除/读失败禁用；只保存结果、同Turn继续和重新审阅后放弃，1240×850无窗口控件回归 |
| wait_question_desktop.go / wait_question_desktop_test.go | 持久单选/多选/文本与编码限额，拒绝空白保存；重开/明确继续同Turn、过期禁用、独立写审批、多问题草稿隔离及滚动动作可达，不要求手写JSON |
| app_desktop.go | 正式工作台装配、共享虚拟消息或执行轨迹（trace状态随会话切换重置、轮询时跟随运行中轨迹）、异步会话/观察/发送与明确审批；会话参数已提交观察独立于高级配置编辑态，真实导航/轮询复用已有Settings读取；高级面板由workspace入口分派；打开/轮询/更早内容分别走Transcripts Tail/Newer/Older，MoreAfter取代旧reviewing |
| panes_desktop.go | MCP/native SubAgent/持久Memory显式冷settings与原视图回执；固定高水位Inspector、typed child钻取/返回父页与2KiB原文（设置「数据与存储」与页头菜单入口保留） |
| workspace_trace_desktop.go / workspace_trace_desktop_test.go | 对话/轨迹切换、时间轴与排布切换、虚拟步骤列表与向前/向后分页、详情三标签、搜索跳转、子会话进入/返回与不支持说明；页读取被丢弃时（epoch仍匹配）也结束loading，generation变化（Send/Stop/上传）则按当前位置重开轨迹；搜索/详情/原始读取各有独立epoch，丢弃时同样结束busy；选中的未完成步骤配对后重读详情；运行结束后在途读取完成再补一次Newer；分页出错后滚动不再每帧重试，等用户操作或下一次轮询；原始内容按记录序号定位；测试覆盖切换保持两侧滚动、加载更早不跳动、详情与原始分页、子会话、无Inspector引擎、搜索与时间轴点击，以及丢弃读取/搜索不卡住、未完成步骤配对后刷新、结束后补读、同角色原始记录按序号、出错退避，ALVA_UI_BENCH=1时轨迹滚动帧CPU p95≤8ms门 |
| export_desktop.go | 已选branch/typed子路径的实际JSONL私有导出；可选真实关联child，失败不报成功，Close等待写入；分类只筛当前metadata页 |
| aep_desktop.go | native-only显式文件夹/JSON检查、disabled pin保存、独立host同意与下一Turn启停 |
| aep_desktop_test.go | MyGo实际输入JSON检查与独立批准，不启动任何模型/解释器 |
| commands_desktop.go | 原生Hooks/diagnostics明确argv JSON编辑、独立host批准与下一Turn启停，不自动运行诊断 |
| commands_desktop_test.go | MyGo实际JSON/批准和持久memory opt-in控件，保存/观察不启动任务 |
| shell_desktop_test.go | 真实应用内后台job冷列表与native显式Stop等待reap，不启动模型 |
| tools_desktop.go | 六类工具原生配置/独立enable、跨pane真实单选/多选/Other/自由回答、失败保稿和提交中防重；问答不增加权限，同pane后台Shell列表/Stop/idle reset复用应用owner |
| tools_desktop_test.go | 七类配置冷读与九类mutation的原session/generation/pane回执隔离、真实提交保留与零执行；问答独立读错、防重、实际答案格式/纠错和重开后旧回执回归 |
| gateway_desktop.go / gateway_desktop_test.go | 显式Gateway Start/绑定generation停止与一次Bearer可见性；跨窗停启/晚回执清过期凭据，冷观察和Closed/Stopping守卫，旧按下控件不能停止替换owner |
| media_settings_desktop.go | 默认off媒体工具、显式decoder绝对路径、编辑禁用和独立host批准，不自动启动 |
| media_desktop.go / media_desktop_test.go | cold来源/产物/selection/receipt分页、显式实际JPEG预览/释放及MyGo无窗口回归 |
| browser_desktop.go / browser_desktop_test.go | explicit browser JSON禁用保存/独立host-network批准/编辑撤销与零启动控件回归 |
| handoff_desktop.go / handoff_desktop_test.go | 仅当前消息窗口内时间线块的handoff ID→显式读取exact已保存脱敏snapshot，不恢复或执行 |
| playback_desktop.go / playback_desktop_test.go | actual ffmpeg+MyGo Play/Pause/seek、单Bitmap.Update或audio mixer position、one-closure背压、导航回收；fixture双流排空后回收，失败也清理；真实OS窗口另验 |
| audio_desktop.go / audio_desktop_test.go | canonical WAV→有界in-memory MyGo voice、显式Play/seek/Close，设备错误不冒充可听成功；冷构造/非发声fake voice测试，不声称真实声学验收 |
| automation_desktop.go / automation_desktop_test.go | 普通会话paused-first CRUD、固定账户/计费确认、冲突保存保稿/重开已提交事实、编辑暂停与ID/revision绑定二次config-only删除；刷新撤销旧审阅，不私自Tick或执行 |
| app_desktop_test.go | MyGo无窗口启动/输入/停止状态与selected侧栏终态/新旧快照/cursor隔离回归；共享workspaceStaticView固定既有业务测试的结算帧，真实动效另由panes测试使用原生时钟检查 |
| observer.go / observer_test.go | 独立时钟的有界UI观察；无重绘仍刷新，慢UI不堆积，关闭不迟到读或Stop |
| debug_desktop.go / debug_desktop_test.go | 独立默认关闭诊断owner注册实际产品视图与Send/绑定实际执行owner的Stop，select_route调用当前模型菜单handler；草稿仅长度/SHA、路由有界、controls/execution分开；Gateway过期凭据撤销后须新帧才可截图，真实HTTP/MyGo回归不冒充OS窗口验收 |
| windows_desktop.go / windows_desktop_test.go | 最多8个同app观察窗共享product/runtime；资源收回走真实Close守卫后聚焦来源，来源关闭则原位/现存工作台，清除双向关系不保留旧缓存；关闭不冒充Stop |
| skills_desktop.go | 受管项目Skills原生编辑/下一轮enable、显式compatibility文件冷读与下一轮启停；无目录时仍可用规则插件，旧原文读取/操作回执不覆盖后来视图 |
| compatibility_desktop_test.go | MyGo实际配置路径输入/明确启停、未初始化native Skills仍可用、导航/重开隔离旧Skill原文与零执行回归 |
| workspace_transcript_desktop.go / workspace_transcript_desktop_test.go | product内容块→稳定ID行与工具/来源投影（类型来自Block而非引擎名）、截断块明示只显示开头、可见行驱动的自动分页；共享有界列表独立top16/行距24、右对齐subtle用户气泡、32头像/12间距回复列、仅已存native/attempt来源匹配的真实身份与当前消息复制；沿同一Markdown注册；工具披露的“在轨迹中查看”切到该工具自身分支的轨迹步骤，拒绝旧会话/分支/行绑定、未知owner不猜当前分支，不用composer目标给历史贴模型 |
| workspace_transcript_scroll_desktop_test.go | 真实SQLite历史下滚到顶部自动加载且首个可见行不变、往上翻时新内容不拉回底部、裁剪窗口滚到底部重新加载最新、无“更早的消息”菜单；ALVA_UI_BENCH=1时2万行无窗口滚动帧CPU p95≤8ms门 |
| team_desktop.go | 原生collaboration、真实Worker会话focus；捕获该Worker文件/Skill与手动正文入原branch的typed FIFO，提交才消费匹配回执；队列编辑/取消/分页、显式resume/stop和持久生命周期 |
| team_desktop_test.go | MyGo建Team/Worker零执行、typed文件/Skill入队重开/防重/显式FIFO与同Turn等待继续、原件/派生损坏在HTTP前失败且保留新稿；跨pane审批及真实Stop等待owner清理并保留后续队列 |
| worktree_desktop.go / worktree_desktop_test.go | explicit首Turn前隔离、冷SHA/path索引与8KiB raw diff页、二次确认真实采纳及整树删除审阅/独立确认/Worker退休；离开释放正文、未知不重试，无OS窗口验收主张 |
| approvals_desktop.go / approvals_desktop_test.go | 跨pane one-call审批，完整正文有界滚动/动作行独立；Allow/Deny共享Loading保焦点尺寸，实际回执防重、问答隔离与旧读排除 |
| workspace_engine_interactions_desktop.go / workspace_engine_interactions_desktop_test.go | native审批/多问题opaque handle及poll/epoch；完整请求滚动、动作可达/保焦点、Other空白拒绝、秘密掩码、失败保稿、成功隐藏原请求且不伪造resolved事实；Build无I/O |
| preferences_motion_desktop_test.go | 应用减弱动效的真实Save/CAS/跨窗投影及paint-only Loader停止请求帧；系统减弱要求始终有效 |
