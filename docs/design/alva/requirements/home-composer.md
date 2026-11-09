# home-composer

> 来源：旧包 `alva.smallpen/requirements/home-composer.json`。本文件是这些需求的正本；页面位置按新包路径和元素 `data-name` 链给出。

## req_alva_composer_state — 状态与实现边界

状态：2026-10-26 用户确认后的当前契约；实现与验收分别以根 MIGRATION 和 docs/HUB-FLOWS 为准，历史截图/测试不代表本轮已通过。
分开保存 draft（正文、明确引用、命令准备及单调 revision）、run（空闲/提交/运行/等待/停止/终态）、voice 和 overlay。14个 Composer 变体是代表外观，不是全排列状态机；复用 Foundation 组件，不为每个触发点或文案画一张图。
首页和已有会话均能准备文件、命令和项目 Skill；选择、详情、取消不执行。运行期间可编辑下一份手动草稿，不自动排队、不插话、不自动发送。
草稿属于逻辑首页/项目或会话，跨同一会话窗口共享；不同会话隔离。保存、切换、关闭和重启恢复正文及明确引用。UI拥有光标/焦点/临时查询；共享服务、SQLite事务和真实执行owner拥有已提交事实。隐藏页面、关闭资源/Inspector、退订都不是Stop。
录音可与运行并存，停止录音不停止任务；下一轮设置保存有独立状态，不冒充消息提交，保存失败保留草稿。
紧凑布局：主导航220、页头52、主区外边距24；首页限宽760，标题复用 ui.font.size.heading（当前24）及600/1.5，标题到输入区16、输入区到建议12。导航行36、条目间隔4、分组间隔12；输入区对称内边距8、间隔8、工具栏控件32；Send/Stop为32×32圆形、圆角16、图标18；Add图标18，Permission/Model文字14/行高21、图标16、下箭头14。输入正文与连续阅读保持16/1.75，两行起始编辑空间56；会话组间隔16。空间靠布局收敛，不缩小阅读字号，不增加Token或变体。
验收：同时打开同一会话可看到已提交草稿版本；另一个窗口有未保存编辑时明确处理冲突，不能静默覆盖。重启只恢复草稿，不执行。运行中录音/编辑不挤走任务Stop。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_state — Composer：草稿、执行、录音、浮层独立；共享浮层的触发和组件引用见 req_alva_composer_focus。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_state — Composer：草稿、执行、录音、浮层独立；共享浮层的触发和组件引用见 req_alva_composer_focus。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_add — 添加菜单

状态：引用 Alva ComposerMenu.View=Add；41 是唯一添加菜单代表稿。首页打开/关闭行为按共享契约接线，当前不再另画本页浮层。
触发：点击 Composer.Attach（+）打开当前输入框的菜单，重复点击收起；不创建新任务。
动作：文件/图片/视频打开系统选择器；目录添加引用；新建目标进入目标填写；计划模式切换下一次提交意图；协同模式先选伙伴及作用域。
退出：Esc/外部点击关闭最上层浮层，焦点回输入框，草稿/光标保留。系统选择器取消不改变草稿。
空/失败：来源不可用时原位说明；选取失败允许重试，不清空其他引用。目录引用不等于授予目录写权限。
验收：有草稿时依次开菜单、取消选择文件、关闭菜单，草稿与焦点不丢失，未发生发送。Goal表单与创建/引用回路已实现；协同的完整选择/取消回路仍缺，不能把有菜单项算功能完成。

前轮运行校准（2026-10-23，交互业务审查；历史记录）
原生已接：首页系统选择器只保留本窗待导入路径，不读原件、不创建会话；明确Send才Create→可选Switch→逐项Upload→Detail→Submit。成功引用和失败/剩余路径分开保留，重试只导入未成功项。目录引用、目标填写、真实计划模式和常驻伙伴选择仍缺；不把会话Team当作这些功能已完成。
证据：apps/hub/desktop/workspace_*_desktop_test.go、apps/hub/runtime_identity_test.go；本轮相关12包581项测试/子测试通过。原生无窗口控件与实际OS操作分别报告，详见根MIGRATION.md。

