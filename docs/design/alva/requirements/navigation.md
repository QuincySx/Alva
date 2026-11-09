# navigation

> 来源：旧包 `alva.smallpen/requirements/navigation.json`。本文件是这些需求的正本；页面位置按新包路径和元素 `data-name` 链给出。

## req_alva_navigation_settings — 设置返回工作台

状态：主设置页头 HeaderReturn（复用 Ghost/Leading Button）是既有设计契约，原生General/Developer及本波Models and connections复用同一设置壳；静态原型只演示页面跳转，真实OS焦点验收另列。
触发：主设置页头左上箭头+返回工作台，高36，与原生标题栏按钮中心同线；设置分类切换不是退出，不用浏览器历史逐类后退。首次进入设置记录原工作台/会话及视图来源，分类切换不覆盖来源。
动作/owner：UI只改变视图，保留会话/文本/引用草稿、Inspector位置与原执行owner；导航不创建会话、不保存配置、不测试模型、不Stop。媒体离开仍由原owner真实join并释放正文，返回只恢复原冷索引位置，不自动恢复播放。
未保存：General与连接表单各自保留本窗草稿；明确Save才写共享产品服务，显式Reload读取已保存值并替换本窗编辑，冲突/失败不得冒称已保存。连接Cancel是明确放弃未存表单；一般设置分类导航不等同Cancel。保存未实际返回时禁止关闭承载窗口，不能把隐藏/退订当已完成写入。
禁用/异常：缺少对应共享服务的分类说明不可用；读失败不显示空成功、不启用依赖旧revision的写入。原入口会话/视图已不匹配时安全回工作台，不恢复另一会话的私有定位。
验收：通用→模型→开发者连续切换，左上始终可返回最初工作台/会话；草稿/Inspector位置仍在，任务无Stop；媒体没有自动播放。General的CAS失败/跨窗同步与连接CRUD分别用各自共享owner回归，不能由原型跳转代替。

绑定位置：
- `pages/设置/33 规则与 Skills.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/36 Gateway.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/35 权限与安全.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/32 插件与工具.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/37 开发者测试.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/31 通用.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/34 数据与存储.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/23 模型与连接.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/38 开发者评估.html` → `SettingsRegion / ReturnRow / HeaderReturn`

标注：
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/33 规则与 Skills.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/36 Gateway.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/35 权限与安全.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/32 插件与工具.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/37 开发者测试.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/31 通用.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/34 数据与存储.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/23 模型与连接.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- [behavior-contract] req_alva_navigation_settings — HeaderReturn 位于导航最上方，箭头+返回工作台。静态原型回22；运行产品回打开设置前的工作台/会话，不停止任务，不用浏览器历史逐项退设置分类。 — `pages/设置/38 开发者评估.html` → `SettingsRegion / ReturnRow / HeaderReturn`

## req_alva_navigation_wizard — 连接表单的返回与退出

状态：24来源选择+25/26共用表单，静态连线不执行网络/账户检查/保存。新建和编辑以caller/mode/稳定ID/revision及返回目标区分，不靠标题推断。
新建：23 Add→24；OpenAI/Anthropic/Gemini预设进入25同一结构，Custom/Local进入26同一Custom结构。24左上返回23；25/26左上Back→24且保留同一未存草稿，二者不是顺次第二/三步，没有必须Fetch模型的第三步。Back不测试、不保存、不创建会话。
编辑：23连接设置→原ProviderForm，带已有ID/revision/值；返回/Cancel直接回23，不回来源选择，不分配新ID。Key输入为空表示保留已存Key；Cancel释放本窗新秘密草稿、不清除持久凭据。
保存：显式Save经共享product连接服务一次提交字段和手填模型；只有成功回执才回23并定位该ID。失败保留表单与可恢复内容，冲突要求Reload，不自动覆盖；忙碌时禁重入与关闭，等待实际回执。
取消：创建Cancel/关闭明确丢弃本次未保存表单回23；编辑Cancel只放弃修改回23。取消不是上一页，不能保存半成品、删除既存连接或Stop当前任务。普通设置分类切换保留本窗草稿，不等同Cancel。
来源切换：创建Back回24后再次选择同一来源保留全部未存草稿；选择不同来源按新预设重新初始化本次表单，丢弃前来源未存字段与秘密，不写任何持久配置、不跨账户迁移Key。这是本波实际行为，不声称存在每来源独立草稿；真正Cancel才清空本次草稿，普通设置分类切换/返回工作台保留本窗编辑便于继续。
owner：UI持有导航来源、mode与未存草稿；共享product服务校验ID/revision并提交；bootstrap装配路由和存储。原型25仅代表共用布局，不证明运行时创建/编辑或焦点保持。
验收：24→25/26→24→原来源草稿保持；编辑返回仍同一ID；Save失败留原页、成功才回管理；Cancel零写入；分类切换/返回工作台不Stop执行。切换不同来源不把旧Key/headers带入新账户，未保存草稿重置与同来源保留分别覆盖。