原生运行校准（2026-10-23，项目引用/命令/Plan 本波实现；本波回归已通过；真实 OS 验收另列）
“+”已连接系统文件选择、项目文件/目录、命令与 Skills、计划模式。项目路径选择限已有 Alva 会话的已保存 workspace；首页不可借此授予目录或读取任意 host path。项目引用选择由共享 service 重审来源后保留文件原件 artifact 或目录 metadata manifest，加入本会话引用草稿；命令/Skill 选择创建本窗 typed preparation。二者均不启动模型/工具执行，明确 Send 再经共享 service 验证归属并提交引用。原有系统文件选择仍按待导入→明确 Send→Upload 处理，失败保留未成功部分。
Plan 首页只改变本窗下一次发送意图；保存会话通过 SetPlanMode 保存下一轮设置，不启动模型或命令。运行/等待时禁改；仅 Alva 有实际等价策略。新建目标和常驻伙伴选择/作用域仍缺，会话 Team 不冒充其完成。
验收状态：本波全仓 desktop-tagged race 96 个有测试包、2252 项测试/子测试、0 失败；原生 headless 六图和深路径标签追加两图通过。选择/退出零发送、来源失效拒绝和实际 Plan Guard 已纳入回归；真实 OS 选择器、窗口焦点与平台验收仍单列。

目标最小流程（2026-10-23，原生共享服务/表单已实现，真实OS验收另列）：点击新建目标→共用DialogHeader/Field/DialogFooter表单，字段只有既有的目标名称、完成标准、项目/会话范围；打开不创建会话或运行。草稿→校验→创建中→真实创建回执或原位失败。名称/完成标准或范围无效时保留输入并定位字段；范围必须由产品根据稳定项目/会话身份验证，没有可识别范围就要求明确选择，不自动建会话或扩大授权。明确创建后由共享L7 GoalsService/GoalStore持久化稳定goal ID+范围+revision，并将可移除引用加回原Composer草稿；只有随后明确Send才提交执行意图。创建失败保留表单，结果未知先查询实际创建结果，不能自动重复创建。Cancel/Esc只放弃目标表单编辑并恢复原输入草稿/焦点；移除已关联目标引用不删除目标事实。目标服务/存储口已在product与bootstrap显式装配；首页明确Send才绑定真实会话与原件Artifact，消费目前仅Alva，不把goal字段塞进kernel或新建执行runtime。验收：开关/取消零创建、明确创建一次、失败保稿、跨会话引用拒绝、移除不删除目标、创建本身零模型/工具执行。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_add — Composer.Attach 点击 → Alva ComposerMenu.View=Add；内容在共享组件和 41 代表稿维护。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_add — Composer.Attach 点击 → Alva ComposerMenu.View=Add；内容在共享组件和 41 代表稿维护。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_references — @ 引用与附件