窗口壳：24/25/26复用req_alva_navigation_native_shell的native-safe顶部36返回行，保留原WizardBack节点ID和既有返回目标，移出居中卡片而非复制按钮；卡片在返回行下的剩余区域居中。运行端pending问答也在该行之后，不能覆盖系统按钮区。保存中沿原owner禁用返回/关闭。

绑定位置：
- `pages/连接/24 选择供应商.html` → `SheetReturnRow / WizardBack`
- `pages/连接/25 配置连接.html` → `SheetReturnRow / WizardBack`
- `pages/连接/26 自定义连接.html` → `SheetReturnRow / WizardBack`

标注：
- [behavior-contract] req_alva_navigation_wizard — 创建Back回24保留草稿；编辑Back/Cancel回23保持ID；Save成功回管理，Cancel放弃未存表单，分类导航不等同Cancel。 — `pages/连接/25 配置连接.html` → `SheetReturnRow / WizardBack`
- [behavior-contract] req_alva_navigation_wizard — 创建Back回24保留草稿；编辑Back/Cancel回23保持ID；Save成功回管理，Cancel放弃未存表单，分类导航不等同Cancel。 — `pages/连接/26 自定义连接.html` → `SheetReturnRow / WizardBack`
- [behavior-contract] req_alva_navigation_wizard — 返回模型与连接→23；来源选择即进表单，无第三步。 — `pages/连接/24 选择供应商.html` → `SheetReturnRow / WizardBack`

## req_alva_navigation_native_shell — 原生标题栏与可隐藏导航壳

状态：2026-11-06 用户已批准 B3 并授权正式回写。本节是工作台导航、标题行、会话标签与开关的单一权威正文；与旧静态样例冲突处以本节为准，设置返回、草稿和审批政策仍由既有设置/资源 requirements 维护。

窗口壳：静态样例使用 36 高原生工具行，按钮中心 y26；左右/底部/栏间与工具行下沿到内容卡片均绑定现有 ui.space.2（实际 8）。顶部安全区读取 MyGo TitleBarHiddenInset 的真实 c.TitleBar.Left/Right/Height，不自绘红绿灯，不把 ui.size.header 误绑成 52。左开关保持在原生安全区右侧或左栏右上两个既定端点；整个窗口只有一个右栏 Panel 开关，在右栏展开时位于右栏 header，关闭时保持窗口最右同一边缘锚点。More 只在右栏展开时出现。

工作台：侧栏严格为“仓库 → Worktree”两级，不展开第三级 Session。每个 Worktree 独立保存并恢复自己的打开会话 Tab 集合、顺序和当前项；点击 Worktree 直接恢复该 Worktree 上次由用户选择的会话，不先要求挑选 Session。标题行使用该 Worktree 当前 Agent 会话 Tab（28 高、16 图标、13/11 字号），加号紧跟 Tab；右侧一个下拉入口承载全部会话、历史与已归档会话找回，不保留永久“全部标签/归档”文字按钮。+ 只创建独立新草稿；Send 前不建立 HubSession、不启动执行。关闭或归档 Tab 只移出视图并保留 Worktree；不 Stop、不删除 Worktree。“历史”下拉可找回原会话。后台 UpdatedAt 更新不得抢占当前选择或重新打开已归档会话；只有用户明确恢复时才重开。长标题优先保留关闭位置；运行端应自动省略并提供完整标题 tooltip，静态样例仅用短标题与省略号，tooltip 行为尚待平台验收。