状态：2026-10-26 当前契约。复用 Alva ComposerMenu.View=References 及 Loading/Empty/Error，不复制文件弹层画板。
触发：非IME、非粘贴的一次新输入单字符@，位于文本起始或空白/([{之后；邮箱词内、预填草稿、多字符插入不自动打开。位置、键盘和退出复用 req_alva_composer_focus。
范围：首页尚无会话也可选文件；任意电脑位置、任意文件类型均可保存原件。默认项目/最近候选按文件名和路径有界搜索，支持明确绝对路径、~/路径、目录导航和系统选择器；不建立全盘正文索引。查询取消旧请求，迟到结果不能覆盖新查询。
选择：返回原输入框，保存原件、SHA256、来源/作用域与真实处理说明到应用私有附属目录和产品目录。选中、捕获中、失败/重试、移除分开。失败不偷偷丢弃该文件后发送；移除只移出草稿，不删除用户原件或已保存事实。
投递：单次至多8个明确文件引用，单文件捕获技术上限256MiB，原件流式保存。UTF-8短文本每文件至多4KiB、每请求至多16KiB，仍受实际模型上下文准入约束；长文本/文档发送小索引，由既有context_read/search按范围读取，不直接塞全部正文。格式处理与捕获独立：Office文本是有损派生；PDF文本/图片像素需要实际处理器和模型能力，失败或不支持必须如实显示，不把原件已保存说成已理解。
安全与恢复：模型输入来自结构化已校验回执，不从提示词反向解析主机路径/权限。Send冻结集合并重验作用域/校验值；原文件删除后仍可读取捕获版本，附属原件变动则拒绝。重启恢复同一集合，不重新扫描/发送。
验收：首页选项目外未知扩展名文件、删除来源、重启后仍能读取原件；长文件跨页搜索；变动/跨作用域/超限拒绝；输入A后B，A迟到不覆盖B。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_references — Composer.Prompt 输入 @ → Alva ComposerMenu.View=References；查询变化、选择、插入和退出见本契约。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_references — Composer.Prompt 输入 @ → Alva ComposerMenu.View=References；查询变化、选择、插入和退出见本契约。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_commands — / 命令与 Skills

状态：2026-10-26 当前契约。复用 Alva ComposerMenu.View=Commands；命令、详情和参数是内容，不为每个Skill建立组件变体。
触发：非IME、非粘贴的新输入单字符/，位于文本起始或空白/([{之后；词内/、URL、多字符插入不触发。后续片段含/、反斜杠或冒号按路径处理，退出命令选择。定位、焦点、键盘与退出复用 req_alva_composer_focus。
目录：首页可读取当前明确项目下真实 .alva/skills 与内置/plan、/review；已有会话以saved workspace为准。不扫描HOME厂商目录、不隐式导入厂商授权政策。外部Codex自己的Skill发现仍由明确选择的本机CLI配置负责，不假冒产品目录。
确认：↑↓活动项、hover预览、Enter/点击只准备，不Send。需要参数先补参数；外链只查看详情。准备记录稳定来源、workspace、名称、原文SHA与会话/分支（创建前可为空）。
Send：共享服务先重验来源；首次创建后绑定同一实际会话/分支。明确所选Skill进入同一Turn结构化快照，执行时可用；没有全局启用Skills时只暴露所选来源，不顺带启用整个目录。来源变化/归属冲突/引擎不支持则拒绝并保留草稿。/plan复用真实只读计划政策，不是shell命令；任何菜单项都不创建另一个执行器。
验收：首页选Skill后重启、发送，真实模型目录只有所选项；修改SKILL.md后Send拒绝；/菜单Enter只选择；选择和预览均无模型/脚本/进程执行。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_commands — Composer.Prompt 输入 / → Alva ComposerMenu.View=Commands；确认只插入，不自动发送。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_commands — Composer.Prompt 输入 / → Alva ComposerMenu.View=Commands；确认只插入，不自动发送。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_focus — 共享浮层、提示与触发绑定

共享规则：外观、圆角、边界、阴影、间距、宽高行为由 Foundation 的共用浮层配方、Alva Tooltip、MenuItem 和 SelectOption 定义一次。PermissionMenu/ComposerMenu 复用这些规则表达不同业务内容；文字变化、触发位置或提示语不新增画板/变体。41 添加菜单保留一处代表装配。
触发绑定（21/22 共用；在各自 Composer 实例上标注）：
| 控件/条件 | 内容引用 |
|---|---|
| Attach 点击 | Alva ComposerMenu，View=Add |
| Prompt 输入 @ | Alva ComposerMenu，View=References；边界/筛选规则见 req_alva_composer_references |
| Prompt 输入 / | Alva ComposerMenu，View=Commands；边界/选择规则见 req_alva_composer_commands |
| Permission 点击 | Alva PermissionMenu，Mode=当前值 |
| Permission hover/focus | Alva Tooltip，内容取当前权限模式说明，见 req_alva_composer_configuration |
| Model hover/focus | Alva Tooltip：选择本轮使用的模型与 Agent。点击仍使用 39 模型选择器代表页 |
| Voice hover/focus | Alva Tooltip：语音输入；停止录音后转写到草稿。 |
| Send hover/focus | Alva Tooltip：可发送时提示发送与换行；运行时提示停止任务；提交中/停止中说明实际状态。禁用时说明当前原因，不复用一句误导文案 |
交互：说明性提示使用 hover/focus，不能放必须点击的动作；可选择内容使用点击菜单。Esc 关闭最上层浮层并恢复触发点焦点；外部点击收起菜单；草稿与光标保留。Tooltip 关闭不改变已点击打开的菜单或运行状态。
键盘：IME 合成先处理；菜单打开时普通 Enter 只选择活动项，平台发送键 ⌘/Ctrl+Enter 被消费但不选择、不发送、不插入换行。菜单关闭后的明确发送手势见 req_alva_composer_submit；Enter/Shift+Enter 仍换行。↑↓移动活动项，禁用项不激活，长列表滚动并保持活动项可见。
定位：@ / 候选菜单锚定整个 Composer.Editor 卡片左上方，与卡片保持6px间距；不锚定正文 TextArea 的底边，不覆盖文字、光标或工具栏。Attach、Permission 与紧凑 Model 菜单优先出现在各自触发按钮上方，间距6px；空间不足时按视口翻转、限高并夹紧位置。菜单保持独立浮层，不占文档流或增加 Composer 高度，不移动正文和工具栏。39 模型选择器代表页仍保留原型导航。共享组件按锚点规则运行，不保存逐页坐标，也不为显示 hover 克隆按钮。焦点、读屏、字体缩放与窄窗要在运行组件中验证。
状态：这些是组件引用与行为约定。当前首页不再通过逐项 overlay FRAME 演示；SmallPen 组件样本和 41 代表稿不执行键入、录音或任务动作。
验收：同一 Tooltip 换多种文字仍可换行/撑高；触发点能打开对应内容、关闭后回焦点；重复使用不会增加页面画板或改变任务状态。

运行校准（2026-10-23，交互业务审查）
原生已接：当前装配路由的方向键/Enter选择、选中焦点、受视口限制的滚动、Escape/外点与上下文切换关闭；disabled Send/Stop原因可通过hover/focus读取。当前Enter仍换行、显式Send提交，尚无发送键设置；不把待确认建议改成既定政策。
证据：apps/hub/desktop/workspace_*_desktop_test.go、apps/hub/runtime_identity_test.go；本轮相关12包581项测试/子测试通过。原生无窗口控件与实际OS操作分别报告，详见根MIGRATION.md。
输入区对齐修订（2026-10-23）：Editor四边均为8，底部工具栏高32；Send/Stop点击区32×32、圆角16、居中图标18。普通按钮保持36。验收：在卡片上方空间足够时打开@ /菜单，菜单下沿与卡片上沿相差6；开关菜单前后卡片高度、光标与工具栏位置保持。窄窗和近视口边缘分别检查翻转/夹紧与滚动。该契约及静态样本不代表真实OS焦点/IME验收完成。
运行工具栏Permission/Model标签使用对称上下4/左右12；SmallPen嵌套Instance不支持局部layout-padding覆盖，静态样本使用共享Button的居中布局与左右12，不声称两种表达方式逐像素一致，不改变全局Button、不用宽度补偿模拟padding。Add图标18，Permission/Model图标16、下箭头14、文字14/行高21。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_focus — 共用 Tooltip/菜单行为与 Attach、Permission、Model、Voice、Send、@、/ 的绑定表；文案/触发点不另画。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_focus — 共用 Tooltip/菜单行为与 Attach、Permission、Model、Voice、Send、@、/ 的绑定表；文案/触发点不另画。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_submit — 提交、回执与草稿

状态：2026-10-26 当前契约；静态原型不能证明执行、保存或结果成功。
前置：有效正文/服务允许的明确引用、附件就绪、模型/Agent可用、下一轮设置已保存，且无未完成提交、运行或等待。键盘Message中Enter/Shift+Enter换行，macOS⌘+Enter、其他平台Ctrl+Enter明确发送；IME和菜单先消费自己的按键，不截获其他表单。
提交：冻结正文、编辑版本、逻辑作用域、路由/分支、文件和命令来源，先保存这份草稿。首页在同一SQLite事务里创建会话并保存PendingSessionID；同一捕获快照重复创建只返回原会话，改动后的快照不能借旧回执。后续提交失败或重启仍找到同一会话，不重复创建，不自动Send。
准入：明确文件/Skill沿可选ComposerExecutionStore进入同一HubRuntime；Runtime执行存储继续六方法。原始用户正文与模型用的小索引/引用分别保存，聊天不显示内部文件路径清单。共享产品服务复用原有路由、权限、Plan和原件重验，不在桌面/HTTP重复业务规则。
回执：实际Turn准入成功才精确消费已捕获草稿revision；任何后续编辑（含改回相同文本）不能被旧回执清除。失败完整保留；未知结果先核对实际事实，不自动重发。提交期间的新编辑留作手动草稿，不自动排队/插话。
验收：双击只认领一次；首发创建后提交失败、重启重试仍是同一会话；新编辑与另一窗口草稿不被旧回执擦除；关闭/重启不执行。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_submit — Composer.Send：校验后提交，保护重复操作，按服务回执清理对应草稿版本。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_submit — Composer.Send：校验后提交，保护重复操作，按服务回执清理对应草稿版本。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_execution — 运行、等待、停止与错误

状态：RunStatus/Composer 静态变体已有；执行事实来自 Go owner。
运行：显示 Working；耗时/速率仅有真实数据时出现，未知不伪造 4s/60.8 tok/s。提交错误与执行错误分开，RunStatus.Error 默认“执行失败”。
停止：Stop 发出取消请求→Stopping，阻止重复停止；等 host/controller 真正结束进程与回调清理才显示 Stopped。关闭页面/取消观察不停止。
等待：耐久审批、问题、Other输入各自绑定请求ID/版本；先保存结果，再显式继续原Turn，普通发送不能绕过等待。外部native即时审批与人类提问是两个请求种类，响应使用原session/branch/request及execution/attempt/engine/native身份的opaque handle，不重新寻找替换owner；旧身份拒绝。原生owner接受响应后即从所有观察者的待办中隐藏，阻止重复回答；接受不伪造PermissionResolved/UserInputResolved，真实resolved事实仍由native事件提交。提交中防重、失败保留原问题草稿，迟到UI回执不覆盖后来会话。owner与来源：apps/hub/product/wait.go、apps/hub/product/engine_interactions.go、apps/hub/engine_interactions.go、apps/hub/desktop/workspace_engine_interactions_desktop.go及各自回归。
运行中补充：草稿可保留；送入当前任务、排队下一轮或新建任务的策略仍待确认。platform/host 的 PendingService 是可选能力，不可因为 UI 有输入框就假定全部引擎支持插话。
重试：用户显式触发；区分重试提交、开始新轮和继续原等待，不自动重放已发生的工具副作用。
验收：Stop 后不会马上假报结束；等待时按普通 Enter 不另开任务；执行失败不会误声称该请求从未被服务接收。

前轮运行校准（2026-10-23，交互业务审查；历史记录）
原生已接：真实完成/失败/已停止反馈。Stop绑定Runtime当前物理owner的不可变初始Attempt ID；自动fallback内不变，同Turn等待恢复后是新身份。CancelExecution原子校验owner，旧停止不作用于替换执行。切走再回保留Stopping，Cancel报错仍待收尾并只允许明确重试。跨窗口的统一Stopping持久状态没有新增。
证据：apps/hub/desktop/workspace_*_desktop_test.go、apps/hub/runtime_identity_test.go；本轮相关12包581项测试/子测试通过。原生无窗口控件与实际OS操作分别报告，详见根MIGRATION.md。

原生运行校准（2026-10-23，本波执行边界；本波回归已通过；真实 OS 验收另列）
@、/ 选择不创建 Turn、不执行；仅明确 Send 进入共享 product preparation 重审、必要的真实 Plan 设置和普通 Submit。等待/运行中的 route、权限与 recipe 沿原 owner 冻结，菜单不能代替 ResumeWait 或偷偷继续。Plan 下非读取调用被真实 Guard 拒绝并按现有 SDK 失败收尾；外部首次准入或自动 fallback 不能忽略已保存 Plan。Stop/cleanup owner、同 Turn 恢复与不自动重放副作用的既有边界保留。
验收状态：本波全仓 desktop-tagged race 已通过：96 个有测试包、2252 项测试/子测试、0 失败，耗时 100.12 秒；906 份 Go 源码/模块记录执行期间未变，全树 vet、mod verify 和构建通过。原生 headless 控件/布局证据不代替实际 OS 窗口、外部账户或跨平台验收；既有 owner 收尾与不重放约束保留。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_execution — Composer.Status/Send：执行事实和停止由 owner 回报；运行提示复用共享 Tooltip。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_execution — Composer.Status/Send：执行事实和停止由 owner 回报；运行提示复用共享 Tooltip。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_voice — 录音与转写

状态（2026-10-24）：macOS 系统本地录音与转写已接入代码；真实麦克风、系统许可与声学质量仍需用户点击设备验收。其他平台明确不支持，不退回远程服务。
触发：明确点击麦克风后检查 app 的麦克风/语音用途声明，请求系统许可，并验证 SFSpeechRecognizer 支持且实际启用 on-device recognition；无本地支持、许可拒绝或设备不可用都显示可恢复原因，不假装录音。默认 zh-CN、最多60秒，音频直接交系统请求，不生成音频文件、云请求或模型消息。
动作：停止录音结束采集后转写；停止任务是独立动作，两者可并存。录音/转写期间禁止普通 Send，编辑仍可继续。结果仅在原会话、原草稿未变化时追加；编辑/导航发生后保留为可显式采纳或丢弃的待定文字，绝不覆盖正文或自动发送。首页首次持久创建会话后，转写和该首页草稿一同迁移，不能落到未来的首页。
退出：切换会话或离开聊天页停止录音，保留原作用域的完成文字；取消/关窗会取消并等待实际 native task 和回调收尾，超时仍保留 owner。转写失败或超时取消任务；重试意味着用户明确重新开麦，不自动录音或重放音频。
布局：工具栏使用共用32px动作和固定图标槽；录音状态为同高计时/停止控件。错误/待采纳文字与附件共用有界滚动补充区，长文字和8项引用不能把 Send/任务 Stop 推出窗口。headless 的假 recorder 检查不代表真实设备验收。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_voice — Composer.Voice：点击控制录音；hover/focus 的提示复用 Alva Tooltip，文字见共享绑定表。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_voice — Composer.Voice：点击控制录音；hover/focus 的提示复用 Alva Tooltip，文字见共享绑定表。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_configuration — 模型与权限的作用域

状态：模型/权限是下一次明确Send的产品配置，不能由静态变体授予能力。本波Ask/Auto/Full与Plan的真实接线契约如下；实际回归和平台验收分别记录。
触发：Permission点击打开同一Alva PermissionMenu，hover/focus仅解释当前值；单选由当前有效策略打勾。Model点击使用39联合选择代表设计，运行菜单只列真实配置路由。更改保留文本/引用，重新校验模型/Agent/附件能力；不兼容时解释并保留内容，不默默改连接。
前置/禁用：首页仅改本窗下一轮意图，明确Send才创建会话并保存；已存Alva会话需先成功读取设置，空闲且无running/waiting/未放弃checkpoint才可经共享product.SetPermissionPolicy(ctx,id,policy,fullConfirmed)保存。读取失败/保存中禁改，错误保留原已提交策略和草稿。外部Agent没有等价策略时显示其实际固定政策，不冒充支持这些授权。
| 策略 | 本波实际范围与提示 |
|---|---|
| Ask 每次询问 | 读取按既有Guard放行；其他需要权限的操作逐次询问，只批准所示操作。 |
| Auto 自动审批 | 仅已知原生文件工具在已授权workspace内的文件写入自动批准；命令、网络、MCP和未知效果仍逐次询问。当前没有可信安全命令分类器，设计中的安全命令自动执行仍未完成，不以SDK AcceptEdits等同整套政策。 |
| Full 完全访问 | 先在当前同一popover显示真实作用域并显式确认，才保存。仅已启用能力在现有Guard的工作区、敏感路径和独立host批准限制内自动批准；不越界、不自动启用插件或继承别的账户授权。 |
确认/退出：点Full先进入同一popover的确认内容；取消/Esc/外点不改原值，恢复触发点焦点，只有确认提交成功才勾Full。普通关闭菜单零写入。保存失败不假装已选；切换设置不执行模型或工具。
Plan关系：Plan始终最高只读。切入Plan重置Ask；显式选择Ask/Auto/Full退出Plan，Full仍须范围确认。退出Plan不恢复旧Full，仍须明确Send；Plan只允许读取，非读取直接拒绝，不通过审批放行；Hooks/诊断/MCP/AEP/Shell/Web/Browser/Media/SubAgents本轮不装配。不能将Full与Plan同时显示成双重有效策略。
生效/owner：设置仅影响下一次明确Send，当前Attempt的route/recipe/权限快照不变；waiting恢复沿绑定快照，审批请求仍绑定原执行/请求ID。UI管理草稿、确认与焦点；共享product owner校验空闲/冻结并持久化Settings；bootstrap在实际native装配中消费，host审批handler落实分类，SDK Guard继续做真实范围约束。
模型与连接边界：连接/模型禁用影响后续选择及准入，不取消当前轮；模型优先选择、搜索/连接筛选、收藏、兼容Agent、参数草稿与真实默认值Save/Reload已接。模型默认以独立revision保存，不修改连接授权；实际Attempt冻结参数，waiting恢复仍沿原快照。Context上限缺少可信声明，行内保持未声明/禁用；当前会话参数通过独立CAS覆盖接线，下一次新Turn生效，运行/Waiting恢复沿用原Attempt冻结参数；未读取/失败不退成默认，旧poll不覆盖新Save，不用模型默认冒充会话覆盖。候选来自实际adapter/目录，不凭名字推兼容，失效说明原因而非暗换账户。
验收：hover/开关菜单零写入；Full取消保持原值，确认回执后重开可见；Auto工作区文件写入可自动、命令/网络/MCP/unknown仍问；敏感/越界不因Full放行；Plan禁止写/进程/网络，退出不恢复旧Full；running/wait/checkpoint冻结；新配置不影响当前owner；失败/冲突保留草稿和真实状态。打开/关闭/焦点/定位复用req_alva_composer_focus，不新增screen/Token。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_configuration — Permission：下一Turn Ask/Auto/Full；Full在同popover显式确认，Plan最高只读；共享product保存、bootstrap/host执行，菜单不授予越界能力。Model引用39代表设计与真实可用路由。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_configuration — Permission：下一Turn Ask/Auto/Full；Full在同popover显式确认，Plan最高只读；共享product保存、bootstrap/host执行，菜单不授予越界能力。Model引用39代表设计与真实可用路由。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

## req_alva_composer_coverage — 首页剩余功能与验收缺口

状态：2026-10-26 用户确认按模块交付，当前首先打通首页/会话输入整条链。功能规则在原生requirements，视觉在Foundation Token/组件/变体，落地与验收在代码、HUB-FLOWS及MIGRATION；不复制业务逻辑为图形状态大全。
当前版本主线：工作台/会话；正文/任意本机文件/@/slash/项目Skill/优先macOS本地语音；模型连接与参数；独立审批和提问；执行、Stop、失败/重开；General主题、字号和减弱动效。
本机Codex：明确选择本地CLI及其配置/登录状态，使用app-server和真实同任务回调，不造产品登录、复制授权或另管账户；Ask/Auto/Full不冒充厂商sandbox/policy。Claude只验证实现和fixture，不调用真实账户。后续其他engine接同一根契约。
运行中输入仅手动草稿，无自动排队或插话。General语言/密度暂固定并隐藏未生效入口。伙伴/群、Team/Worker/自动化、Gateway/开发者工具保留已有代码与事实，作为后续模块继续对齐，不删除功能。
首轮关闭条件：可重复的发送/失败/审批/提问/Stop/文件读取/跨会话/跨窗/重启流程，符合既有组件约束的实际原生画面，以及一小时持续运行和内存证据。构建、自动测试、headless像素、真实OS窗口/IME/设备、本地Codex真实调用分别报告，未实际运行的一项不得写通过。PDF/图片格式能力按实际处理回执报告，文件捕获不等于解析成功。
验收进度：本轮实现与集中检查进行中；旧轮次截图和测试数保留在历史记录，不作为本轮关闭依据。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`

标注：
- [behavior-contract] req_alva_composer_coverage — 首页未完成功能及未定项清单；不以组件样本或备注冒充运行实现。 — `pages/工作台/21 新任务.html` → `Main / ConversationSurface / Body / Conversation / Welcome / WelcomeContent / Composer`
- [behavior-contract] req_alva_composer_coverage — 首页未完成功能及未定项清单；不以组件样本或备注冒充运行实现。 — `pages/工作台/22 对话与面板.html` → `Main / ConversationSurface / Body / Conversation / Composer`