普通聊天/普通目录项目：标题行使用同高 36 单标题，不显示会话 Tab、不增加第二/第三行目录副标题；正文仍是标题行下 gap 8 的圆角内容面板。目录/工作区来源信息归侧栏、菜单或项目入口，不占用标题行。

视觉：Main/RightWorkspace 外层透明；标题行独立在窗口 chrome 上。中心 ConversationSurface 使用既有 ui.color.surface + ui.radius.card，无描边；左 SidebarSurface 与右 ResourceSurface 使用既有 ui.color.subtle + ui.radius.card + 内侧 1px ui.color.border。两侧同结构，不新增 Token、手写颜色或装饰。右侧资源 Tab 的关闭/More 只影响资料视图；关闭右栏不归档会话、不 Stop，也不改变左栏。所有静态颜色、圆角、间距和几何只证明设计版本，不证明 App、OS 红绿灯、动画、IME、焦点或真实会话恢复已通过。

全局搜索（2026-11-06 用户确认）：状态：设计契约，运行端尚未实现；不为它新增页面或状态画板。
触发：左栏工具行的 Search 或 Cmd+K 在窗口正中打开同一个统一 Dialog；两侧栏、标题行与内容面板保持原状，不新建第二入口、不跳转独立页面。当前查询真实会话标题与目录（有界、分页、取消旧查询，迟到结果不覆盖新查询）；未来设置、命令与资料复用同一入口同一结果行样式，不复制多套弹层。
边界：打开/取消保持原页面、侧栏过滤状态、左右栏展开状态与草稿；只有明确选择结果才导航到该会话。搜索本身零模型、零工具执行、不改会话状态。加载、空、错误、快速改词与关闭后重开的旧回执互相隔离，迟到响应不能覆盖新查询或重新打开已关闭的弹层。一般 Dialog 复用现有基础样式（含 21/22 标题行与工具行锚点），不新增颜色、尺寸 Token 或组件。
验收：从 21 与 22 打开搜索后取消，原页/草稿/栏位不变；输入 A 后输入 B，A 的迟到结果不覆盖 B；未选择结果时零会话、零模型执行；选择结果只导航到被选会话。运行端此行为尚未验收，本节只固定设计与契约。

绑定位置：
- `pages/工作台/21 新任务.html` → `Main / PageTitleRow`
- `pages/工作台/22 对话与面板.html` → `Main / WorkspaceTabs`
- `pages/设置/23 模型与连接.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/连接/24 选择供应商.html` → `SheetReturnRow / WizardBack`
- `pages/连接/25 配置连接.html` → `SheetReturnRow / WizardBack`
- `pages/连接/26 自定义连接.html` → `SheetReturnRow / WizardBack`
- `pages/设置/31 通用.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/32 插件与工具.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/33 规则与 Skills.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/34 数据与存储.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/35 权限与安全.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/36 Gateway.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/37 开发者测试.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/设置/38 开发者评估.html` → `SettingsRegion / ReturnRow / HeaderReturn`
- `pages/工作台/21 新任务.html` → `Sidebar / SidebarTools / SearchAction`
- `pages/工作台/22 对话与面板.html` → `Sidebar / SidebarTools / SearchAction`

标注：
- [behavior-contract] req_alva_navigation_native_shell — 36工具行、动态native-safe避让、导航真隐藏与PanelLeft开关位置。 — `pages/工作台/21 新任务.html` → `Main / PageTitleRow`
- [behavior-contract] req_alva_navigation_native_shell — 36工具行、动态native-safe避让、导航真隐藏与PanelLeft开关位置。 — `pages/工作台/22 对话与面板.html` → `Main / WorkspaceTabs`
- [behavior-contract] req_alva_navigation_native_shell — SearchAction 打开窗口正中统一搜索 Dialog，不新增页面；当前查真实会话/目录，未来复用同入口。 — `pages/工作台/21 新任务.html` → `Sidebar / SidebarTools / SearchAction`
- [behavior-contract] req_alva_navigation_native_shell — 同一 SearchAction 契约；取消保留原页、栏位与草稿，选择结果才导航。 — `pages/工作台/22 对话与面板.html` → `Sidebar / SidebarTools / SearchAction`
