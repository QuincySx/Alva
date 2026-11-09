# Alva 桌面原型与界面契约

状态：**基础规范与组件复用、功能 SPEC、按需组合预览**，2026-10-26。保留已确认线稿和正式页面；页面维护与可点击原型按具体请求交付。这里定义目标界面及其规范入口，实际产品进度见根 MIGRATION。

AI 接手或使用 CLI 修改前先读 [SmallPen CLI 使用与踩坑指南](./CLI-GUIDE.md)：区分 Token、实例内容、布局与必要变体，避免再次全排列扩张；下文的保存状态和历史验收不代表这些建模问题都已完成收敛。

2026-11-05工作台还原复核：本轮修原生实现，Foundation/Product保存包保持下列正式修订。核对范围补齐21/22的导航层级、8类设置、真实会话消息与同时打开的右侧文档；旧首页与局部控件通过不能视为整页还原。明暗宽窄整页、实际Mac导航/设置/左右栏及一次真实Codex只读请求分别验证，同名文件采用真实来源身份避免条目合并。MyGo标题行框仍有明确差异，可访问性异常仍单列；具体结果见根[MIGRATION](../../../MIGRATION.md)，不修改设计审查记录来替实现验收。

## 已保存正本：新任务首页、窄窗普通聊天与全局搜索契约 · 2026-11-06

用户批准 B3 后完成语义纠正。Product 由 `7f4ce71f7576aed7f6b13de41c9824cd71bea500cd328a4efb5804f7656ed9f1` 前进到
`2854e314e6e451feac570500c2a53c95b81845dffd3064f5d851125c8947a8a5`（typed batch `alva-b4-welcome-and-narrow-v2`，3 个文件、
193 个受影响 ID，inverse 备份见 `/tmp/alva-formal-b3-rewrite-20261106/work/b4.inverse.json`）；Foundation 保持
`f25a54ab61abbbff71b7095d84f24599ef46a205709f0468bd857713744ee2bd` 未改，无新 Token、无新组件、无新 Screen。

- 21 `scr_21_u3cb59444` 恢复为 1440×960 的「新任务」首页：保留原 root/`Header`/`Composer` 锚点 ID 与 prototypeFlow，
  Composer 回到 `State=Idle`（占位「描述你要完成的任务…」），纸面内保留原 Welcome 品牌、标题、说明与三条建议；
  不再有 UserText/AssistantBody/LoginPlan 或 Running Composer。B3 左栏工具行承载搜索/新建/收起，
  主标题行只在纸面外单标题＋一个右栏开关；左栏内嵌原 Alva Sidebar（Area=New）作为 Navigation，
  隐藏其重复 Brand/新任务/搜索，内边距改用现有 8px spacing Token，不拆包不重画 Popover。
- 22 `scr_22_u7e7bb1f2` 的 desktop 保持 93 节点的 B3 开发者主版本（仓库→Worktree 两级、顶部会话 Tabs、
  base presentation 不变）；同一 Screen 新增 `pres_22_u7e7bb1f2_narrow`（960×960，44 节点，独立 ID），
  承载已批准 B3 折叠 ChatHeader（原生安全占位＋唯一左栏重开开关＋单标题＋固定右开关）与进行中正文。
- 几何核对同次 solve：root 四边 8、列间 8、工具行 36、Header 下沿到 surface 8；1440×960 三 surface y52–952，
  x8 w220 / x236 / x1000 w432；narrow 纸面 x8 y52 w944 h900。原生控件只保留平台安全事实注释，未自绘红绿灯。
- 全局搜索写入现有 `req_alva_navigation_native_shell` 单一正文并绑定 21/22 工具行 SearchAction 锚点：
  左栏 Search 或 Cmd+K 打开窗口正中同一 Dialog，当前查真实会话标题/目录（有界分页、取消旧查询），
  未来设置/命令复用同入口；打开/取消保留原页、侧栏过滤、栏位与草稿，只有选择结果才导航，零模型执行。
  未新增搜索页面或状态画板。
- 证据：`exports/archive/workspace-tabs-reference-2026-11-06/formal/` 为保存后 fresh 原生导出
  （developer Light/Dark 1440×960、narrow 960×960、21 新任务供审阅），四份 revision/kernelHash 同为
  `2854e314…`/`5103ac82…`，issueCount 0；scoped validate 覆盖 21 desktop 与 22 narrow，text-contrast 仍按工具原因跳过。
  三张前台 PNG 与该副本字节一致。这些只证明已保存正本的几何与外观，不证明运行 App 或业务验收；
  运行端进度仍以根 [MIGRATION](../../../MIGRATION.md) 为准。

## 工作区会话标签与归档方向 · 2026-11-06

用户已确认：开发项目侧栏保持「仓库 → 工作区」，同一 Worktree 的独立 Agent 对话在工作区内以编辑器式 Tab 呈现；可以打开多个相同或不同 Agent 的会话，不为每个 Agent 增加树层级。此处记录已确认的交互方向，正式 SmallPen requirement/页面与生产功能尚未更新；[可点击线稿](./exports/archive/workspace-navigation-prototype-2026-11-06/navigation-wireframes.html)仅使用示例数据，视觉仍待审阅。

- 新建直接增加一个会话 Tab，保留已打开的其他 Tab；新建未发送时仍是草稿，不提前创建 HubSession。
- 会话 Tab 直接占用原有顶部标题栏，桌面保持单行36px，与左栏工具行同高；Worktree名称/分支留在侧栏及上下文信息中，不再另占标题行，也不在Tab下重复当前会话标题。＋、全部标签和归档同处这一行，Agent选择保留在输入区。
- 关闭会话 Tab 即归档，从本工作区活跃 Tab 集合移出；可经本工作区「已归档」入口搜索、重新打开同一会话。后台更新不会自动恢复已归档 Tab。未发送草稿保持草稿身份，不能伪造空会话。
- 切换 Worktree 后恢复该工作区的 Tab 集合与上次用户选中的 Tab。归档不隐式停止 Agent；运行、审批与问题仍须在集中入口可见，停止继续由真实执行 owner 负责。
- 不设置很小的产品 Tab 数量上限。大量标签采用单行、有界可见区与全部标签查找，不换成多行或持续压缩字号；打开数量与实际执行并发分开。原生实现须按需载入正文和视图，不为每个打开的 Tab 常驻完整页面、轮询器或独立进程。

## 当前：一套正常信息密度 · 2026-11-06

用户明确要求按钮、图标和字号同时收紧，先做好一套正常密度，不新增大中小模式。现有Token原位更新，Foundation `f25a54ab61abbbff71b7095d84f24599ef46a205709f0468bd857713744ee2bd`、Product `e174b150043139f25591271efbc01006f1709eea007c2fb75faea2b6bb267c1f`；66组件/533变体/151 Tokens/204值、72页/599连线不增。旧密度主题都映射到同一正常值，入口仍不额外暴露；不通过整页Zoom或DPI缩放制造紧凑感。

界面body13、caption11、subtitle14、heading20、display28；普通按钮/Input/Select32，IconButton28，图标16/小图标14，导航行28，普通行高1.5/阅读1.6。原生窗口工具行仍36，较小按钮双轴居中；外距12、三栏16圆角、左移/右固定运动规则保留。值在既有Token及运行端`workspace_metrics_desktop.go`集中维护，不为某个页面新增尺寸Token。已保存正文字号和系统TextScale继续作用于阅读theme，不让正文选项重新放大固定的UI标签；未修改任何用户偏好。

独立任务选择区只保留主头标题和顶部返回，搜索/列表限宽760，取消重复大标题和显然的操作说明；嵌入设置的选择器保留局部返回。既有选择→匹配Detail→明确打开、搜索/分页/失败/取消政策保持。附件和模型字段随内容hug，编辑区保持原对称8内距，/与@、权限/模型弹层继续走原锚点。

相同40条fixture、相同窗口及原生band：1240×850完整任务行11→16，1547×932为14→19；旧源通过Go overlay运行，并未安装旧客户端或写入用户任务。新旧各4张实际MyGo图分别留存；原生SmallPen Sidebar4、Input28及Composer14目标无报告问题，对比度跳过仍列明。实际Mac已打开当前普通发布，CUA随后因用户状态变化拒绝指针动作；不把无窗口任务选择验收标成实机通过。完整测试/安装/限制见[实施证据](./exports/archive/density-2026-11-05/implementation-evidence.json)与根[MIGRATION](../../../MIGRATION.md)。下文保留旧修订及其实际检查，不重新标记旧PNG。

## 当前：三栏圆角、独立开合与原生过渡 · 2026-11-05

用户明确要求“直接改”，本轮转入运行端实施，不再等待静态图视觉确认。共享定义及22代表布局已保存回正式包：Foundation `1268268b4445af4409543fe20164cdd255160cb9e4e2031a094446f0dd91e94c`，Product `d8fb60ba7192050c123b3fc6c095e37a4eff5af280a214dca086d958507eca1b`。以下本日早先的窗口壳记录保留历史修订，本节与原生requirements是当前规则入口；较早导出的图片不改标为新revision。

左侧32开关展开时在左栏右上角，收起时移动到红绿灯右侧；Main外框与标题同步向左，标题最终排在开关右侧8。展开反向。右开关始终固定于窗口右上安全区，独立于Main/RightWorkspace动画父容器，每帧不漂移；右栏打开才显示标签/Add/More，收起后只留开关。右侧关闭时只显示展开入口；分栏时Main外框实际收窄，右栏与Main同级，顶部复用36 Tabbar、32动作，More和资料弹出/收回操作归入右栏。低于既有阅读宽度时保留资源专注，窗口壳保留同一收起动作，主工具行/Tabbar只预留相应32槽，不重复构建开关；点击右侧入口不退出当前业务面板，也不自动折叠左栏。已有保存视图继续恢复，新空视图需明确打开；首页只开空右栏，选择任务是单独动作。

左/中/右复用既有16圆角；左右subtle、中间surface，外距和区域间隔仍12，不增加圆角档位或装饰描边。开合使用MyGo原生160ms位置/尺寸过渡，两个开关均由不动的窗口壳持有稳定控件身份；只有左开关的位置参与过渡，支持中途反向；标题局部位置与主框同步，避免先向右跳；原生退出快照只持有构建时捕获的绘制值，不跨帧读取已过期Element。已提交的减弱动效偏好与系统要求取或，立即切换；连续拖动分隔器直接跟手，动画结束不保留常驻刷新。

macOS使用MyGo原生TrafficLightPosition=(16,19)，由更新后的真实TitleBar安全区计算工具行位置；不再认为TitleBarHeight配置macOS红绿灯。静态52/86样例只表达布局，不是系统实测值。保存与入口迁移没有新增Token、组件、变体或页面。原生SPEC沿用 `req_alva_navigation_native_shell`、`req_alva_resources_layout`，精确保存回执见[formal-save.json](./exports/archive/pane-controls-2026-11-05/formal-save.json)。源、测试、安装和真实窗口验收分别见根[MIGRATION](../../../MIGRATION.md)，不从设计写入推断已运行通过。

## 本轮较早的开合候选 · 2026-11-05

用户指出左开关折叠后与红绿灯不齐、右侧入口与More位置不合理。本轮以当前正式两包为基准准备[可点击开合比较](./exports/archive/pane-controls-2026-11-05/review.html)及[右栏展开稿](./exports/archive/pane-controls-2026-11-05/expanded-open.png)：左开关两态保持原生安全区右侧的同一锚点；右侧关闭时只留Panel，展开后收窄Main，在同级右栏显示文件/任务Tabbar及More。候选复用既有组件与尺寸，行为补充写在候选原生requirements中。此段记录当时待审状态；用户随后授权直接实施，现已由上节正式定义及运行端取代。旧图保留原revision，不能将旧图视为最新三栏/动效证据。

当前MyGo支持原生TrafficLightPosition；TitleBarHeight不配置macOS红绿灯。实际系统按钮位置仍需后续窗口校准，设计样例不代替OS证据。来源、四种组合及明暗/960/首页的原生检查见[本轮记录](./exports/archive/pane-controls-2026-11-05/report.md)与 `review.json.currentPaneControlsReview`。以下保留该轮已保存的历史基准。

## 已保存的原生内嵌标题栏与导航壳 · 2026-11-05

用户确认参考 Hergo 的顶部结构，保留 Alva 配色与组件。正式 Foundation 为 `25e27c5ec18d7f7f9ff6f42cf002d4bc38869f3a6ebb30eabb8891b6ecd56cc3`，Product 为 `2dbf464cac3cfc7a9a61435ffc2690760a5227cd802be39e1e4d3f53e96605f7`。本节保留该轮窗口壳定义；当前使用上方三栏/开合规则，下方本日早先的自绘32窗口条方案已经退出使用。行为正本是 Product 原生 `requirements/navigation.json` 的 `req_alva_navigation_native_shell`，返回行为仍引用既有 settings/wizard 规则，不复制业务政策。

MyGo 使用 `TitleBarHiddenInset` 保留 OS 原生关闭/最小化/缩放按钮，实时读取 `c.TitleBar.Left/Right/Height`。主 `PageHeader` 和侧栏顶部共用既有36工具行/32图标动作，标题16与来源12横排居中；内距对称。原生栏有效时顶部为 `max(0,(max(native.Height,36)-36)/2)`，没有原生事实时沿用12；左右、底部和区域间隔12。52高/Left86/Right0的 macOS 样例得到顶部8，两个工具行中心均为26。原生避让为动态系统事实，样例空白留位不产生 Token、平台变体或假系统按钮。

展开侧栏220宽，顶部留原生安全区后由 Grow 推动 PanelLeft32 开关靠右；收起时整棵侧栏及分隔器退出布局/焦点/命中，主区移到 x12，开关进入 `PageHeader.Navigation=Collapsed`。页头基本左右24；样例左内距 `max(24,86+8-12)=82`，开关实际 x94。Narrow主题的展开侧栏也保持220与完整标签，SettingsNav也保持208；窄宽度不再偷偷切成72 rail。右侧资源 `PanelHeader` 保留52。

SettingsNav 顶部36只避让并显示16“设置”，`HeaderReturn` 在主设置页头36作为唯一返回入口。旧 source child ID 留存但隐藏、移除旧触发，新视觉节点与原 interaction/requirement 关联明确迁移。24/25/26沿用原 `WizardBack` 节点 ID，将它移到 native-safe 顶部36行，卡片在其下剩余区域居中；运行端 pending 同样归入下方内容，保存中禁用策略仍由原 owner 负责。

平台映射：macOS Right0时 host 仍为 y8..44 的36工具行；Win/Linux若 native.Right 的系统按钮点击区覆盖 y0..52，内部工具行保持 y8..44/中心26，host 底部再留8直至 y52，让正文不进入原生按钮点击带。无原生事实时顶部12。该 owner 适配使用实时几何，不增加组件变体，也不改变本轮 macOS 原生导出或冻结修订。

[展开浅色1024](./exports/archive/native-shell-2026-11-05/conversation-expanded-narrow.png)、[展开深色1440](./exports/archive/native-shell-2026-11-05/conversation-expanded-dark.png)、[收起浅色960](./exports/archive/native-shell-2026-11-05/conversation-collapsed-min960.png)、[收起深色960](./exports/archive/native-shell-2026-11-05/conversation-collapsed-min960-dark.png)、[设置返回](./exports/archive/native-shell-2026-11-05/settings-return-narrow.png)、[连接返回](./exports/archive/native-shell-2026-11-05/connection-return-narrow.png)。收起两图是以正式 Product `2dbf464…` 为 base 的隔离组合 candidate `5cf033732e9e28d86086975f933007386613b690774793d46e41c776a9e2e4d6`，不是正式业务页的新版本；仅临时设置960视口、隐藏侧栏并选择正式 Collapsed 变体。逐图来源和 layout hash 分别记录，不能把六图都标成正式 revision。数据/视觉检查与 OS 实际窗口和 Go 实施分开，详见[本轮记录](./exports/archive/native-shell-2026-11-05/report.md)及 `review.json.currentNativeShellReview`。

补充[960展开资源专注组合](./exports/archive/native-shell-2026-11-05/resource-focus-expanded-min960.png)：它以同一正式Product为base，隔离candidate为 `045f96f5f2869d9a0753d242e51428e3d70d38305a4fa545739ebaec1cbfa08c`。1024展开双栏图仅是静态装配检查；运行端既有conversationMin432和资源专注政策继续生效，不能为该图放宽Go阅读宽度。此补充不新增正式页、变体或Token，不改变冻结修订。

## 已保存的共享输入家族 · 2026-11-04

用户以“可以啊，相当不错呀。来，你先画上”确认输入家族候选，现已合并到正式 Foundation。既有 Input/TextArea/Field/Composer：普通输入使用 subtle 填充及 border 弱边界，Field 标签改为 regular；大编辑器保留 surface 写作面并去除阴影，让内部圆形发送动作仍有层次。保留原尺寸、对称 padding、双轴对齐、焦点/错误描边和业务变体；禁用编辑器内仍可用的模型/权限入口不随容器变淡。没有新增 Token、组件、变体或页面；所有引用这些共享定义的页面继承更新。

[表单浅色](./exports/archive/input-family-2026-11-04/profile-light.png)、[表单深色](./exports/archive/input-family-2026-11-04/profile-dark.png)、[编辑器](./exports/archive/input-family-2026-11-04/home-light.png)及[记录](./exports/archive/input-family-2026-11-04/report.md)。正式 Foundation revision 为 `843480b8e9e0ca197c929aff59707cb88c3f0d7d601d302cdb1bbf468a35349d`，Product 仍为 `0c285b1d5e710f87559a6b84c6bb8b312ba1cbe26a73981a6ed23e8e488411f0`；两包与已审副本逐文件字节一致，原生图无需改标修订。该增量补充下方边界基准，源默认规则已同步，43128 Web 已重新打开正本。2026-11-05原生共享颜色/Input/TextArea/Password/Select与主壳已消费该正式定义，原尺寸/对称padding保留；普通输入subtle、焦点surface，Primary text/surface，窗口栏独立验收。共享Disabled保留MyGo真实0.5透明度与设计0.6的明确差异。11张无窗口图与运行中自身首帧通过各自审查，实际窗口交互仍未通过；详见根[MIGRATION](../../../MIGRATION.md)。保存包和 `review.json.currentInputFamilyReview` 仍记录原设计审查修订，不改写为生产平台通过。

## 历史自绘窗口层 · 2026-11-05（已由原生内嵌壳取代）

以下记录本日较早的方案；当前应使用上方已保存的原生内嵌标题栏与导航壳，不从本段恢复自绘系统按钮或独立32空条。

用户明确要求处理现有窗口顶部。Hub原生窗口采用MyGo v0.7.4的 `TitleBarHidden` + `HideWindowButtons`，Go自绘窗口区域复用32px工具栏尺度；保留系统圆角、阴影和缩放边缘，不增加新的尺寸或状态目录。macOS按钮在左侧，提供关闭/最小化/切换全屏；其他平台按钮在右侧，提供最小化/最大化还原/关闭，最大化动作标记 `MaximizeButton` 以接入Windows Snap。空白区域通过 `DragWindow` 拖拽和处理标题栏双击，交互按钮独立命中，关闭仍调用原窗口守卫。

窗口动作不创建会话、不授予权限或Stop执行。当前实现入口是 `apps/hub/desktop/window_chrome_desktop.go` 与 `windows_desktop.go`，来源为[上游窗口接口](https://github.com/egoist/mygo/blob/v0.7.4/window.go#L91)。实际Mac窗口及回归记录见根[MIGRATION](../../../MIGRATION.md)；其他平台实机、完整生产样式和业务验收分别记录，不因共享设计图或依赖升级自动通过。窗口层使用自身原生宿主，没有给每个设置页复制一套标题栏画板。

## 当前正式视觉基准 · 2026-11-04

本节是当前入口。下方候选与历史记录保留各自修订；不能从旧图或旧表格重新拼出另一套现行规范。

用户以“舒服”确认边界统一候选，并以“那你开始啊”授权继续。该方向现已合并到原来的 Foundation/Product 两包及21–43共23张正式页面，仍位于 **01 产品界面**；不是另一套独立待用稿。入口示例：[首页](./exports/archive/modules-2026-11-04/pages/21-light.png)、[会话](./exports/archive/modules-2026-11-04/pages/22-light.png)、[伙伴群聊](./exports/archive/modules-2026-11-04/pages/29-light.png)、[模型与连接](./exports/archive/modules-2026-11-04/pages/23-light.png)、[通用设置](./exports/archive/modules-2026-11-04/pages/31-light.png)。完整页面索引及明暗/宽窄证据见[本轮记录](./exports/archive/modules-2026-11-04/report.md)。

后续维护沿用这些规则，具体值与变体以当前 Foundation 保存定义为准：

- 工作区、阅读面、输入区复用16圆角；内部控件使用既有8圆角，头像和发送动作按角色保留圆形。不因多画一个区块增加尺寸档位。
- 窗口与侧栏底色连贯，主要阅读区使用 surface；资料阅读面内收并使用 subtle。以既有12/24的外层间隔和内容内距分区，不重新添加通栏分割线、执行轨迹线或段落装饰左线。
- 描边只承担输入边界、选中、焦点及必要列表分隔等用途。Primary 使用 text/surface 明暗反色；导航静态选中用 surface 与文字角色，焦点和禁用变体完整保留。
- 控件保持显式双轴对齐与对称 padding。沿用现有字体、行框、Lucide 图标网格和间距；页面用 hug/fill 与阅读限宽表达，不挪字形或固定每段内容的高度。
- 删去重复说明时，保留影响选择和操作的内容。实际资源路径、查找、工具详情、更多操作、保存/返回等入口不能因参考图没画就丢掉。菜单和 Tooltip 继续复用组件，业务触发与生命周期仍写在原生 requirements。

Product revision `0c285b1d5e710f87559a6b84c6bb8b312ba1cbe26a73981a6ed23e8e488411f0`，Foundation revision `3c49207c3acc71ce4120dfd112ea6dd0cb70a43b98a86c028e239fac57393295`。两包与已检查副本逐文件字节一致；72页、599条连线、原节点和功能 SPEC 保留。没有新增 Token、组件或变体。`source/boundary_refresh.py` 保存这次增量，旧生成器不得覆盖后续保存定义；相关源默认配色和控件角色已同步。

本轮是正式设计维护：29张原生PNG逐张检查；23页明暗×宽窄共92个页面组合及四组组件的282个变体/主题目标检查完成。Button有16条已确认的透明文字/Loader预期叠放提示；透明背景对比度仍有跳过。SmallPen Web复用43128重新打开正本，浏览器像素、真实平台、业务和生产UI实施另行验收。1070份Go/模块指纹未变，不将设计传播写成软件功能完成。详见 `review.json.currentFormalVisualBaseline`。

## 已确认的边界候选 · 2026-11-04

按用户对圆角与竖线的反馈形成工作台候选：[浅色](./exports/archive/coherent-2026-11-04/workbench.png)、[深色](./exports/archive/coherent-2026-11-04/workbench-dark.png)。这两图保留当时的候选修订，不改标为上节正式合并后的导出。原始修订记录见[候选记录](./exports/archive/coherent-2026-11-04/report.md)及 `review.json.currentBoundaryReview`。

## 上轮 Unslop 工作台候选 · 2026-11-04

本轮按用户提供的 [unslop](https://github.com/heydamir/unslop) 方法继续修改 SmallPen 工作台候选：[浅色](./exports/archive/unslop-2026-11-04/workbench.png)、[深色](./exports/archive/unslop-2026-11-04/workbench-dark.png)。导航与主动作收为中性色，把章节预览与执行轨迹连在一起，颜色主要用于品牌及内容。保留既有字体、Token、组件和变体；正式包及程序保持，仍先审设计再实现。伙伴协作尚沿用上一轮候选，不代表本轮全页面完成。

来源归属、实际改动、原生导出及检查边界见[本轮记录](./exports/archive/unslop-2026-11-04/report.md)和 `review.json.currentUnslopReview`。图中的文档提示块属于内容排版，不新增编辑器选区或实时跳转业务。

## 上轮精修候选 · 2026-11-04

用户认为上一版不够精致。本轮继续在隔离的 SmallPen 候选中打磨，正式包与程序保持。新预览：[浅色工作台](./exports/archive/refined-2026-11-04/workbench.png)、[深色工作台](./exports/archive/refined-2026-11-04/workbench-dark.png)、[伙伴协作](./exports/archive/refined-2026-11-04/companions.png)。三图为2倍分辨率原生导出，逻辑视口仍为1440×960。

精修重点是视觉轻重和内容归组：减少大片紫底与重复容器；侧栏退为中性底，正文、进度、成果和消息操作沿同一回复列排列；复用32头像与12间隔自然产生对齐线。右侧复用原PanelHeader/文件标签及关闭、添加、弹出/隐藏入口，移除前轮重复的分类按钮和成果卡；文档用已有24标题与14正文分层。输入区保留原对称8内距/32圆形动作，阅读限宽760，沿用既有字号、圆角、间距与Lucide家族，没有光学挪字或新增Token/组件/变体。

颜色及9个共享组件的候选增量、内容覆盖、状态约束和检查范围见[精修记录](./exports/archive/refined-2026-11-04/report.md)及 `review.json.currentRefinedReview`。候选仍待用户视觉确认；确认后先合并规范并映射到原正式节点、固定确认revision，再实施。不可把候选图、原正式稿和旧运行窗口混为同一版。

## BoardUI 参考候选 · 2026-11-04

用户提供 AI Chat、AI Image Generation、AI Profile 与 Multi-agent Chat 四组参考，要求使用 Alva 自己的风格。本轮保留现有蓝紫/灰白语义色、字体、间距、圆角和 Lucide 家族，研究对话/成果的分工、紧凑执行活动、成员身份与资料层次。成果内联摘要加右侧观察、Profile 的身份分层和多成员发言归属可复用；图像专用功能、统计大屏与持续装饰动画没有自动成为产品需求。

先交付两张包外 SmallPen 候选：[工作台](./exports/archive/boardui-reference-2026-11-04/workbench.png)、[伙伴协作](./exports/archive/boardui-reference-2026-11-04/companions.png)。使用公开 `page draw --preview`，仅在隔离副本完成候选与嵌套内容覆盖；正式两包、其组件/Token/功能SPEC和生产 Go 保持。候选未覆盖正式21/22，也未进入客户端。当前仍遵守用户确认设计后再实施的门槛。

参考来源、意图/原生导出、当前SPEC绑定与候选观察分区的待定边界见[参考记录](./exports/archive/boardui-reference-2026-11-04/reference-notes.md)和 `review.json.currentBoardUIReferenceReview`。新增“成果/成员”观察分区是待审布局提议，不能据此删除当前逐来源资源标签、关闭/恢复与跨窗能力；成员快照只读也不改写全局伙伴资料编辑政策。

## 前轮评审稿：视觉完善，先审后实现 · 2026-11-04

用户要求先在 SmallPen 完善并审阅 UI，确认后才继续实现。当前视觉正本仍是相邻的 Foundation/Product 两包，本节与最新保存定义优先于下方历史配色、尺寸及实施记录；不能从旧截图或旧表格拼成另一版界面。运行中的 Alva 尚未采用这次设计，不标记为已对齐。

本次使用中性灰白阅读面、蓝紫重点和轻微输入区阴影。品牌、新任务、当前导航、发送及工作状态承担视觉重点，普通阅读正文保持中性色；成功、警告、错误继续使用各自语义色。沿用现有 Token、字体、Lucide 图标、尺寸和组件变体，没有新增颜色角色、组件、变体或正式页面。新任务使用既有 Primary，导航 Selected/SelectedFocus 使用 accentSoft 与 accent，SelectedDisabled 保留 secondary；Composer 仅增加共享 Editor 的轻阴影，居中与对称 padding 不变。首页显示原有品牌标记，隐藏重复身份说明。

当前审阅范围是21首页、22对话与面板及其共享组件。除正式代表稿外，包外保留与实际窗口相同尺寸和“你好”内容的对照预览，避免把项目示例、面板开关或运行状态差异误认为所有实现都相同。实际窗口中作者头像/模型身份、导航底部、消息留白等偏差仍须在用户确认后按稿修正；不反改稿子去掩盖缺失。

精确修订、原生 PNG/spec/geometry、主题与覆盖限制见[本轮记录](./exports/archive/visual-polish-2026-11-04/report.md)及 `review.json.currentVisualPolishReview`。本轮不修改生产 Go、不构建或替换客户端；后续实现须引用本次被用户确认的两包 revision，并按相同内容、尺寸、主题和状态核对。

## 原生首页与对话回归 · 2026-11-04

本轮从实际 Alva 窗口重新对照正式21/22及其组件，修正此前“业务/越界测试通过即视为还原”的验收缺口。欢迎标题依当前 `ui.font.size.heading` 为24/600/1.5；建议复用 Ghost/Leading，透明底、行距8；页头主组间距8、标题/路径间距4，搜索位于伙伴与群聊之前。输入框采用内部描边，内容四边保持8，避免 MyGo 布局 border 额外吞掉1；工具栏标签内部间距8、麦克风图形20。对话滚动视口保持全宽，消息项与输入框各自居中限宽760；私有工作区显示业务名称。

工具正文与 typed 引用分别保留，默认呈现紧凑活动行，明确展开/查看详情后复用现有 Inspector 元数据与有界读取，不在消息里拼接原始引用 JSON。用户消息采用既有 accentSoft、12内距和16/1.75正文；普通助手消息沿用共享虚拟列表与 Markdown，不另造聊天引擎。真实身份、工具回执和演示提示仍来自实际状态，不拿设计样例填充用户数据。

原生 requirements 的旧“标题32”已通过公开 SmallPen guarded batch 改为引用现有 heading Token；Product revision `a2260b0516feb62a425cd773d17a6047dcaf7f124603cfa5872d2ce8792031db`，Foundation `1f19aafa7ac0e316c04ed462b13716ed2a8ea5e11500b71ef488bfdcfe13284b` 保持。只更新该说明，页面、组件、Token和连线不变。设计PNG仍绑定修改说明前的源修订，图形相同不改标旧图。字体 WOFF/TTF 的字形/度量与实际普通400/强调600选字已独立核实；光栅器观感一致性不由此推定。

本轮检查、实际客户端更新与系统窗口确认结果见根 MIGRATION；明暗/宽窄无窗口证据及源指纹随 `review.json.currentNativeHomeConversationReview` 记录，不宣称所有页面或全部交互完成。

## 资源组件与模块契约对齐 · 2026-10-27

资源工作区继续复用既有 PanelHeader/Tab：单行52页头、36标签、32动作，默认432面板宽；没有为每个回执或浮层新增页面/Token。原生实现补齐选中 Surface、活动长标签完整关闭、双分页按钮与返回来源窗。三张浅/深色和432/636/弹出1240无窗口代表图已独立检查，捕获源码指纹与后续拖动提示修正分别记录在 `review.json.currentNativeResourceModuleReview`；不据此宣称全部系统窗口/IME/设备通过。

本批功能说明以原生 requirement 元数据维护，连接创建回执、伙伴/群及 Worker typed FIFO 通过共享服务和 SQLite 实施；未保存表单与服务持久事实分别描述。公开 CLI guarded typed batch 新增 resources 的3条、companions 的4条与 workers 的1条文字契约，并更新既有 providers/home-composer；伙伴/群绑定原有节点，Worker 不借用未确认页面。Product revision 为 `15d39a49499d6df9ee69cd53c8f00b3506e110e2670f0680981946c35a7037d1`，Foundation 保持 `1f19aafa7ac0e316c04ed462b13716ed2a8ea5e11500b71ef488bfdcfe13284b`。dry-run/apply 均0 warnings，图形与599条连线保留。详细闭环与验收见[Hub流程](../../HUB-FLOWS.md#2026-10-27-模块闭环增量)和根 MIGRATION，不在画板上复制业务说明。

## General与原生编辑器组件对齐 · 2026-10-26

模型选择器39的原生ModelRow后续校准已按相同两包修订核对：整行hover/selected覆盖右侧收藏与参数，复用既有32px动作/20px图标、8/12内距、8间隔和14/600标题；选中使用subtle底色。正式组件没有额外勾号，已从实现撤去。原生明暗、长名称及独立操作的[定向审查](./exports/archive/model-picker-2026-10-26/report.md)与旧General/菜单证据分别记录，未反改设计来适配实现。

当前Product revision为 `587801a76b8311d623f3cd89b65328b030a6cc09d370558c1cbb2fab23f80152`，Foundation保持 `1f19aafa7ac0e316c04ed462b13716ed2a8ea5e11500b71ef488bfdcfe13284b`。公开typed批次仅隐藏31的Density、Language及未实现Interaction分组，并同步两条既有preferences说明；节点/组件/原型/annotation身份保留，不另加Token或变体。下节 `b11e…` 继续绑定它的元数据批次。

原生实现据已有定义补齐可见的38×24 Switch、16px滑块及键盘checked语义，General平铺/800限宽/24外距/16区块距与ghost值选择；Commands640宽的双316栏、37px图标行和底部搜索，References352宽，窄窗保留全部功能。浮层6px整卡锚点与32px圆形Send不变。[本轮原生无窗口证据](./exports/archive/core-composer-2026-10-26/report.md)保留4张PNG与几何JSON；这是当前尺寸/主题/状态的定向检查，不是全组件或真实OS窗口验收。实际业务、集中测试和一小时稳定性状态见根MIGRATION。

## 首页编辑器实施契约 · 2026-10-26

用户完成逐项确认后，当前先实施首页/会话编辑器。行为正本已通过公开 `put-requirement-file` 与 revision 守卫合并：任意本机文件与首发前项目Skill、同一逻辑会话跨窗共享耐久草稿、首次创建与草稿关联的原子保存、明确Send、运行中仅手动草稿、本机Codex app-server及独立审批/提问。后续伙伴/群、Team/Worker/自动化等保留实现，按模块继续核对。完整规则只在Product原生requirements维护。

本次只改 `home-composer` 的5条既有说明和 `providers` 的一条来源说明，保持requirement ID、标题、links、全部annotation/flow、图形、Token、组件和线稿。Product revision为 `b11e1b3104d71cd4697869fac0cd309256ae33bd3648159a3dd51726d72992b4`，Foundation保持 `1f19aafa7ac0e316c04ed462b13716ed2a8ea5e11500b71ef488bfdcfe13284b`。[保留性核对](./exports/archive/core-composer-contract-2026-10-26.json)只证明本次元数据写入；实现、窗口/设备、真实Codex和一小时稳定性验收另见MIGRATION，不将旧图像或旧测试改标为本轮通过。

## 现行工作方式：基础规范、SPEC与组合预览 · 2026-10-26

本轮采用 `smallpen help skills --json` 提供的 `smallpen-ui-design` Skill（核对版本 alpha.21）。Skill 提供设计/预览操作流程；本项目的风格、组件和业务选择仍由以下现有位置维护，不复制到 Skill 中。

| 内容 | 当前正本与职责 |
|---|---|
| 颜色、文字角色、间距、圆角、共享尺寸及组件变体 | `alva-foundation.smallpen/` 的实际保存定义；从最新 revision 读取绑定与解析值。`source/` 是维护输入，不能全量覆盖后来的人工编辑 |
| 通用视觉/组件交互约定 | 本文现行规则及 [COMPONENTS](./COMPONENTS.md) 的使用边界；精确值引用 Foundation，不另抄一份数值表 |
| 功能 SPEC、触发点与节点关联 | Product 的 `requirements/home-composer.json`、`providers.json`、`navigation.json`、`preferences.json`；每条以稳定 `req_` ID 为准，annotation 关联真实节点，调用位置引用共享组件和通用规则 |
| 职责、实现和验证 | [ARCHITECTURE](../../ARCHITECTURE.md#hub-页面与业务落地) 定义 owner；[HUB-FLOWS](../../HUB-FLOWS.md) 链接操作与验收入口；根 [MIGRATION](../../../MIGRATION.md) 记录实际结果。验收表和施工历史不替代功能 SPEC |
| 本次组合、样例和 Review 证据 | 包外的预览 intent 与本轮 `exports/` 产物；`review.json` 记录来源、组合及检查范围，不将样例写入正式库 |

默认先维护基础组件，通过普通容器组合业务内容。页面实例只提供文案、图标、当前值和必要覆盖，不为每种提示、弹窗使用位置、文案或记录再建组件/画板。复杂新结构可做一处代表设计；仅当任务要求正式页面或原型时才保存页面和连线。当前不支持任意 Instance 内容槽，组合应遵守实际 schema。

功能 SPEC 写清目标/范围、触发与前置条件、输入和可观察结果、状态变化，以及适用的失败、取消、重试、迟到结果、持久化/重开和验收条件。引用组件 ID/变体与通用交互，只描述业务差异；没有决定的条目标为未定，不从名称、图形或静态连线推断行为。CLI 导出的 specification 是视觉约束、来源与原生几何，不是上述功能 SPEC。

复用 Foundation 后使用 `page draw --preview` 形成候选，保存可复现的 intent 和返回证据。Review 核对组件/变体、实例覆盖、Token 来源、布局意图及解析值、功能规则，并查看同次布局的 PNG/spec/wireframe；记录两包 revision、主题/context 和 `layout.hash`。长文案、窄宽度和关键状态按变化范围选取，不展开全部组合。保存正式候选时沿用返回的受 revision 保护批次；过期先重读变化和重做预览。

规范按角色、布局和行为表达，可映射到 Go/MyGo、Electron、Android、iOS/macOS 等平台；值差异、局部结构与整体结构差异分别选择主题、变体或必要的 Presentation。平台适配需有依据，同一功能的结果保持一致。Alva 当前唯一生产桌面仍为 Go/MyGo，本轮不改变技术栈。设计渲染、实际平台画面、交互和业务验收分别记录；预览不证明保存、执行、权限或后端实现完成。

## ComposerMenu 居中与留白修订 · 2026-10-26

用户指出共享组件目录仍有居中和过大留白。实测发现：原生单行菜单项为37高，文字21行框与20图标框中心相同；Web alpha.18组件目录却沿用隐藏说明前的59高/43高Copy，图标与文字中心错开11。前轮只校准页面消费者，漏查了组件目录；不能用原生PNG通过代替Web目录验收。

本轮通过公开CLI替换既有MenuItem的8个状态、ComposerMenu的6个View，身份/节点/选择值保持。紧凑规则见[COMPONENTS](./COMPONENTS.md)：文字与行高保持，共用既有4/8/12标尺，菜单项左右收至8，取消项间额外gap，页脚和空状态去掉叠加留白；正常/空/加载/错误共用图标列。原生Add从265收至237，Commands从219收至199，Empty从112收至88；这些是内容计算结果，不是新增高度约束。

Foundation保存revision为`1f19aafa7ac0e316c04ed462b13716ed2a8ea5e11500b71ef488bfdcfe13284b`；Product为`18a4a829cef30fa2643e335aa7ad6cb21883139af173af481489c6fc51a60c30`，只把41代表装配到输入框的间隔12改为既定6，保持左锚点。同一进程预览修复补齐组件目录和按需生成的Design System样例，并让这两个组件的目录占位忽略隐藏子树；三份已安装模块按SHA守卫，仅改进程投影，无安装文件/设计源直接写入。43128原位重启后，实时API核对6个View及其总览占位与原生高度一致，普通项图标框/文字行框中心差0。

原生导出覆盖6个View、浅/深色代表、MenuItem默认/焦点/禁用以及24/41实际消费者；8张重点候选画面独立视觉审查通过，41锚点另行复核。两个组件的14个状态自动检查0报告问题，自动跳过项仍保留。CUA超时，未取得实际浏览器像素，HTTP/原生证据不冒充此项验收。没有新增Token、组件、变体或页面，也没有改业务SPEC、连线、生产Go或原始线稿。详见[本轮证据](./exports/archive/composer-menu-2026-10-26/report.md)和`review.json.currentComposerMenuRefreshReview`。

## 正式页面稿更新 · 2026-10-26

本轮已实际修改并保存14张既有正式页面：21/22/23、27/29、31–38、40。首页标题与建议动作减轻层级；对话/伙伴的正文、输入区和右侧文档按阅读区组织，正文保持16/1.75；模型目录合组搜索/刷新/添加、列表随内容收缩，240px业务连接列保留完整名称；设置页共用左侧起点和适用的800/960/1120内容上限，减少嵌套卡片。31补上真实提交契约所需的保存/重新加载入口，原始未编辑样例禁用保存。默认包入口仍是旧框架线稿，新稿位于“01 产品界面”。

同步修正已有ConnectionRow/Composer/RunStatus的10个变体：选中/未配置/已保存不再冒充连接测试成功，未知耗时/速率/Context不显示，运行输入文案只表达下一步草稿。没有新增Token、组件、变体或页面；既有身份、主题值和599条原型连线保持。General只沿用原SPEC增加2条保存/重新加载annotation与装配说明，其他功能说明保持。对话框24–26/28/30、选择器39、添加菜单41及模型表单42/43本轮保留，未声称逐页重审。

保存后的Foundation revision为`e97738a8e0f4454c4f5701b7deaae7b1140c81cd4a177e1dc5076d2957a6c8d8`，Product为`d22789819c74715624526554bee9fbd91f881e986b6c8c121bbe090ff8f14f38`，与已审候选逐文件相同。14页浅色桌面及其中4页深色窄宽度的18组原生导出均resolved、0报告问题；仍有对比度等自动跳过项。14张最终PNG与已审初稿逐字节一致，另4张重新检查窄宽度文案/列宽。详见[本轮画面与审查](./exports/archive/formal-pages-2026-10-26/report.md)及`review.json.currentFormalPagesRefreshReview`，历史报告继续绑定原修订。

本轮只改设计稿、作者输入和规范，未修改生产Go代码；浏览器现有43128服务已重新载入正式保存包。HTTP重新载入与原生静态图审查分别记录，不代表真实浏览器像素、键盘/IME、设备或全部业务验收。后续实现应从这次保存的约束/组件引用继续对齐，不重新生成旧稿覆盖新布局。当前增量入口为`source/formal_refresh.py`，先隔离候选、原生检查及视觉审查，再按已审revision原位提交。

## 原生模块首批对齐 · 2026-10-26

沿[逐模块清单](../../HUB-FLOWS.md#逐模块对齐顺序--2026-10-26)把既有规范落实到 Go/MyGo：共享 Button 的保宽/保焦点 Loading、Ghost/Danger 与焦点环，连接获取/测试/保存和 General 保存，以及输入区发送/停止、权限图标、Full 确认和语音状态。本轮没有扩充 Token 或组件；Loader 仅重绘，系统减弱动效下不旋转。实现细节、回归和未闭环范围以根 [MIGRATION](../../../MIGRATION.md) 为准。

功能正本只通过公开 `put-requirement-file` 更新 `home-composer` 里5条既有说明：单字符 `@`/`/` 的边界触发、菜单与发送快捷键的区别、下一轮设置独立保存状态及实际发送准入。其余说明、10条 requirement 和20条 annotation 的身份/链接、图形与组件都保留。Foundation 仍为 `608325d27c9c3647e024a43d91cd4c3ea82413e069966646f96f1d4b15406e35`；Product 当前元数据 revision 为 `7195d58b760c54c8bcfac0156ac582646feb8dd41cfd13f8906307f0f38115fb`。下节较早的 `7e2af363…` 继续绑定其原始静态设计审查，不将旧图标成新 revision。

本次证据独立关联 `review.json.currentNativeModuleAlignmentReview` 与[原生样本报告](./exports/archive/native-module-alignment-2026-10-26/report.md)。四张 PNG 证明所列主题/尺寸下的展示；两项使用真实 gated 本地服务的按钮行为、自动化业务回归和系统窗口/账户/设备验收分别记录。没有将每种 popover 或状态重新保存成页面；其他模块仍沿清单继续核对，既有业务回归不代表已完成本次整域 UI 审查。

## 基础组件实查与加载态修正 · 2026-10-26

按新流程审查 Button、DialogHeader/Footer、菜单项与 Tooltip，使用包外原生组合验证默认、焦点、加载、隐藏可选说明、长文案及拉伸。修正了纯文字 Button 直接切 Loading 时新增24px宽度的问题，并保留后置 Loader 的位置、转写状态可见文字与获取模型的动作名；共享使用约定见 [COMPONENTS](./COMPONENTS.md#button-加载态与复用)。

本轮仅通过公开 typed batch 修改既有 Button、VoiceInput、ProviderModels 的41个变体；组件／变体／节点身份、轴与 Token 均保留，没有保存样例页或重画 Product。Foundation 当前 revision 为 `608325d27c9c3647e024a43d91cd4c3ea82413e069966646f96f1d4b15406e35`，Product 仍为 `7e2af36356bfb58acc2276853bcf8ef0615b190be52c2cce4de0fee649f6cebf`。修正前审查、隔离候选及保存后验证分别关联到 `review.json.currentFoundationControlsReview`，保留各自来源；实际产品代码和运行验收不由这轮设计修改代替。

## 功能实施状态与契约收口 · 2026-10-24

本轮增量实现与验收分别记录在根 MIGRATION；下方历史修订不能作为本轮设备/平台证据。伙伴、资源视图与会话参数均复用现有共享服务和唯一Runtime，SQLite v4→v5同事务升级，失败保留旧事实；macOS语音由独立system owner管理实际采集/回调收尾。

2026-10-24仅通过公开CLI、revision守卫合并既有home-composer/providers两份行为元数据，保留全部requirement/annotation身份；Product为`7e2af36356bfb58acc2276853bcf8ef0615b190be52c2cce4de0fee649f6cebf`，Foundation保持`7f389e1a417aa4b763b2e46e01bcec766172fc28fb6e84374e81f4cf1668985c`。本轮没有新增Token、变体、页面或图形定义，历史截图不改标本轮。

本节以当前代码和原生 requirement 为准；下方各轮的“尚缺”保留当时范围，不能覆盖本节。当前生产界面仍是 Go/MyGo。模型连接与权限本波已写入代码，运行检查由本波实现记录报告；本文的元数据保存与静态引用检查不代替真实平台或外部账户验收。

| 功能 | 当前实现 / 明确缺口 | 业务 owner 与验证入口 |
|---|---|---|
| 权限 Ask / Auto / Full | 下一次明确 Send 生效，运行、waiting、checkpoint 冻结；Full 在同一菜单确认范围。Auto 仅自动批准已知原生工作区文件写入；命令、网络、MCP、未知效果仍问。设计中的安全命令自动分类仍未实现 | `product.SetPermissionPolicy`、共享 Settings、bootstrap 审批 handler 和原 Guard；核对保存/重开、Full 取消、越界拒绝、当前 Attempt 不变 |
| Plan | 最高只读，切入重置 Ask；选择权限显式退出，退出不恢复旧 Full；进程/网络集成本轮不装配 | product 与 Attempt 准入、native Guard；读/拒写、冻结恢复和 external fallback 零调用 |
| 连接 / 手填模型 | 稳定 ID + revision 的创建、编辑、启停、二次删除；空 Key 编辑保留，Clear 或 Auth=none 清除。秘密存私有 SQLite 分表，未加密，不宣称系统钥匙串 | `product.ConnectionsService` / `hub.ConnectionsStore`、bootstrap；CAS 冲突、失败保稿、脱敏读取、重开与 exact-revision 路由准入 |
| 连接测试 / 自动目录 | 已有四协议显式目录 GET、六态、搜索/分页/选择及手填合并；自定义/本地须明确同源目录地址，保存不联网。Test仅证明目录访问，不冒称推理成功 | `ConnectionsService.TestConnection/FetchModels`；真实本地HTTP、401/超时/空目录、来源变更、取消/迟到结果与Close回归 |
| 本机 Agent 来源 | 用户确认直接复用明确选择的本机 Codex CLI 配置/登录状态；不自建登录/退出，不扫描或复制凭据。真实调用边界见根 MIGRATION，Claude仅fixture | 实际 Agent adapter、显式 executable/config route；来源变化拒绝、同thread恢复和真实取消/join |
| 联合模型选择器 | 已有真实连接/模型聚合、搜索/筛选、收藏、兼容Agent及默认参数Save/Reload；独立偏好revision不改连接授权，实际Attempt冻结并应用参数。当前会话覆盖已有按账户/模型独立CAS、清除和下一新Turn生效；运行/Waiting保留原Attempt。Context上限缺可信声明时保持未声明，不冒用样例 | 产品目录/路由与ConnectionsService提供事实；16384路由虚拟列表、失效拒绝、真实Provider请求、等待续轮冻结及未知回执Reload回归 |
| 新建目标 | 已有共用表单、稳定目标事实、项目/会话范围、RequestID幂等回执及可移除草稿引用；首次明确Send才绑定真实会话与原件Artifact。原件消费目前仅Alva | `GoalsService` / `GoalStore`与既有Submit链；创建/取消零执行、失败保稿、未知回执显式重试、最大转义原件、真实首发/重开回归 |
| 常驻伙伴 / 群 / 协同 | 独立Profile/群、固定Skill来源、私聊与群×成员隔离记录、明确账户/计费确认和有限回合已接代码；等待/未知保留原Round/Turn与游标，重开只冷读，明确继续不重跑已完成成员。实际Skill repo按名称/原文SHA限制，群只用明确共享项；受限scope不支持compatibility导入 | 产品伙伴/协作服务，复用现有任务执行 owner；成员快照、预算、审批与 Stop 真实收尾 |
| 命令 / 引用 / 发送 / Stop | 已有 typed preparation、来源重验、首发、匹配回执清稿和物理 owner 隔离；首页可选任意本机文件/受管项目Skill并保存耐久草稿，首发原子采纳；compatibility Skills 不隐式枚举 | `product.composer_*` / workspace references 与现有 Runtime；选择零执行、来源变更拒绝、新草稿保留、迟到 Stop 不影响替换 owner |
| 语音 | macOS系统on-device录音/转写已接代码；明确点击才请求许可，最长60秒，无音频文件或远程fallback。离开聊天停止，修改过的草稿保留待采纳，首页首发随真实会话迁移；真实设备/拒绝/声学质量仍待用户点击验收，其他平台明确不支持 | 产品媒体/设备 owner；实际设备拒绝、独立停止录音/任务、原草稿与晚转写不互相覆盖 |
| 右侧标签工作区 | 逐会话标签的关闭/菜单重排/专注/同app弹出、来源与分页/滚动CAS恢复已接；文件/原件/任务/Inspector读实际事实。拖拽/键盘重排及宽度CAS已补，弹出可返回原窗口或当前窗口恢复；媒体播放/递归Inspector继续走专用pane；观察/重开不执行 | UI视图容器与各资源 owner；开关/退订不 Stop，恢复不重放进程或浏览动作 |
| General 与设置返回 | Theme、14/16字号真实生效与CAS；语言/密度暂隐藏并保留旧值；应用级减弱动效已接同一CAS/Save/Reload与系统要求取或，实际Loader消费。分类切换共用原工作台来源，保留未存草稿 | PreferencesService 与 UI 导航 owner；跨窗/重开、冲突保稿、保存期间关窗守卫和媒体冷索引返回 |
| 运行中补充 | 用户确认仅保留可编辑耐久草稿，不自动发送/排队/插话；Worker FIFO 为独立显式功能 | 产品准入/执行 owner；未知回执先查询，不自动重放副作用 |

连接新建 Back 回24，同来源保留本次草稿，换来源按新预设重置未存字段/秘密；Cancel 明确放弃回23。编辑携原 ID/revision，Back/Cancel 直接回23；普通设置分类切换/返回工作台保留本窗草稿。Save 成功才退出，失败留稿，冲突要求 Reload。模型/连接禁用影响之后的选择和准入，不取消当前执行。具体触发、迁移、禁用和错误写在 [providers](./alva.smallpen/requirements/providers.json)、[navigation](./alva.smallpen/requirements/navigation.json)、[home-composer](./alva.smallpen/requirements/home-composer.json) 与 [preferences](./alva.smallpen/requirements/preferences.json)，不靠画布上的解释文字补逻辑。

目标沿用已有设计流：打开共用表单 → 本窗草稿 → 名称/完成标准/范围校验 → 明确创建 → 真实 goal ID/范围/revision 回执 → 关联可移除草稿引用；之后明确 Send 才表达执行意图。取消零创建，失败保稿，结果未知以原RequestID显式重试查询同一事实，移除引用不删目标。实现复用既有执行runtime，不把会话Team改名成伙伴。

本轮只通过公开 `put-requirement-file` 合并既有4份元数据，保持18条 requirement ID与原有节点身份，新增一条绑定23管理容器的启停/删除 annotation；没有新增页面、组件、变体或 Token。Product revision 为 `1210803cfc4bc7ad81cbf2cec60ac36833cf9b2ffaf01e34fe7753d5e2137966`；Foundation 不变。检查范围与原始回执见 [本轮契约证据](./exports/archive/workflow-contract-2026-10-23/)。没有进行新的截图调查，不重启43128或原生预览客户端；真实 OS、IME、读屏、设备、外部账户与跨平台仍单独验收。

## 输入区对齐与浮层锚点 · 2026-10-23

2026-10-24原生还原校准：实现按Foundation Tokens→组件变体→Product装配逐层核查，共用内部原生controls，不直接采用框架默认外观。重点修正208px设置导航、居中限宽区域、隐藏辅助描述、36px控件与37px菜单行、39的640px主浮层/300px选项/16px内距、25的800px对话框/密码图标，以及59px Attachment和26px Badge。设计图形、Tokens、组件与页面定义未改；公开CLI只同步两份requirement的实际实现状态，当前Product元数据revision为`388aa49805bbeb4e8bbaff53ecdc42c68c38ab58d9916840147f83d8a36e280a`，Foundation保持`7f389e1a417aa4b763b2e46e01bcec766172fc28fb6e84374e81f4cf1668985c`。分轮图像只证明捕获时源码，不能给旧截图套用后续修正；代码规则与实际平台/业务验收分别维护。

Composer.Editor四边对称内边距由12收至8，组间隔8、16/1.75正文和两行起始编辑空间56保持。底部工具栏控件统一高32；Send/Stop为32×32圆形、圆角16、居中图标18，Add图标18，Permission/Model图标16、下箭头14、文字14/行高21。普通Button高36维持。运行工具栏标签采用对称上下4/左右12；SmallPen的嵌套Instance不支持局部layout-padding覆盖，静态Permission/Model使用共享Button的居中布局与左右12，不能将两种表达方式声称为逐像素一致；不修改全局Button、不用宽度补偿模拟padding。边框1加对称padding8，原生动作距卡片外边缘实际为9。

`@`与`/`候选菜单锚定整个编辑卡片左上方，间距6，不再锚定正文TextArea底边。Add、Permission和紧凑Model菜单优先在各自触发点上方6，空间不足再翻转、限高并夹紧。菜单保持独立浮层，不占布局流、不撑高输入区、不遮挡正文/光标/工具栏。39模型选择器的代表页与现有导航仍保留；21/22继续通过既有`req_alva_composer_focus`引用共用菜单，不新增逐项overlay示例。

本轮仅修改14个既有Composer变体和首页既有state/focus契约，作者配方同步；没有新增Token、组件、变体、页面或包。保存修订、尺寸测量、保留项及静态工具限制见`review.json.currentComposerAlignmentReview`和[本轮证据](./exports/archive/composer-alignment-2026-10-23/)。设计导出与原生运行验收分别记录，真实OS窗口、焦点/IME与其他平台未由本次静态检查证明。43128现有服务与原生预览进程不重启、不替换。

## 原生业务接线校准（2026-10-23）

原生本波在前轮首发附件、草稿回执、模型选择、停止 owner 隔离及设置返回的基础上，接入项目引用、命令/Skill preparation 和实际 Plan。`@` 限已有 Alva 会话的 saved workspace：搜索只读有界 metadata，确认选择时保留文件原件 artifact 或目录 metadata manifest，Send 核对 branch/workspace/native 归属后提交冻结引用。`/` 提供内置 `/plan`、`/review` 与已有项目 `.alva/skills` 的 typed preparation；选择不执行，明确 Send 由共享 service 重验来源 revision/可用性后沿普通 Submit 准入。compatibility Skills 未枚举，命令不是 shell；首页仅内置命令和本窗 Plan 意图，项目引用需要已保存会话。

Plan 保存为下一轮选择，实际 native Guard 只允许读取，非读取效果直接拒绝；本轮不装配 Hooks、诊断、MCP/AEP、Shell/Web、Browser、Media、SubAgents，原配置和批准保留。关闭只恢复 Ask，仍须明确 Send，写入继续独立审批。运行/等待和未放弃 checkpoint 禁改，恢复绑定原 recipe；实际 Attempt 准入事务也拒绝 Plan 逃往 external engine，包括已授权 fallback。Auto/Full、目标完整表单/取消、常驻伙伴选择及作用域、Provider 模型目录、语音与右侧标签工作区仍缺；详见[实现行为对照](../../ARCHITECTURE.md#hub-页面与业务落地)。

本波仅通过 revision 守卫的公开 `put-requirement-file` 更新 `requirements/home-composer.json` 中 7 条既有行为说明；Product revision 为 `53fa1801dce1f2677cb33b79d5ede59ebe7a9f8f7c3dc4e2311fa8fc187a10fd`，Foundation 保持 `c1c734a19b93d8d0f9ed96a2c36880621ee8083055123b8dd1de6ea17a7f694d`。10 个 requirement ID、20 条 annotation、links/标题和全部画面/Token/组件/原型连线保持；旧视觉审阅继续绑定原 revision。证据见 `review.json.currentWorkspaceCapabilitiesBehaviorReview` 和 [最终状态范围核对](./exports/archive/workspace-capabilities-2026-10-23/validation-preservation.json)。本波全仓 desktop-tagged race 已通过：96 个有测试包、2252 项测试/子测试、0 失败，耗时 100.12 秒；执行期间 906 份 Go 源码/模块记录未变，全树 vet、mod verify、构建通过。原生 headless 明暗/1240 与 960 六图及深路径 basename/full-path tooltip 追加两图通过；证据为 checkpoint 下 `validated-full-race-summary.json`、`validated-static-build.json`、`visual/findings.txt`、`visual-labels/findings.txt`。新版独立 local-demo 预览 PID 8503 已启动，旧 PID 34764 保留；真实 OS 窗口/选择器、焦点/IME、读屏、外部账户和其他平台仍未验收，以根 MIGRATION 和实际测试记录为准。43128 现有服务保持运行；仅备注变化不重载视觉投影。

## 工作台密度：收紧留白（前轮记录）· 2026-10-23

针对实际原生实现显得松散的反馈，本轮让21首页、22对话与共享导航/编辑器遵守同一套紧凑参数。导航宽232→220、页头64→52；首页外边距32→24，内容限宽720→760。欢迎标题继续32，标题到输入区16、输入区到建议12；不新增字号或密度变体。导航行40→36、条目间隔8→4，分组前仍保留12（4间隔＋8上边距）。普通按钮36、图标动作32维持。

输入区内边距16→12、组间隔12→8；16字号与1.75行高保持，起始编辑空间统一两行56，加36控制行、8间隔、24上下内边距，Editor配方为124。SmallPen用28行框＋8间隔＋20预留表达两行；原生导出按实际文本测量取整为125，不做像素补偿。20是行高推导的局部几何，不是新间距Token。真实产品应按两行最小高度实现并随内容增长。会话组间隔16，连续阅读正文保持16/1.75。

仅对既有3个尺寸Token、Sidebar/Composer的真实共享变体、21/22装配和已有`req_alva_composer_state`说明做公开typed增量写入；不重生成其他页面，保留稳定ID、组件引用与原型连线。作者定义与[增量脚本](./source/density_refresh.py)同步，后续重生成不能退回旧密度。精确revision、原生导出与范围保护见`review.json.currentDensityReview`及[本轮证据](./exports/archive/density-2026-10-23/)。共享组件会影响既有消费者，自动与视觉检查的具体范围分别记录；静态稿不代替真实平台交互验收。

## 当前组件：偏好设置行 · 2026-10-23

31通用页的外观不再用四个满宽Field排成填写表格，改为共享SettingRow：左侧名称、右侧当前值与下箭头，宽度随内容；减弱动效与其他偏好使用同一行框。组内连续排列，去掉末行分隔线，必要说明保留在交互与通知中。具体配置表单仍用Field/Input，不能把所有输入都改成无边界按钮。

SettingRow只增加Control=Select结构，内部复用Ghost/Trailing Button及其既有状态，菜单复用SelectOption和Popover配方，不复制浮层画板。共享行的上下padding为8、说明gap为4、标签常规字重；Switch的On改用主题accent/onAccent，Focus/OnFocus使用主题色环与surface间隔。没有新增Token或组件；正式范围为56个组件、479个变体。

截图四项在保存稿里是Default输入态；Web投影边框为 `#76867D`，实际焦点绑定ui.color.accent，而非紫色。截图紫框可能来自编辑器对象选中提示，不能以它反推产品组件颜色。新原生 `requirements/preferences.json` 的2条契约/5条标注绑定真实节点，定义触发、共用菜单、焦点、选项来源与实现边界。保存修订、明暗/宽窄和共享消费者检查见 `review.json.currentPreferenceRowsReview`；静态设计不执行偏好保存或系统主题切换。

## 当前主题：清透底色与青绿重点 · 2026-10-23

减少前轮纸黄、暖灰与陶土同时铺开的厚重感。浅色画布改为近白 `#FAFBFA`，侧栏为轻灰绿 `#F0F3F1`，阅读表面仍用白色；正文 `#29332F`、辅助文字 `#5E6B64`。强调色改为低饱和青绿 `#246B65`，选中底色 `#E7F2EE`，只在主要动作、导航选择和已有重点使用。没有给更多区域增加填色。

深色使用墨绿灰底 `#171C1A`、表面 `#212825` 和浅青绿重点 `#91CFC3`。已有成功、警告和错误语义色保留，继续结合图标/状态文字区分；不把操作强调色当成功回执。浮层阴影只减轻色值与透明度，几何、排版、字重、边距和组件状态不改。

本轮仅通过公开CLI修改Foundation的25个既有主题值；Product、72张页面、全部组件定义、字体、图标、原生行为备注和稳定ID保持。该轮调色时为56个组件、478个变体、61个ui.* Token；最新结构数量见上节。当前颜色以本节和保存包为准，下方陶土色记录保留对应历史修订。配色对比、原生页面/控件检查、7张视觉样本与同一Web预览核对见 `review.json.currentThemeRefreshReview`；不代表真实浏览器交互或长时间阅读实验已验收。

## 当前：来源优先的连接表单 · 2026-10-23

23 模型与连接保留连接列表和模型目录；添加流程改为“24 选择供应商 → 25 预设 / 26 自定义”。25 和 26 是同一共用表单的两种来源，不是连续步骤。预设默认只突出 Key，连接设置可展开完整端点与协议；自定义直接显示完整字段。模型获取、搜索、勾选和手填在表单内可选完成，保存直接回23，不再要求第三步。

Foundation 新增 ProviderForm（Preset / Custom）与独立 ProviderModels（Idle / Loading / Ready / Empty / Unavailable / Error）。供应商名称、端点、创建/编辑模式和模型目录是实例数据，不能乘成变体。该轮正式范围为 **56 个组件、478 个变体、61 个 ui.* Token**；没有新增 Token 或页面。原24/25/26的页面、配置与根节点身份保持，26改名为“自定义连接”；旧线稿08–10保留为历史参考。

24左上返回23；25/26左上返回来源选择24，取消/关闭退出到23。编辑入口复用表单并携带现有ID、调用来源和返回目标，不能误建连接。原生 [providers.json](./alva.smallpen/requirements/providers.json) 维护来源、字段/测试、可选目录和保存的4条契约/13条标注；[navigation.json](./alva.smallpen/requirements/navigation.json) 更新为2条契约/12条标注。首页10条契约/20条标注保持不变。

参考 Magpie 固定修订的[添加表单](https://github.com/yetone/magpie/blob/eee9ece29306cd26f78e86cea075f2bff01870a0/internal/gui/assets/app.js#L7226)，吸收预设快捷路径、折叠设置与保存前可选模型选择。账户连接仍须用户主动发起，不扫描旧凭据；生产栈与SDK边界保持。精确修订、44个明暗/宽窄原生检查目标、展开探针、视觉复核与 Web 投影见 `review.json.currentProviderFlowReview`。实际联网、凭据保存、长目录滚动、键盘及草稿保持仍须产品实现与验收。

## 子页返回入口与目标（前轮记录）· 2026-10-23

9 张设置页共用的 SettingsNav.BackToWork 从左下方移到导航最上方，位于“设置”标题之前；保留左箭头与“返回工作台”，宽度随内容，不再被弹性留白推到底部。8 个分类变体统一修改，已有节点身份及返回工作台的原型目标保持。

连接向导在弹窗左上补齐“返回选择供应商”（25→24）与“返回配置连接”（26→25），复用现有 Ghost/Leading Button；24 的“继续”补齐到25的原型跳转。上一步与底部取消、右上关闭分开，后两者仍退出到23模型与连接。没有增加 Token、组件或变体，也没有重画整套页面状态。

返回规则通过原生 [navigation.json](./alva.smallpen/requirements/navigation.json) 维护：2 条 requirement、11 条节点 annotation。静态设置返回仍指向22；运行产品应回打开设置前的工作台/会话，返回向导上一步应保留同一草稿。真实草稿保持、键盘及未保存离开策略仍待产品验收；视图退出不停止任务，不触发测试或保存。首页的10条契约/20条标注保持不变。修订、明暗/宽窄检查与 Web 核对见 `review.json.currentNavigationReview`。

Web alpha.18 仍把返回按钮的 hug 宽度取成旧样本105，并未遵守向导 maxWidth。现有带精确版本哈希守卫的进程级预览修正扩展到 SettingsNav 与连接向导（Alva Step / WizardBack）消费页，使用该 Web 自带 native 布局恢复尺寸和相对坐标；不修改安装包、正式数据或产品运行依赖。原生 PNG 与 Web 投影分别核对，浏览器实际点击与真实状态保持仍待验收。

## 暖纸与陶土色打磨（前轮记录）· 2026-10-23

沿用已有页面与组件，借用 Open Design 的实际页面审阅、少量集中修正方法。画布保留纸白 `#FAF9F6`，导航调整为暖灰 `#F1EEE8`，正文用暖炭灰 `#2D2C29`、辅助文字用 `#6B665F`，减少原先的冷紫灰感。阅读面板继续使用白色，以表面和文字层级区分区域；不增加边框卡片或装饰效果。

强调色统一为陶土珊瑚：浅色 `#AD4732`、深色 `#E7A28C`，继续使用原有 accent / onAccent / accentSoft 角色。选中导航只让图标呼应强调色；用户消息复用淡珊瑚底 `#F9EBE3`，与回复及右侧资料区分。选中且禁用的导航图标回到辅助色。浅色浮层阴影去掉蓝紫偏色，深色表面同步改为暖墨色。

保留中文正文字号、行高、对称 padding、图标槽、圆角、布局及交互备注。没有新增 Token、组件、变体或页面；共享主题值与 NavItem 在 Foundation 修改，22 页只改 UserMessage 的既有颜色绑定，作者源码同步维护。正式范围仍为 61 个 ui.* Token、54 个组件、470 个变体。对比度矩阵、原生明暗/宽窄样本、常用控件状态与 Web 重载结果见 `review.json.currentColorPolishReview`；静态样本不代表实际录音、执行或浏览器交互验收。

## 浮层复用，不逐项画提示 · 2026-10-23

21/22 各自的 11 个重复浮层示例已撤下，共 22 个辅助根、52 个节点；移除只服务于这些示例的原型开关，保留主页面与原有模型选择等业务导航。Foundation 的 **Alva Tooltip、PermissionMenu、ComposerMenu** 和既有变体维持一份定义，41 添加菜单保留一处代表装配。提示语变化、不同按钮的 hover、权限说明不再产生新的画板、按钮副本或变体。

使用位置通过 `req_alva_composer_focus` 和 Composer 实例上的原生 annotation 声明“触发点/事件 → 共享组件及内容 → 关闭/焦点规则”。保留 10 条功能契约，相关 annotation 合并为 20 条；移除指向已撤示例的重复绑定。`@`、`/`、提交、执行、录音与权限的业务规则继续保留，文字配置不表示已经执行这些行为。

通用 Popover 的表面、圆角、阴影和布局来自 `design_system.popover()` 的单一共用配方，使用现有 Token；Tooltip 的外观由共用组件维护。当前 CLI 没有任意 Instance 内容槽，不能用空壳组件名伪装已支持插槽。业务菜单继续复用现有原生组件；未来运行层把触发、锚点和内容作为组件配置。复杂菜单/表单的内容仍需一处实际设计，其余调用位置引用即可。

本次 Foundation、Token、组件及变体数量不变；未扩大为新的页面状态集。收敛记录与检查见 `review.json.currentPopoverReuseReview`。下节记录前轮的具体修复过程，其 52 条标注和 22 个浮层是历史状态，不是当前维护方式。

## 首页行为备注与 Web 浮层修正 · 2026-10-23

功能规则的当前入口是 Product 内的 [requirements/home-composer.json](./alva.smallpen/requirements/home-composer.json)：10 条原生 requirement、52 条绑定到 21/22 真实节点的 annotation。覆盖 `+`、`@`、`/`、菜单与 Tooltip、提交回执、运行/等待/停止、录音转写、模型/权限以及首页剩余缺口。每条说明触发、前置条件、动作、退出、异常与验收；明确区分设计约定、静态原型和未接通的产品行为。新增建议标为待确认，不当作用户已经批准的产品决定。

这些备注供作者和实现 AI 读取，不增加页面文案。画布上的 PopoverAdd 等是编辑器 FRAME 名称。同步采用稳定的 `req_alva_composer_*` ID 与原生节点引用；改 UI 时连同受影响行为一起更新，不能让实现 AI 从截图猜事件。已有原生原型只演示点击/悬停，键入 `/`、IME、Esc、真实查找、执行与录音仍需产品实现。当前未确认运行中补充的插话/排队策略、目标/协同完整流程、命令触发的精确词边界和权限政策分类。

用户截图暴露了前轮验收漏项：Web alpha.18 只重排主根，没有重排同页 secondary roots；其旧布局投影还保留了隐藏 Description 的高度。于是原生导出正确，Web 菜单行仍高低不一，Tooltip padding 未落地，麦克风与提示文字重叠。组件保持对称 padding 与内容撑高；隐藏的 Shortcut 也退出布局。RunStatus 的执行错误与 Composer 的提交失败文案分开，避免把已接收任务误说成未发送。

当前预览通过 [smallpen_web_layout.mjs](./source/smallpen_web_layout.mjs) 对**本次 Web 进程**加载的精确 alpha.18 模块做有哈希守卫的修正：用该版本自带的 native 引擎计算 secondary roots 及 ComposerMenu 页面，转换成父容器相对坐标，并显式保留无填充 FRAME 的透明性。它不修改全局安装或 canonical 节点，不是产品运行依赖，也不修复所有 SmallPen 组件目录/版本；上游修复后应移除，升级时哈希不符会停止而非继续套补丁。

前后同一组 Web 接口断言已验证 21/22 的 22 个浮层及 41 添加菜单：单行项按 21 行高 + 上下各 8 推导为 37，图标槽垂直居中；Tooltip 上下 8/左右 12；HoverVoice 回到 x=288/y=42，透明装配区域不再声明白色实底。此结论是 Web 投影数据检查，当前 CUA 没有连接的浏览器，**未宣称最终浏览器画面或 hover 操作已验收**。证据见 `review.json.currentComposerBehaviorReview`，旧原生导出报告仍保留原 revision 与范围。

## 输入区状态、菜单与悬停 · 2026-10-23

该轮正式范围为 **54 个组件、470 个变体、61 个 ui.* Token**。Composer 使用 14 个视觉样本；实现时草稿、执行、录音、浮层是独立状态，不能照抄为互斥的一个枚举。权限与模型仍是配置；新增 PermissionMenu、ComposerMenu、VoiceInput、RunStatus。完整状态与使用边界见 [COMPONENTS](./COMPONENTS.md)。空输入不能发送，提交中保留忙碌反馈，工作中显示独立停止动作，停止中等待真实收尾，提交失败保留草稿。录音停止与任务停止分开，转写不覆盖原草稿。

前轮曾把 21/22 的权限、添加菜单和 hover 提示铺在原页面附近；现已按上面的复用规则撤下，41 继续使用共享菜单定义。权限边界放 Tooltip，点击选择使用 Popover/Menu。选中值与执行状态由原生变体审阅；SmallPen alpha.18 的 App 原型不能执行 set-state、真实输入或录音。键盘 focus/Escape、实际筛选、命令插入、语音、状态更新与动态定位须在产品中验收；不以静态原型冒充行为完成。

首页两个图标槽与文字行框原本都居中，但文字墨迹仍低约 1.2–1.4px，前轮整页视觉检查漏掉了这一点。正式 `ui.font.family` 现在使用 **Alva UI Sans SC**：保留 Noto 字形与字宽，把 hhea 与该源字体已有的 OS/2 typographic 度量统一为 880 / −120 / 0，并启用 USE_TYPO_METRICS。保留 Windows 裁切范围、历史 Prototype 字体与 Lucide 图标；没有不等上下 padding、逐标签位移或新 Token。详见 [UI 字体来源](./fonts/UI-NOTICE.json) 与 [OpenType 建议](https://learn.microsoft.com/en-us/typography/opentype/spec/recom)。

本轮记录在 `review.json.currentComposerReview`，独立保存字体保护、控件状态、原型连线和视觉检查；下文历史审查的 revision 不改标。

## 常用组件覆盖与统一图标 · 2026-10-22

继续复用 Foundation 的 Tokens 与共享组件，Product 保留页面装配；不要求 Go／HTML 发布额外 UI 包。完整目录、编辑入口、组合方式和后续专业控件缺口见 [COMPONENTS.md](./COMPONENTS.md)。

本阶段补充 SelectOption、Attachment、CodeBlock、Citation、DetailRow；为已有控件补齐必要的选择／焦点、错误／焦点、只读、工具展开和任务排队状态。模型编辑中的 ID 使用只读而非禁用。当时正式范围为 **50 个组件、438 个原生变体、61 个 ui.* Token**；唯一新增 Token 是代码／日志／标识符共用的 `ui.font.mono`，颜色、尺寸、间距与圆角档位沿用。

68 个界面符号全部统一为 **Lucide 1.61.0**，使用官方 24×24 画布、2px 线条和圆角端点，显示尺寸继续为 16／20。Alva Icon 的名称、码点、组件与变体身份保持不变，不混用其他风格。原始 SVG 保存在 `fonts/lucide/`，SmallPen 使用同源的字体子集；格式转换保留画布、advance 和真实左边距，不按图标单独挪位置。Go／HTML 可直接复用 SVG，不必加载整个图标库或前端框架。许可与来源哈希一并保留。

日期／时间、图表、执行图、差异编辑与媒体查看等专业交互仍需随具体页面确定，不能仅凭通用目录承诺未来所有页面完成。本轮组件覆盖、图标替换前后检查及 Web 重新加载分别记录在 `review.json.currentComponentCoverageReview`；前轮证据不改标为新修订。

## 整体 UI 审查：让布局说明内容 · 2026-10-22

Avoid unnecessary secondary labels and descriptions. Let the layout do the explaining

本轮覆盖 21–43 全部 23 张正式页面。移除重复页面简介、常驻操作教学、来源徽标与同一事实的重复状态；对话正文、文档内容、选项差异、权限边界、数据保留、生效范围、错误和禁用原因继续保留。隐藏的原有节点保留身份，旧线稿和旧草稿不重绘。

- 页面标题使用 24，设置区块标题使用 16；以分组和留白说明层级。模型能力表单取消内层底板，成功回执使用单行文字和图标。
- 模型／连接列表去掉装饰头像和重复默认徽标，将空间还给名称与来源。共享侧栏统一珊瑚色 `a` 标识及“搜索任务与资料”。
- 输入添加菜单宽 352、内边距 8、条目间隔 4；菜单项沿用共享列表规则。该局部浮层宽度不扩张为新 Token。
- 隐藏弹窗辅助说明时，取消／保存仍靠右；头像文字补全水平居中。居中继续由共享行框和对称内边距表达。
- Gateway 的禁用原因紧邻启动操作。供应商步骤示例明确选中自定义端点，与下一步示例一致；未选来源的运行态应禁用继续，静态导航不实现选择与保存。

此次只修改现有组件及页面实例，不增加 Token、组件或变体。精确修订、逐页视觉复核和明暗／宽窄检查记录在 `review.json.currentWholeUIReview`。SmallPen Web 重新载入保存包与浏览器画面验收分别记录；这些静态检查不代表 Go 产品已实现或跨平台布局已通过。

## 文字对齐契约 · 2026-10-22

先保证设计源本身成立，再考虑平台适配：需要垂直居中的控件，上下 padding 数值及 Token 绑定必须相同；文字使用共享字体／字号／字重／行高形成稳定行框，图标使用共享画布和槽位，然后由布局 center。禁止用不等的上下 padding、负 margin 或单个标签的 y 偏移制造居中。框居中后仍有一致的视觉偏差，继续检查共享文字样式或图标资产在框内的排版，不能直接归因于其他平台。

“居中”必须说明对齐对象。跨平台共享的是内容区、行框、基线与图标槽的关系；各端用实际字体测量后排版。CLI 导出的 x/y/width/height 是当前字体、配置与渲染器下的结果，不是 Android、iOS 或 HTML 必须复刻的像素坐标。普通文字的可见笔画范围不等于行框，不能按每句文案的墨迹中心反推位置，否则 `ABC`、`gyp`、数字与中文切换时会跳动。

| 组件／场景 | 共享约定 |
|---|---|
| Button、ToggleButton、Segment | 图标与文字组成一组，整体在扣除 padding 的内容区居中；文字按规定行高形成行框，图标在固定槽内居中。宽度默认 hug，需要时由父容器 fill |
| Tag／Badge、Step 数字 | 文字在自身分配的框内水平居中，行框在外框内垂直居中；固定字槽与字符的自然占用宽度分开 |
| NavigationItem、ChoiceOption、Step 标签 | 前导图标／选择符在固定槽内居中，标签沿阅读方向起始对齐；单行标签行框与槽垂直居中。标题＋说明作为整体对齐，多行场景另外声明首行关系 |
| 同一行的不同字号文字 | 按文字基线对齐；不能把所有文字框各自 centerY 当成基线对齐 |

正式 UI 沿用已有字体角色、400/600 字重与 1.5／1.75 行高，不新增文字偏移 Token。各平台组件层统一处理字体资源／回退、行间留白、字重、字号缩放和逻辑单位；控件基准高度不能在大字体模式下成为裁字上限。若一种字体在全部样本中确有一致的视觉偏差，先检查字体度量与平台的行框策略，再审阅该字体角色的统一调整，不按页面、字符串或状态逐个平移。

本轮为 8 个已有共享组件的 52 个变体补全明确的 `textStyle.textAlign=center`，范围是徽标文字、选择符、步骤数字、分段／切换标签及导航尾部计数；导航和选项的主要文案仍沿阅读方向排列。字体、垂直度量和布局几何未修改。正文 hhea／OS/2 度量的副本实验没有采用；此前对称图标的 side-bearing 修复仍保留。精确修订与本轮验收见 `review.json.currentTextAlignmentReview`。

继续复核双轴表达时，补全了 StatusBadge／Alva Badge／Alva Segment 的 `justify=center`，以及 Alva Button 的 Label `textAlign=center`。前者在 row 中明确水平居中，配合原有 `align=center` 实现垂直居中；不能依赖 hug 样本刚好没有水平剩余空间。4 个组件的 126 个已有变体只增加对齐字段，没有增加尺寸档位、修改 padding 或移动节点。原尺寸与隔离拉伸探针的同次布局规格／图像证据见 `review.json.currentTwoAxisAlignmentReview`；CLI 的轴向映射见踩坑指南。

验收分两层：结构上验证内容区和槽位、行高、基线、换行与裁剪；视觉上用固定样本集观察是否稳定、易读。样本应覆盖中文、`ABC`、`gyp`、数字、混排、回退／emoji、长文案、RTL 和字体缩放。当前 SmallPen 静态验收不代表 Android／iOS 排版或无障碍缩放已经通过；具体平台映射与 CLI 能力边界见 [踩坑指南的文字对齐规则](./CLI-GUIDE.md#文字对齐先约定对象再测量)。

## 图标对齐修复 · 2026-10-22

用户指出多个图标不居中。实测发现图标框布局正确，作者生成的字体却把所有字形的左边距设为 0：32px IconButton 中的 Plus 左偏 3.5px，20px Close／ChevronDown 左偏约 4.375px，纵向未偏移。字体 v1.1 将左边距修正为各字形的 xMin，保留原 24 单位画布、68 个图标的形状／码点／占用宽度；两包中原字体与变体 ID 保留，没有新增 Token 或逐页面位置补丁。

CLI spec 能读取精确容器坐标、尺寸和对齐规则；这些数据不能替代字体内部边距与墨迹边界检查。前轮视觉报告中“图标居中”的判断失准，本轮明确更正，旧报告保留原 revision 供追溯。修复后的 6 个对称图标／按钮样本在 4× 原生导出中中心偏差均为 0，另目视复查 21 Desktop Light、22 Narrow Light、23 Narrow Dark。68 个图标变体、3 个控件及 3 个页面配置共 74 个原生检查目标完成、0 issues，仍有透明背景等明确跳过项；此结论限于静态 CLI 渲染，不代替浏览器交互验收。

精确修订、字形度量、像素测量和写回记录见 `review.json.currentIconAlignmentReview`。页面、组件、Token 与导航数据均未更改，只有共享图标二进制及对应资产引用更新。既有区块规则继续沿用下文。

## 当前规则：按区块收敛 Token · 2026-10-22

用户明确要求整套 Token 遵循少量共同原则，不得每画一个元素就新增一项。此次把暖纸色、珊瑚强调和几何规则统一到共享 Foundation 与 23 张正式页面；两页阅读试点中的临时色值、字号、圆角也改为共享绑定。原始线稿与旧草稿继续作为历史参考。

### 标尺与区块

| 共用标尺 | 当前规则 |
|---|---|
| 间距 | **4、8、12、16、24、32**；0 表示无间距，不另设 Token。去掉 20、40 的间距档 |
| 圆角 | **4** 用于小标记，**8** 用于控件／列表项，**16** 用于内容容器／对话框；全圆只用于头像、开关、单选和胶囊 |
| 字级 | **12 / 14 / 16 / 24 / 32**；辅助信息、操作／表单、阅读正文／小标题、标题、欢迎标题。400 / 600 两档字重 |
| 行高 | 普通界面 **1.5**，连续阅读 **1.75**；图标字体自身的 1.0 排版不作为文本档位 |
| 操作 | 按钮高 **36**、图标动作框 **32**；按钮内容间隔 8、水平内边距 12，文字宽度 hug |
| 边界／阴影 | 通常 1px，焦点 2px；阴影只用于浮层，常规卡片和输入区无阴影 |
| 动效 | 一档 160ms 过渡、忙碌指示 840ms 周期；减弱动效偏好单独处理。三项为运行实现契约，静态稿没有执行动效 |

| 区块 | 内边距与间隔 | 宽高与表面 |
|---|---|---|
| 导航／次级导航 | 内边距 12；主导航条目间隔4、分组12；次级导航条目间隔8 | 主导航220／收拢72；设置、伙伴、连接列表共用次级导航208／窄窗184；导航行36、页头52 |
| 主内容区域 | 内边距 24，章节间隔 24 | 占满剩余空间；普通设置限宽 960，双列模型管理限宽 1120 |
| 内容卡片／问答／审批 | 内边距 16，内容组间隔 16 | 宽 fill、高 hug，圆角 16；无需每组内容都套卡片 |
| 列表项／菜单项 | 垂直 8、水平 12，图文间隔 8 | 宽 fill、高随内容；标题说明间隔 4，圆角 8 |
| 表单 | 字段组间隔 16，标签与控件间隔 8 | 输入宽 fill，说明和错误自然换行；不为每种字段生成尺寸 Token |
| 对话输入区 | 内边距12，区域间隔8 | 宽fill；正文至少两行56、控件36；圆角16 |
| 对话框 | 内边距 24，组间隔 16 | 宽 fill，常规限宽 720／复杂表单 800，高 hug；不再为每张表单写 800、850、860、900 的高度 |
| 输入区添加菜单 | 内边距 8，条目间隔 4 | 局部浮层宽 352、高 hug，条目复用列表项内边距 |

这些区块规则是 [design_system.py](./source/design_system.py) 中对共用标尺的组合，不是额外七套 padding、radius、color Token，也不是新的变体轴。窗口 1440／1024 和高 960 是 SmallPen 配置采样，不是运行界面的固定尺寸。图形本身的几何、1px 分隔线、进度条半径与组件目录样本宽度可以是有说明的局部值，不能为了消灭全部数字而制造 Token。

### 颜色与新增规则

表面按职责使用：canvas 是纸白工作底，surface 是阅读／卡片／浮层，sidebar 是导航底，subtle 是内嵌信息或柔和控件底；hover 复用一档加深表面。文本与图标共用 text / secondary 两个前景层级，边界保留普通与控件两档。普通 Info 复用中性前景和底色；日常成功 Notice 使用文字＋图标，警告与危险保留对应浅底，Badge 的语义浅底继续保留。透明与 scrim 用于结构和遮罩。

强调色从试点浅色 `#BE4B32` 微调为 `#B44830`，深色使用 `#E3A18B`，各自配套 onAccent；选择浅底、焦点、主按钮和阅读试点的身份／文件小标记使用相同角色。浅色强调文字在选择浅底上约 4.71:1，白字在实底上约 5.37:1，因此不用额外增加一个强调文字色。取消 accentText、focus、icon、disabled 等独立配色：状态继续由原生变体表达，不能把禁用态简单理解成降低整个控件透明度。这里保留的 20 个颜色角色包括三组必要状态色、透明与遮罩，并非 20 种装饰色。

本轮删除 19 个旧正式名称，新增一个共用的 `size.navigation` 替代 settingsNav / roster，净减 18 项。控件高度不再重复存放相同的密度覆盖。45 个正式组件和 383 个变体保持原有身份／数量；没有新增组件轴。字体尺寸与图标框尺寸仍保持正确类型，数值相同不等于语义相同。

新增 Token 前必须说明稳定的跨区块职责、现有规则为什么无法表达、已有消费者如何迁移。单页留白、单个卡片宽度、某段文字长短，优先用既有标尺与 hug / fill / maxWidth。只为一次视觉微调而新增 Token 不符合本项目规则。精确保存 revision 与本轮验收见 `review.json.currentRulesReview`；历史记录不改标为本轮结果。

本轮保存 Foundation revision 为 `fb9a9259947fd8f8ea3d979c542e5cb64d77d658cb83ac935e64d8c31e926121`，Product 为 `3cb1fb774e5ad53d4cb6931655db100d336010d53d8e7f85dce988569acb41d3`。72 页 × 明暗／宽窄共 288 个目标（其中正式页面 92 个）均完成、0 issues。Foundation 明暗共 870 个变体目标完成；初检的 4 个图标字体对比度提示已通过复用正文色修正，受影响的两类控件共 64 个主题／变体目标复查为 0 issues，另两页窄窗 Compact 检查为 0 issues。最后改动的 hover／pressed 状态没有页面或嵌套组件消费者，因此无需重画全部页面。

实际查看 8 个页面样本与最后 4 个控件状态，无遮挡／裁剪阻断；保留各自真实 revision，未把较早截图改标为最终版本。原有 49 张历史画板、3770 个页面节点、599 条连线、3 个起点、字体资源及旧组件／Token 定义保持。正式保存的 55 个组件对象与审阅副本逐 ID 相等，原组件目录顺序保留；Token 和 screen 文件与最终副本相等。Go 架构检查 276 项／3 包通过，Web 两包均重新加载成功。检查有明确跳过项，静态样本不证明真实滚动、键盘、读屏或运行性能。

## 历史：阅读与查找优先的两页试点 · 2026-10-22

用户确认先在 **21 新任务、22 对话与面板** 尝试明快、有温度的工作伙伴方向，并强调长时间阅读与查找入口同样重要。本次通过原生实例覆盖和页面装配验证这两页；其他页面与共享 Foundation 保持原有体系，尚未把新方向推广到整个组件库。

阅读正文采用 16 px；对话说明与文档段落使用 1.75 行高，标题、正文和辅助信息分层。主要操作仍沿用现有控件高度及 hug / fill 规则。宽屏正文区实测 728 px，窄窗正文区 584 px；右侧文档窄窗为 272 px，保持标题与段落配对，减少阅读时的断裂。更窄或更长内容仍需要实际产品滚动与响应式验收。

输入区保留附件、权限、模型和停止入口；对话内容放在独立 Transcript 视口。侧栏明确“搜索任务与资料”，右侧文档补充查找入口；窄窗保留“文件”“任务”短标签。阅读时不把全部控制都塞进正文，也不为了减少视觉元素把常用入口全部收成无文字图标。

暖珊瑚色 `#BE4B32` 集中用于 Alva 身份、主动作及文档类型小色块，白色图形／文字与其对比度约 4.95:1。深色下的执行图标使用主题前景；小文件图标采用白色图形与色块组合，避免直接把浅色主题的深朱橙线条放在深色底上。其余表面和文字继续引用现有语义 Token。`a` 是本轮用于观察身份感的临时字母标识，不作为已完成的品牌设计。

22 页展示“主体草稿已保存，仍在补充细节”的执行中状态；文档成果比过程说明更突出，任务停止与面板关闭保持独立。原生组件的 Running 状态、Loader 与停止图标表达设计意图；真实旋转、滚动、查找、停止和保存仍需产品实现。执行中仍可阅读已有草稿，不把“文件已保存”写成“整个任务完成”。

试点值集中在 [reading_pilot.py](./source/reading_pilot.py)，由页面定义接入。此次不增加 Token、组件或变体轴；先看两页整体效果，再决定是否把方向收敛进共享定义。写回前在隔离包编译，并保留既有页面／节点身份、实例导航与原型起点。精确 revision、明暗／宽窄检查和长内容视口检查记录在 `review.json.currentReadingPilotReview`；此前审阅仍保留原 revision。

保存的 Product revision 为 `8e56c66c3bed4e43e6f2dda45ef609a5e7caf3841230d5a7b636e7f9237744f1`，Foundation 仍为 `52254e7546dd4858dad8f2c9631f12c3081515d484e59fe35d71bc745f292bb9`。两页明暗／宽窄共 8 个常规目标检查为 0 issues，检查仍有明确跳过项。实际查看首版 4 张及修正／压力样本 3 张；修正了深色小图标对比度和窄窗页签标签。1200 × 800、回复文本扩为 4 倍时，输入区与右侧入口保持可见；裁剪区的后续内容产生 17 条 overflow / outside_root 诊断，不计作自动检查全通过，也不证明真实滚动和隐藏内容可达性。正式保存的两张 screen 内容与最后审阅的编译版本相等。其他 70 张画板、Foundation 和资产不变，原有 3763 个节点、599 条连线和 3 个起点保留，新增 7 个布局／标识／查找节点；Web 已重新加载原包。

## 正式 Design System 与页面装配

原 **00 工作台线稿** 的 15 张画板保持原始文件与连线；另在 **01 产品界面** 装配正式界面。旧视觉草稿继续保留。复用现有 Foundation 包和依赖身份，新增 `ui.*` Tokens 与 `Alva …` 组件族，不修改早期 90 个 Token 或 10 个组件的含义。正式组件从 SmallPen 原生组件目录和自动生成的 Design System 浏览，不另造组件展示页面集合。

视觉方向是纸白 / 深墨双主题，以阅读、查找和明确的行动层级为先。暖珊瑚色用于身份、主操作、焦点和必要选择，普通操作使用中性色；成功、警告、危险保留独立语义。主操作实底，次要操作柔和底色，轻操作透明。输入区保留浅边界，阴影只用于浮层；不以层层套卡片、常驻装饰动画建立层级。密集工具状态优先折叠，停止与待处理事项保持可发现。

| Token 组 | 契约 |
|---|---|
| `ui.color.*` | 直接保留有消费者的语义色；移除额外中性调色板与逐状态专用色，表面、文本、边界、焦点与状态随 Appearance/Light、Appearance/Dark 切换 |
| `ui.font.*` / `ui.line.*` | 正文、标签、标题与阅读行高；400/600 字重；中文正文、代码等宽与 Lucide 符号字体分别声明 |
| `ui.space.*` / `ui.radius.*` / `ui.border.*` | 有限间距标尺、控件/卡片/弹层圆角与普通/焦点边界；不为每一段内容制造高度 Token |
| `ui.control.*` / `ui.icon.*` | 常规操作固定 36、工具栏图标操作固定 32；导航行在 Density/Compact 下由 40 变为 36，按钮点击区不跟随缩小；文本域 96 仅为组件样本基准，实际编辑内容需自然增高 |
| `ui.size.*` / `ui.view.*` | Desktop 1440 与 Narrow 1024 的同页采样；导航、名册和右侧宽度；窄模式用图标收拢导航，仍保留可访问名称与展开入口 |
| `ui.shadow.*` / `ui.motion.*` | 表面层级、弹层阴影和未来运行实现的动效约束；静态设计不声称已实现动效或减弱动效行为 |

正式体系为 61 个 Token（此前从 78 收敛到 60，本轮仅增加共享等宽字体角色）。静态可表达的颜色、字级、字重、行高、间距、圆角、阴影和控件尺寸实际绑定到组件与页面；动效属于后续实现契约。字号与布局尺寸使用各自的 Token 类型，不能因为值同为 20 就复用同一 Token。

| 组件域 | 当前组件 |
|---|---|
| Symbols | Icon：68 个同源 Lucide 线性图标，统一 24 单位网格、20/16 显示尺寸；Loader 独立于 Refresh；跟随语义颜色，保留 ISC／MIT 许可 |
| Controls / Selection | Button、IconButton、ToggleButton、Input（文本/搜索/下拉/密钥）、Field、TextArea、Checkbox、Radio、Switch、Segment、SelectOption |
| Navigation / Shell | NavItem、Tab、MenuItem、Step、Pagination、Sidebar、SettingsNav、PageHeader、PanelHeader |
| Feedback / Surfaces | Badge、Tooltip、Notice、EmptyState、Progress、Skeleton、Toast、DialogHeader、DialogFooter |
| Settings / Models | SettingRow、ConnectionRow、ModelRow、ModelForm；模型的来源、收藏、设置、Agent 与思考等级有独立位置；新增与编辑共用表单 |
| Skills / Resources | SkillRow、ResourceRow、TreeRow、Attachment、DetailRow；来源/版本、启用和执行权限分开 |
| Conversation / Execution | Composer、PermissionMenu、ComposerMenu、VoiceInput、RunStatus、MessageHeader、ToolActivity、TaskRow、QuestionOption、Question、Approval、CodeBlock、Citation |
| Companions | Avatar、CompanionRow；长期身份与一次执行、模型路由分别管理 |

54 个正式组件共 470 个变体，覆盖适用的 default、hover、pressed、focus、disabled、loading、selected、error、read-only、empty、unavailable 与执行终态。文字内容通过实例覆盖，交互状态通过组件轴，主题/密度通过 Token 组合；不为“保存”和“取消”复制两套按钮。界面图标使用独立、可随主题着色的矢量符号字体，目录的稳定引用是图标名称，私有字符编码不是外部接口。

组件保留嵌套引用。SmallPen 的 variant 简写不能跨嵌套 Instance 修改内部节点，因此作者工具先创建本地树，再通过公共 typed operations 和真实 source node IDs 写入标准 Instance overrides；不把子组件展开复制、不直接写 canonical JSON。页面业务容器承载任意内容，避免向不支持 content slot 的 Instance 偷塞子树。

正式页面共 23 张：21 新任务、22 对话与面板、23 模型与连接、24–26 添加连接向导、27 伙伴私聊、28 伙伴资料、29 群聊、30 建群、31 通用、32 插件与工具、33 规则与 Skills、34 数据与存储、35 权限与安全、36 Gateway、37 开发者测试、38 开发者评估、39 模型选择器、40 等待回答、41 输入添加菜单、42 模型设置、43 手动添加模型。添加与编辑引用同一个 ModelForm 的 Mode 变体；两张局部弹窗分别演示新增与编辑的导航目标，避免原生 navigate 不携带 mode 时展示错误的只读状态。三个向导步骤具有不同内容；等待问题保留独立工作状态；模型浮层与添加菜单是局部装配。其余 hover/error/loading 等在组件变体里复用，不复制整套页面。

数字、账号状态、型号、可用能力和连接都是设计样例。服务不因为展示就执行、授权、收费或写入配置。模型列表单击直接选用，收藏和设置阻止选择；参数未改保持继承。每次询问留在输入区左侧，模型留在右侧。问答（含 Other）与审批独立，测试与评估独立。存储、Gateway、伙伴与能力装配保持前述产品/SDK 边界。

正式阶段的最终检查与 revision 记录在本文件后续审阅段和 `review.json`；历史报告保留自己的 revision。原型连线只演示导航，不构成真实输入、保存、聚焦、滚动、审批或执行验收。实际 ORCA 画布与跨平台表现仍须分别确认。

## 历史：克制的 Agent 界面与 Button 变体 · 2026-10-22

用户指出上一版尺寸和颜色过度展开、视觉仍像线稿。本轮取消 Button 的 Size 轴，不为三档高度展开所有排列。保留真正有交互含义的原生变体；没有另画组件陈列页，也不复制页面来展示 Hover / Loading。此前 336 个 Button 组合缩为 112 个。

| 组件 / 轴 | 当前选择 |
|---|---|
| Button · Kind | Primary / Secondary / Ghost / Danger：石墨实底、柔和中性、透明轻操作、危险语义 |
| Button · Content | Text / Leading / Trailing / Icon：文字、前图标、后图标、纯图标 |
| Button · State | Default / Hover / Pressed / Focus / Disabled / Loading / LoadingFocus |
| Button · 尺寸 | 统一 36 高，文字宽度随内容；纯图标动作 36 × 36，不设置 Size 轴 |
| IconButton | 工具栏操作 32 × 32；7 个动作状态及 Selected / SelectedHover / SelectedPressed / SelectedFocus / SelectedDisabled |
| ToggleButton | Text / Leading × 普通与 Selected 的 default / hover / pressed / focus / disabled，统一 36 高 |

图标名称、业务文案与容器宽度属于实例内容。字号和图标框虽然可能数值相同，仍使用各自类型的 Token。色彩减少 12 个原始中性色、6 个逐状态专用色；尺寸减少 Small / Large，移除未使用的 focus shadow，并清理 7 个没有消费者的行高/间距/边界/容器尺寸定义，共删除 28 个正式 Token。Hover / Pressed 复用现有表面语义；主题仍通过 Token 解析。Token 或设计变体的数量本身不是生产运行时的节点数量，运行界面只实例化当前需要的组件。

页面调整落在共享组件与 23 张正式装配里：首页的长框建议入口改为内容宽度；标题和说明更紧密，输入区有独立呼吸空间。侧栏的新任务和辅助操作回归中性，选中导航用表面与文字表现。模型设置减少外层套框；选中模型与开关避免同时铺满品牌色。资料结果以文件图标和内容层级识别，普通工具活动回到轻量的文字行。品牌细节集中在少数紫色符号、浅色选择与焦点。

性能方向只使用纯色、轻微静态阴影、圆角和已有矢量图标；输入区与弹层分层，不给每个列表项加阴影。运行时仅对当前操作的透明度或颜色做短过渡，忙碌 Loader 才旋转。此处是实现约束，尚未测量生产 Go / HTML 的帧率、内存或绘制成本。

Focus 保留两道原生内描边组成的 2 px 环与 2 px 表面色间隔，完整圆角位于控件边界内。SelectedFocus / SelectedDisabled 保留当前选择；持续开关和一次性执行结果分别表达。Loading 使用独立 Loader，不被业务图标覆盖；实现须保留动作名称、按钮宽度和焦点，阻止重复触发。Loader 旋转契约仍为 840ms，减弱动效时静止并保留忙碌说明；SmallPen 中仅展示静态状态。

Go / HTML 实现沿用原生 button、可访问名称、焦点、busy 和 aria-pressed 契约；产品层负责 DOM / MyGo 呈现，Go owner 负责动作生命周期，不向 SDK 添加样式政策。36 / 32 面向桌面，触摸界面另扩展点击区域。这里没有新增运行实现或改变已确认的产品栈。

使用公共 CLI 在隔离包编译，再通过 revision 守卫的 typed batch 写回；移除尺寸轴时迁移现有引用。原有 15 张线稿、34 张旧草稿、页面节点身份与原型导航保留。上一轮审阅记录保留在 `review.json.currentControlsReview`，本轮精确 revision 和检查证据由 `currentQuietUIReview` 单独记录。

本轮保存 Foundation revision 为 `52254e7546dd4858dad8f2c9631f12c3081515d484e59fe35d71bc745f292bb9`，Product 为 `9b88191cb683ffdc3c1f9a23fddb0296e1de0efd242157d611e5835faa94c469`。最终明暗主题的 Button / IconButton / ToggleButton / Input 共 328 个控件目标，以及全包 72 页 × 明暗 / 宽窄共 288 个页面目标（含 23 张正式页的 92 个组合）均完成、0 报告问题。另实际查看 6 个核心页面组合和 14 个控件样本，焦点环、加载图标与布局均通过；随后仅加深浅色辅助文字/图标并重新完成检查。原有 49 张参考画板文件、全部页面节点和 599 条导航连线保留；字体资源未改。检查覆盖仍有明确跳过项，不视为完整无障碍或运行性能验收。

## 正式阶段审阅记录 · 2026-10-22（按钮整理前）

最终 Product revision 为 `ceb96479cfef21311f3e4172316020d9a5ebd4c69ced7840a42d3d31cfabcefe`，Foundation 为 `128b03971ee0455cf98272dbf1c513626ff157081fa05de795161ef426b55b4c`。两包合计保留原始定义并新增 95 个 Token、44 个组件 / 269 个变体、67 个原创图标及 23 张正式画板。Product 总计 72 张画板、599 条无重复原生连线和 3 个原型起点；本轮新增 142 条代表导航和正式工作台起点。按同一 revision 重放导航是 no-op，不重复写入。

23 张页面的 Light / Dark × Desktop 1440 / Narrow 1024 共 92 个组合检查完成、0 问题；7 张关键页补查 14 个 Compact 组合，0 问题。后续模型表单、群头像、多选成员和开发者空态修正均另行检查受影响范围。44 个组件的明暗主题共 538 个变体目标检查完成、0 问题。自动生成的 Design System 明暗主题均完成；早期 30 秒超时及随后发现的 8 个目录样例问题保留旧报告，修正后最终两主题 0 问题。半透明/混合背景的对比度仍有显式跳过项，不能把“所有目标完成”说成所有检查项完全覆盖。另核对 28 对常用语义前景/背景，均达到 4.5:1，最低 4.573:1。

23 张正式画板的浅色 CLI 图像逐张查看；对话、模型管理、表单、选择器、问答及窄窗另看深色或窄窗样本。实际图像审阅修正了只读 ID 外观、表单标签对齐、空态动作和建群多选语义。它不替代连续缩放、超长内容、键盘、屏幕阅读器、真实数据与生产 MyGo 验收。

原有 15 张线稿的 screen 文件逐一 SHA-256 核对完全不变，旧连线和画板尺寸保留。共享字体扩容后重渲染的像素不再逐字节相同：每张 14–645 个像素变化，最大占比 0.047%；记录实测差异，不声称位图完全一致。字体扩容已逐字形核对原轮廓和横向度量不变，现有定义不重绘。6 个字体资源哈希、中文覆盖、Python 语法、Git 空白检查通过；本轮 Go 架构检查 276 项 / 3 包通过。新 QA 图片只存在临时审阅目录，没有再往仓库导出一套 PNG/HTML。

Web 健康检查和两份工作区投影均返回最终 revision，投影错误为 0。隔离的后台 Chrome 在软件 WebGL 下实际渲染了正式画布，23 张画板可见；选择了 21 新任务，前后两包 revision 不变。最初关闭 GPU 的探针出现 WebGL 提示，后续软件渲染探针第一次截屏超时，这些不能计作成功画布验收；最后的真实截图确认无该提示。它证明本机 SmallPen Web 能加载设计，不代替用户的 ORCA 视口或生产 UI 验收；没有启动桌面客户端。

## 当前先在 SmallPen 看什么

当前通过 SmallPen 网页服务查看原始可编辑设计：[正式产品界面](http://127.0.0.1:43128/?screen=workspace&file-id=52af4e0e-c022-5cdd-af7c-e0821c774456&page-id=14e1fbeb-6b2d-50e8-b141-c970dd88198d&layout=layers)、[Tokens 与组件库](http://127.0.0.1:43128/?screen=workspace&file-id=f9594f9e-ee60-542c-9ba5-b10d0ab0bb76&page-id=9b4f7fb5-4683-5d2f-bdda-2b86243e8997&layout=layers)、[保留的工作台线稿](http://127.0.0.1:43128/?screen=workspace&file-id=52af4e0e-c022-5cdd-af7c-e0821c774456&page-id=0edcf8ef-1e5f-5483-a7bf-cf990ffaf8af&layout=layers)。这些 file/page ID 取自实际 Web 工作区投影。用户在 ORCA 旁边打开网页即可，后续不自动启动桌面端。服务直接加载两份 SmallPen 设计包。

第一张画布 **00 工作台线稿** 包含四张工作台画板、05 模型选择器、06 输入区添加菜单、07 右侧工作区、08–11 供应商与模型设置，以及 12–15 伙伴与群聊。**框架 / 03 成果展开** 展示真实工作台中的右侧文件标签；**选择器 / 07 右侧工作区** 在一张组件画板内对照空态、已开标签的后台任务空态及添加标签菜单，不复制整套应用页面。

本轮参照并实际查看了 [Codex 官方工作台截图与发布说明](https://learn.chatgpt.com/docs/changelog)、[Claude 的对话与浏览器并排界面](https://claude.com/product/cowork)、[Cursor Agents Window 的会话与文件并排界面](https://cursor.com/docs/agent/agents-window)。这些是结构参考，不代表三个产品具有相同布局或能力；以下是 Alva 的设计判断：

- 左侧主要服务项目与会话：新任务、搜索、项目及其会话。自动化、自定义保留少量入口，设置在底部。
- 新任务以输入框为中心；进入任务后变成连续对话，输入框固定在阅读区底部。工具活动折叠在消息里。
- 右侧是可收起的标签工作区，承载文件、审查、子任务、后台任务、浏览器、终端和运行详情。空态与顶部加号都能打开面板；点击成果直接定位到文件标签。收起后恢复对话宽度，已打开视图与任务状态各自保留。
- Agent 与模型合并为输入区的一个入口；选择后的示例为 `ALVA · GPT · 高`。权限另有自己的入口。连接、插件、规则、存储、Gateway 和开发者配置进入设置或自定义，不铺满主导航。
- 临时协作跟随会话的成员与活动；长期伙伴和群聊另有入口，不能混同临时 Subagent。待回答问题和审批跟随任务状态。
- 导航可折叠，详情可关闭；任务状态、停止和待处理事项保留可找回入口。设置页保留返回运行中任务的入口。

原型连接演示：新任务 → 对话 → 点击成果 → 关闭成果；底部设置 → 模型与连接 → 添加供应商三步 → 返回管理页；模型选择器的“配置模型”进入管理页，“模型设置”进入共用模型表单。侧栏伙伴入口可查看私聊、伙伴资料、建群与群聊。原生连线只演示代表路径；表单、收藏、筛选、保存和真实执行仍是设计契约。

## Agent 与模型的联合选择

当前采用用户提供的两张参考截图的模型优先结构，替代先选 Agent 的三级向导。Agent 成为模型的执行选项。

- 主入口直接搜索、浏览模型。左侧是收藏、全部和 Provider 具体连接筛选；线稿中的中文标签和 O / A / G / R 是图标占位。模型行显示名称、连接来源、能力摘要，以及实际预选的 Agent / 思考等级，不要求先了解或选择 Agent。
- **单击模型行就选中并关闭选择器**，没有额外确认或“选用模型”按钮。模型行的一次选择包含模型连接、有效 Agent 和显式修改的参数。没调过的项保留原值和继承关系，浏览不会写入默认覆盖值。
- 悬浮或键盘聚焦模型行时显示收藏、设置操作；点击设置后，在该行左侧打开并保持配置浮层。点击收藏只切换收藏，点击设置只打开配置，均不触发模型选择。浮层可通过键盘操作，关闭后还原焦点；屏幕左侧空间不足时翻转位置。
- 配置浮层包含模型和连接身份、思考程度、**当前模型与连接可用的 Agent**、Context、视觉开关及模型设置入口。Agent 可用集合由真实接入和配置返回，不能因为模型同名或同属一个厂商就宣称兼容。线稿展开 GPT 的设置，只演示 ALVA / Codex / Pi 三个候选，不声明产品真实兼容性。
- 一般优先保留该模型连接上用户选过的 Agent；其次沿用仍兼容的当前 Agent，或使用明确配置且兼容的应用默认值。列表始终标明有效 Agent。没有有效候选时要求在设置中选择或配置，不暗中换到另一条连接。参数和 Agent 的预选修改留在选择草稿，单击模型行一次提交；关闭丢弃未提交草稿。执行中的更改只对后续轮次生效。
- 模型行和收藏身份是 **Provider 连接 + 模型 ID**，同一个模型连接不会因为支持多个 Agent 而重复成多行。不同连接的同名模型保留来源，不按显示名合并。最终执行选择额外包含有效 Agent 与显式参数；收藏不随 Agent 切换丢失。
- “模型设置”允许之后修正默认配置和能力标记。会话参数与模型默认值分别标明作用域；例如漏配视觉能力时可更正标记，选过的模型仍能重新打开调整。能力标记不创造模型或连接的实际能力，Context 预算按真实限制校验。当前原型链接进入 11 共用模型表单的示例；真实携带连接、模型身份和返回来源的路由仍待实现。

目录数据和模型别名均为示例，**不声明 Claude、Codex、Pi 的真实可用连接、具体型号或思考参数支持**。实际可用集合须由产品接入服务返回，候选失效应明确反馈，不能悄悄改选另一条连接。这些目录、选择和 UI 状态属于产品外层，不向通用 kernel / Agent SDK 增加界面或厂商政策。

SmallPen 只保留一张选择器画板，生产实现一个模型选择组件及其配置浮层。浮层靠近屏幕边缘时翻转位置或限制高度，不撑大主选择器。原型的打开 / 关闭和设置入口使用原生 navigate 连线；这不验证真实模态背景、Esc、键盘焦点、悬停边界、收藏或点击选择。运行中会话、设置作用域和已选值恢复留给实际组件验证。

本轮模型优先 revision：`6c63c17bc5ef60afc5e3d32c552c05eac5d81fe40cc4f197fa5d48082b586eb6`。主选择器为 560 × 484，同一张 880 × 560 画板额外展示左侧配置浮层。浅色桌面检查完整完成，0 问题；实际 CLI 渲染已人工查看，之后仅去除重复原型连线。当前记录见 `review.json.currentModelFirstSelectorReview`；各轮旧方案及检查保留原 revision，不能用于声称当前原型具备动态选择能力。

第二张画布 **旧视觉草稿（暂不评审）** 保留早期细化内容。原先暂缓的 Foundation 在本轮正式扩展；旧定义仍按历史用途保留。下面的功能覆盖、配置和交互清单作为后续细化的候选约束，**产品行为继续遵循线稿契约，正式视觉以 01 产品界面和 Alva 组件族为准**。旧 PNG、离线预览及其报告只对应原有细化草稿，不是本轮交付入口；本轮不新增仓库内导出集合。

工作台初版的审阅 revision 为 `953730354dfdafef91e06d22f983c71a09b5c084c960f03fdfc436a553a0fa58`：当时四张浅色桌面线稿完整检查、0 问题、15 条原生连线，四张渲染已人工查看。当前选择器记录见 `review.json.currentModelFirstSelectorReview`。窄窗、主题、细部交互和生产运行留待继续验收。当前使用 SmallPen Web 服务；ORCA 的 iab 自动创建接口不可用，用户可直接点击上方链接打开，浏览器画布显示尚未自动确认。桌面旧报告保留原 revision。

## 供应商、连接与模型添加

23 模型与连接是正式应用级管理页：全局设置导航 → 具体连接列表 → 当前连接的模型目录。连接列表显示账户来源、就绪状态与启用数量；右侧提供连接设置、目录刷新、手动添加、模型设置和启用开关。列表里的就绪、测试通过、型号与能力都是示例，不是本机探测结果。旧线稿04及08–11不再定义当前添加流程。

| 阶段 | 内容与完成条件 |
|---|---|
| 24 选择供应商 | 搜索、API服务预设、自定义、本地HTTP、本机Agent账户。选择直接进入对应表单，不需继续按钮；预设只初始化数据，不授予权限或启动服务 |
| 25 预设连接 | 默认Key快捷路径；连接设置展开名称、认证、协议和完整端点。测试为可选，反馈复用已有Button/Field/Notice状态；错误保留草稿 |
| 26 自定义连接 | 同一个ProviderForm的Custom结构，完整字段直接显示。模型区复用ProviderModels，图中Ready是样例状态，不是强制第三步 |
| 25/26 内联模型区 | 可选获取、搜索、勾选、刷新和手填准确ID；尚未获取、加载、成功、空目录、不支持、失败分开。目录不可用仍可手填并保存为未验证，不强制 `/models` |
| 42/43 共用模型表单 | 添加/编辑准确ID、显示名、用途、Context、视觉输入、工具调用、思考默认值与对话可见性；编辑已有模型不能靠改名改ID |

连接以稳定 ID 定位，模型以连接 ID + 服务要求的原始模型 ID 定位。别名只影响显示；同名模型来自不同连接时分别保留。保存连接时一次提交；测试、获取目录与保存相互独立。目录失败可保存为未验证并保留手动配置，不能伪报可调用。返回来源选择保留草稿，取消不改变已存在的配置；编辑已有连接携带现有ID，不走创建重复连接的路径。未保存离开的确认策略仍待确定。

“连接就绪”“模型已启用”“当前 Agent 支持这条路由”是三项独立事实。禁用模型影响之后的选择，不取消当前轮次；刷新目录保留用户启用选择、手动条目和显式覆盖，删除/改 ID 必须说明对收藏与默认值的影响。目录为空、刷新失败、尚未请求目录分别显示，不能都画成暂无模型。视觉输入仍属对话模型能力；图像生成是另一用途，不混进聊天选择器。没有价格或能力证据时标记未知，不能按名称猜测。

实际 Go provider 当前有四种原生协议：`openai_chat`、`openai_responses`、`anthropic`、`gemini`。其 `Config.Endpoint` 是**完整原生请求地址**；25预填 `https://api.openai.com/v1/responses`，26示例为 `https://api.example.com/v1/responses`，不声称当前代码会给 Base URL 自动补路径。Gemini的模型相关路径由产品使用准确ID解析，不能把未解析模板传入SDK。地址传输按用户 2026-11-07 的决定：允许任意主机使用 http（局域网与自建网关不必先套 TLS），选择 http 是配置者的显式行为；非 loopback 的 http 要给出明文传输警告（目前该警告只在 MCP 设置页实现，模型连接页尚无）。地址仍不得带凭据、查询或片段；OAuth 授权端点仍要求 https（本机回环地址除外）。目录发现是产品适配能力，不是这些协议必然提供的统一接口。当前 bootstrap 通过显式配置装配路由，**尚无本轮管理页对应的供应商/模型 CRUD 后端**；后续在产品外层和共享存储插口上实现，不给 SDK 增加供应商目录政策。

API Key、本地 HTTP 服务、本机 CLI 订阅账户分成不同来源。只有用户主动点击检查与连接，才能验证本机账户；订阅账户绑定对应 Agent 的账户路由与计费，不提取为通用 API Key。跨 Agent 可用性由实际 adapter 返回。25/26画的是API Key示例；本地无认证时按数据隐藏Key，账户登录由独立认证分支实现，24入口不假装已经发现登录。密钥只通过宿主安全存储和引用使用，不进入普通配置导出、聊天或日志；自定义请求头同样处理秘密值。

正式原型连线为23→24，24的OpenAI→25、自定义→26，25/26返回24或保存/取消/关闭→23；42/43保留独立模型编辑。它不执行测试、枚举、勾选、保存或草稿保持。其他预设以同一表单的实例数据装配，账户检查、编辑调用来源与真实返回由产品维护。共享外观只在Foundation定义一次，具体触发与异步规则见原生providers/navigation备注；旧线稿三步连线保留为历史，不再作为当前实现契约。

## 长期伙伴与伙伴群聊

用户明确希望有长期伙伴、给伙伴挂 Skills，并能把它们建群。参考固定于 Cindy commit `62236d3d499b0f9fa5607bb030afc9cc46e3cb2c`，实际阅读了 [伙伴运行时契约](https://github.com/makecindy/cindy/blob/62236d3d499b0f9fa5607bb030afc9cc46e3cb2c/docs/product-rules/cindy-bots-runtime.md)、[伙伴群聊契约](https://github.com/makecindy/cindy/blob/62236d3d499b0f9fa5607bb030afc9cc46e3cb2c/docs/product-rules/bot-group-chat.md)、[能力解析实现](https://github.com/makecindy/cindy/blob/62236d3d499b0f9fa5607bb030afc9cc46e3cb2c/apps/desktop/src/main/maker-ipc/botProfileRuntime.ts)、[技能归属服务](https://github.com/makecindy/cindy/blob/62236d3d499b0f9fa5607bb030afc9cc46e3cb2c/apps/desktop/src/main/maker-ipc/botSkillService.ts)与 [群聊执行服务](https://github.com/makecindy/cindy/blob/62236d3d499b0f9fa5607bb030afc9cc46e3cb2c/apps/desktop/src/main/maker-ipc/botGroupChatService.ts)。未安装或实际跑 Cindy，其跨账号 Chat Server 仍有部署/兼容验收要求，文档不等于端到端验证。

Cindy 将永久 Profile、长期主任务记录、可压缩模型上下文分开；伙伴有自己的技能，外部 Skill 从实际目录解析，群聊还有独立会话与确定的发言编排。这些适合借鉴。它关于“Core 承载群聊”的说法是它的应用划分，不能推导为给 Alva 通用 kernel 加功能；它的自动学习、默认工具继承、跨渠道记忆政策也不直接照搬。

| Alva 概念 | 身份和归属 | 在哪里实现 |
|---|---|---|
| 伙伴 | 稳定 partner ID、owner、资料版本、名字、职责、默认模型路由、Skill 引用与记忆命名空间 | 产品服务持有配置；能力由既有 Plugin/harness 装配，模型/Agent 可换，身份不换 |
| 私聊 | 用户与该伙伴的长期记录；完整历史和当前压缩上下文分开 | 复用会话与引用检索；压缩/重建不删除时间线 |
| 群聊 | 稳定 group ID、成员、独立多作者消息和参与规则 | 产品协作服务；成员在 group × partner 范围有各自执行会话，不把私聊直接拼进群 |
| 群任务 | 一次明确的分工与执行，包含状态、依赖、审批和收尾 | 复用现有任务/TeamRun/子执行，不让群消息本身成为另一套执行 runtime |

12 展示伙伴与群的同一名册、伙伴私聊、当前使用的 Skills 和设置/建群入口；13 展示伙伴资料与能力；14 展示两位伙伴在群中的署名回复；15 展示选择成员、回应模式、顺序、预算和共享范围。普通会话的模型选择仍只选择执行路由，长期伙伴入口另有语义。临时 Subagent 不是长期伙伴；可从伙伴发起临时任务，但不能混用身份和记录。

Skill 只引用已有加载与插件能力：优先显示名称、简述、来源、固定版本/哈希与可用状态，正文按需加载。引用身份用来源命名空间 + 稳定 Skill ID；伙伴自有 Skill 另有 owner + ID，名称重复不静默覆盖。共享 Skill 的移除只取消此伙伴引用，编辑共享源需明确权限；日常修改默认另存自有版本并保留来源。依赖丢失或当前 Agent 不支持时说明不可用，不把“配置了”当作“已挂载”。更新资料在安全轮次边界生成能力快照，当前执行不中途偷换。

每个伙伴可以选择自己的 Skills、Tool/MCP 集合与权限，但 Skill 文本不会授予工具或文件访问权。实际能力同时受安装/启用状态、账户授权、执行平台、工作区和本轮权限约束。不因进群就继承其他成员的连接与授权。自学先形成建议，由用户查看差异和确认保存；支持关闭、编辑、删除，不默认把每次群聊转成永久技能。

群默认仅 @ 回应，用户也可选择所有成员；初版按确定顺序发言，每位一条、整轮预算受控。用户消息才开新轮；伙伴互相提及不自动无限续轮。并行独立回答和依赖分工将来分别采用显式模式；不能让提示词代替调度、额度或停止。消息按低信任内容处理，不冒充用户审批。待回答问题/权限必须绑定原执行和对应伙伴，用户真实应答后继续。停止由原 owner 取消并等待清理，UI 隐藏/关页不停止执行；失败、待确认、正在收尾与完成分别显示。

**群的可见范围默认更窄**：只加载本群已授权消息、共享文件与明确允许在群里使用的 Skills；不自动加载伙伴私聊、私人记忆或其他群的资料。向群发布文件与写入长期记忆分别授权/留痕。更换成员/权限后冻结新的有效快照；旧执行结果按原快照核验，不能投递给无权的新观察者。云盘、内存或本地存储继续由共享存储能力插口选择，不把伙伴服务绑定文件目录或 SQL。

这些是新产品目标，现有 Go 伙伴 Profile/群聊服务还没有因线稿而实现。当前原生导航只演示查看资料、建群和群记录；Skill 选择器、群设置编辑、私聊发送/保存、权限审批和完整按调用方返回仍待真实 UI/服务验证。当前轻量页不引入独立多账号聊天服务、远端自动迁移、自动分享凭据或新的通用执行抽象。

本轮保存 revision：`5f256a1727b22e05f65dda64634c8114e59dd8150630e7d7887b8f261d04e207`。13 个受影响的浅色桌面目标完整检查，0 问题；9 张新增/重做页面的实际 CLI 图像已逐张查看，最终 09/14 的说明和轮次结束操作又重新检查。其余范围保留各自实际审阅 revision，不重标旧证据。当前总计 49 张画板（含未改的 34 张旧稿）、457 条原生连线，无重复；三个连接 helper 重放均不写入。具体记录见 `review.json.currentProviderCompanionReview`。

SmallPen Web 的 `/health` 正常，工作区 API 已返回同一最终 revision、49 张画板并包含伙伴页；这证明服务加载了新设计，不代表 ORCA 实际画布已自动目检。Go 架构检查 276 项 / 3 包通过，Python 语法、名称唯一、字体覆盖与文件哈希通过。只更新设计及作者工具，没有实现生产 Go 界面或新增仓库导出集合。

## 右侧标签工作区

本轮参考用户给出的右侧空态、添加标签菜单、后台任务空态三张截图，形成 Alva 自己的共用工作区结构。右侧整体是一个容器，文件预览只是其中一种内容。页头统一使用“面板”作为入口。

- 标签栏包含可关闭标签、加号、弹出、专注、收起。标签多时横向滚动，加号和布局操作保持可找到；正文各自滚动，切换标签保留位置、选中项和未提交编辑。
- 尚未打开标签时显示“开始一个面板”，列出可用类型。最后一个标签关闭后回到这个空态；收起整个右侧后，左侧对话恢复宽度，页头“面板”可找回。两者都不清空会话或停止任务。
- 加号打开紧凑的“添加标签”菜单。菜单的已打开状态来自真实面板实例；点击已打开的单例类型切回对应标签，不再创建一个副本。示例中的文件、子任务和后台任务都标为“已打开”。
- 点击消息中的文件或成果，会打开 / 聚焦文件面板并定位具体资源，不替换其他标签。后台事件可显示活动或未读标记，不抢走用户当前查看的标签。

| 面板类型 | 内容与作用域 | 打开与运行的边界 |
|---|---|---|
| 文件浏览器 | 当前项目的文件索引、选中文件、预览 / 原文 / 来源；文件正文按需读 | 打开面板不增加文件授权，也不把文件自动投递给模型 |
| 审查 | 当前任务 / 分支的文件变更、diff 与处理状态 | 查看与应用 / 拒绝变更分别操作，不充当权限审批；与运行详情分开 |
| 子任务 | 当前会话的 Subagent、派生关系、进度与结果 | 打开只观察已有子任务，不新建 Agent；停止必须操作对应任务 |
| 后台任务 | 当前会话的后台作业、状态、日志与结果 | 无作业、加载中、失败、不可用分别呈现；不能把获取失败画成“暂无作业” |
| 浏览器 | 当前项目可用的浏览器视图 | 先恢复已有视图或显示新建入口，不因恢复标签自动重放浏览动作 |
| 终端 | 当前项目的终端会话与输出 | 连接已有资源或明确新建；恢复布局不自动运行命令，关闭视图不等于终止进程 |
| 运行详情 | 实际可见的模型、工具、中间件与子执行记录 | Inspector 只观察授权范围内的事实，不获得执行所有权；外部 Agent 没有的记录不能伪造 |

工作区按类型、项目 / 会话范围和资源身份识别实例。文件浏览器、审查、子任务、后台任务、运行详情按各自范围复用；浏览器、终端需要多实例时显式区分实例身份。布局偏好属于窗口，具体标签和定位属于对应任务 / 项目，切换会话时恢复其自己的视图，不串用另一会话的记录。

“弹出”移动当前视图到独立窗口；“专注”临时扩大右侧并提供还原双栏；“收起”只隐藏容器。标签关闭、窗口关闭、隐藏、移动和退订都不调用 Stop。真实停止、终止终端、取消子任务由原执行 owner 处理并反馈收尾状态。未保存编辑的处置由对应编辑器负责，布局操作不静默丢弃草稿。

1440 宽线稿保留 530 宽右侧、666 宽左侧阅读外框；右侧可调宽。窗口不能同时容纳阅读区与面板时，改为抽屉或专注视图，不将对话挤成窄条。新增面板先声明自己的尺寸、渲染和平台能力；浏览器 / 终端不可用时显示原因。

面板目录及其工厂属于产品 UI 装配，提供类型身份、标题、能力条件、实例身份规则、视图和操作入口。现有 Tool / Plugin / Middleware 的职责不变，不为标签页给 kernel、Agent SDK 或 Bus 增加万能接口。后续资源监视器或可信插件面板接入同一容器；未知 / 不可用类型保留可解释的占位，不丢持久视图信息。原始模型输出不能直接注册为可执行 UI。真实 MyGo 渲染、浏览器 / 终端嵌入、独立窗口与性能仍须实现后验收。

本轮 revision：`d2a575f17f4b731ad7b3357d0829bfa38749fcaf8d776ed0ce7404771d6a476f`。实际文件工作区与局部组件状态、统一页头入口及已打开标记已通过 CLI 渲染查看，之后仅恢复收起的原型连线；最终范围检查见 `review.json.currentRightWorkspaceReview`。原型连线只演示已有成果工作台 → 添加面板示例 → 文件标签 / 收起；截图中的弹出、专注、标签关闭、任务观察与停止关系是目标交互，尚无运行实现。

## 输入区与加号菜单

新任务、对话进行中、成果展开三处共用输入区，控件顺序统一为 **加号 → 每次询问 → 弹性空白 → 模型 → 发送 / 停止**。权限与模型交换位置，不改变当前值或执行所有权。

加号打开锚定输入区上方的添加菜单。空间不足时限制菜单高度并内部滚动，必要时翻转；不撑开消息列表。点击外部或 Esc 关闭并恢复焦点，保留输入草稿，不发送消息。菜单条目由当前 Agent、应用形态和已启用插件的真实能力决定；不可用项应显示原因，不能只添加提示词就冒充能力已经开启。

| 菜单项 | 打开后填写或选择 | 生效和撤回 |
|---|---|---|
| 添加文件、图片或视频 | 系统 / 来源选择器；选中后显示附件名、类型、大小和准备状态 | 加入当前消息草稿，不自动发送。可从附件标记移除，失败保留可重试入口；发送前按实际大小、格式和处理能力校验。原件与派生物分别记录，模型按有界引用取用；未解析的视频不能冒充已理解 |
| 新建目标 | 目标名称、完成标准、项目 / 会话范围；明确点击创建 | 创建不等于启动执行。取消不创建；创建后的目标以可移除引用关联到草稿，后台运行须有明确入口 |
| 计划模式 | 选择下一轮先规划的执行策略；展示选中状态 | 输入区保留可取消的计划标记，当前执行不中途改写。只读约束由执行 owner 实施，开始执行仍遵守真实审批政策；不把计划模式当作自动授权 |
| 协作模式 | 选择成员、分工和当前 Agent 支持的协作方式 | 输入区显示可编辑的成员 / 模式标记；不因点击菜单就启动 Worker。提交任务后按选定方式装配；成员权限由各自 owner 校验 |
| 添加目录 | 选择来源 / 目录，查看引用范围和现有访问权限 | 目录引用先进入草稿，不把整棵目录正文塞给模型，不自动获得递归写权限。需要新增授权时走现有宿主审批；移除目录标记只移除引用，不伪称撤销了既有 grants |

文件拖放、粘贴和菜单添加采用同一附件准备与去重入口；选择器取消时已有输入不变。附件与模式在发送前均可撤回。切换模型或 Agent 后重新校验资料与模式兼容性，明确呈现不支持或待处理状态，不丢弃草稿。设置中的默认值与本条消息的选择分别保留作用域。

本轮 revision：`0dcced5f62a24d0fe0f3cf2372e54be6ee86b46b755b0f272792a14cf16b6659`。三张含输入区的工作台与一张添加菜单，共四个浅色桌面目标检查完整完成、0 问题；添加菜单和成果展开窄栏的实际 CLI 渲染已人工查看。原型只连新任务加号 → 06 添加与模式 → 关闭返回新任务；其余条目、系统选择器、模式标记、创建表单和真实执行仍待实现，不用静态页面导航冒充文件或权限操作。记录见 `review.json.currentComposerMenuReview`。

## 从哪里看

- `alva-foundation.smallpen/`：唯一 Token、主题与共享组件来源；在 SmallPen 查看自动生成的 Design System，不另画一套会失配的组件陈列页。
- `alva.smallpen/`：当前工作台线稿与旧视觉草稿分画布保留；每组的业务画板及原型连线在各自同一画布内。相对依赖旁边的 Foundation，移动时两个包一起移动，并保留 `fonts/OFL.txt` 及字体归属说明。
- `exports/`：实际 SmallPen 渲染的 PNG、约束说明与线框证据。检查结果和精确 revision 见 `review.json`。
- `source/`：CLI 意图与作者辅助脚本。SmallPen App 中的后续编辑是设计事实；重新生成前须用 `smallpen changes PACKAGE --since REVISION --json` 阅读差异，不能用旧脚本覆盖。

页面里的项目名、任务、文件、模型路由和运行结果均为示例数据。原型导航不连接真实账户、不保存配置、不发起模型调用，不代表控件已经有实际业务行为。

作者意图与日志保留早期试写和拒绝记录，不是按文件顺序全部重放的迁移脚本；当前可编辑设计以两份保存的包为准。`workbench.py` 维护四张工作台、输入区和右侧共用外壳；`model_picker.py`、`composer_menu.py`、`right_workspace.py` 维护各自局部组件；`provider_setup.py` 维护保留的历史线稿；当前正式连接流程由 `product_pages.provider_setup()` 与 `design_system.provider_definitions()` 维护，`companions.py` 维护伙伴身份、Skills 与群聊布局。写入必须提供已审阅的 `--since` revision；工作台可按 `--page` 限定，`--setup` 只用于初次转换，不可重放。`review.py` / `preview.py` 仍只审阅旧细化草稿，不覆盖本轮线稿，也不应重标旧导出的 revision。

## 设计选择

首个平台是桌面：1440 × 960 舒适布局；用同一页面检查 1024 × 960 窄窗口、深色和紧凑密度。暂不设计手机版。两个宽度是审阅样本，生产实现仍须验证连续缩放、长文本和系统缩放。

正式界面采用中性底色、紫色主操作与克制的状态色；早期蓝色 Token 保留历史用途。主次文字分明，当前任务比技术信息更突出。导航不塞进所有设置；用户先知道正在做什么，再按需看执行细节。中文字体使用随设计提供的 Noto Sans SC 派生子集，许可证与来源在 `fonts/`。线稿中的图标占位保留；正式组件使用统一矢量符号，并约定生产实现的语义标签和键盘焦点。

两份包均使用 WOFF 资源：当前 CLI 支持单独 TTF，而已安装的 alpha.15 App 要求 WOFF。Product 的 400 字重先为选择器和视频菜单扩至 821 个码点，供应商与伙伴内容再补 27 个，共 848 个码点；此前所有字形轮廓和横向度量核对不变，TTF / WOFF 的对应表一致。供应商阶段的 Foundation 和 600 字重曾保留原子集；正式组件阶段两包都更新为 400 字重 877、600 字重 875 个码点，旧轮廓与度量继续逐项核对不变。不向设计包写入临时转换缓存。Product 导入同名字体资源，避免跨 Foundation 的正文选字回退；Token 和共享组件定义仍由 Foundation 拥有。

Token 分三类：原始色板；有语义的色彩、文字、间距、边框、圆角与状态；共享控件和区域尺寸。组件实际绑定 Token。主题只改变值与可见性，不隐式切换组件树。不要为每块示例内容测得的高度创建 Token；卡片和文本随内容增长。

| Token / 维度 | 规则 |
|---|---|
| Appearance | Light / Dark；状态同时有文字，不能只看颜色 |
| Density | Comfortable / Compact；控件 36 / 32，导航行 44 / 36；正文保持可读 |
| Viewport | Desktop / Narrow；导航 248 / 72，窄模式收起标签与最近列表，保留入口 |
| 字体 | 正文 14、辅助 12、控件 13、标题 20；常规 400、强调 600 |
| 空间 | 4 / 8 / 12 / 16 / 24 / 32；页面阅读区域不无限拉宽 |
| 形状 | 控件圆角 6、容器 10 / 14；主操作与普通动作分级 |

SettingRow 由父卡片提供背景，不为通过孤立检查而强加不必要的底色；更换承载背景要复核对比度。按钮覆盖默认、悬停、焦点、禁用、加载；输入覆盖默认、焦点、错误、禁用；导航与页签有选中态；工具活动有执行、成功、失败态；选择项区分单选、多选和 Other。原型里的开关表达目标呈现，不绕过后端能力与权限检查。

## 页面与入口

| 页面组 | 首层入口 | 主要内容 | 次级入口 / 弹出 |
|---|---|---|---|
| 工作区 | 左侧项目 / 会话、新任务 | 项目与会话层级、创建项目或独立会话 | 创建不发消息 |
| 对话 | 当前会话、最近列表 | 消息、工具摘要、动态内容、输入区 | 模型选择、会话设置、详情、来源、展开内容、快速打开 |
| 插件与能力 | 自定义 / 设置 | 已安装 / 已启用 / 界面扩展，运行时与授权范围 | 详情、版本、配置、审阅并启用；保存不自动加载 |
| 资料与记忆 | 会话资料 / 成果侧栏；设置中管理数据 | 原件、派生物、记忆、投递记录 | 预览、来源和处理、存储范围；预览不投递模型 |
| 协作 | 当前会话的成员 / 活动入口 | 协调者、成员、独立会话和队列状态 | 成员详情、会话、继续 / 停止队列；后续细化 |
| 自动化 | 主导航 | 暂停 / 启用的计划、下一次运行、结果 | 计划编辑与启用审阅、运行记录 |
| 等待任务 | 会话状态 / 消息内待处理卡片 | 当前项目的持久问题 / 审批 / 恢复事实 | 汇总入口后续细化；处理、保存、继续分别操作 |
| 设置 | 左下常驻入口 | 见配置矩阵 | 各设置页保持明确的作用域与来源 |
| 开发者选项 | 设置 → 开发者 | 接入测试、模型评估两页，各自计划、预算、运行、报告 | 测试工具名称和参数契约；评估版本化数据集和质量，不混为一种结果 |

功能映射：Session settings → 当前会话 / 设置；Inspector → 右侧“运行详情”；等待任务 → 当前会话状态与持续提醒，汇总视图待细化；Handoff evidence → 运行详情 / 引擎分支与交接；Media evidence → 会话资料 / 投递记录；Project skills → 自定义 / 规则与 Skills；Collaboration → 会话活动与子任务面板；Automations → 自动化入口。直接桌面与 Hub 共享视觉语言，面板按真实能力显示；只有 Hub 才提供多引擎分支及其产品会话功能，不能给 direct 入口加 EngineRuntime 依赖。

## 配置矩阵

| 分组 | 配置内容 | 权威 / 作用域 | 生效与反馈 |
|---|---|---|---|
| 外观与布局 | 明暗、密度、字号、导航、面板、恢复默认 | 应用偏好；面板位置是窗口状态 | 即时；恢复布局不能清空会话 |
| 模型与路由 | 执行引擎、连接、模型、账户、计费、分支、fallback | 应用连接 / 项目默认 / 会话覆盖；分别显示来源 | 本轮执行不被隐式换引擎；切换与自动回退都先展示目标和费用归属 |
| 工具与连接 | 文件、命令、浏览器、MCP、子 Agent、交互提问 | 项目装配；当前会话能力快照 | 检查、授权、固定版本后下一轮生效；关闭 UI 不撤销或批准正在进行的请求 |
| 插件 | Go 内置、WASM、JS / Python AEP；工具、服务、上下文、可选 UI | 插件身份与版本、项目启用项、宿主授权 | 执行运行时与 UI 运行时是两件事；WASM 插件不代表可任意画 HTML |
| 规则与 Skills | Alva 规则、兼容来源开关、Skills、Agent 定义、加载预览 | 明确目录与规范身份；项目来源可审计 | 按统一身份去重；显示胜出来源、覆盖与错误，不按显示名悄悄合并 |
| 资料与存储 | 后端能力、路径 / 配额、原件、记忆、保留策略、导入导出 | 外层装配同一存储能力插口；当前项目 / 会话归属 | 云盘、内存等由组件提供；切换需要迁移方案、失败保留原归属 |
| 权限与安全 | 模式、授权根、域、现有 grants、待审批项、撤销 | 宿主 / 产品的真实授权所有者 | 按具体请求响应；Other 是回答，不是授权；必需审批提醒不是可关闭偏好 |
| Gateway | 状态、监听地址、协议、上游路由、凭据引用 | 本机产品服务；与模型连接设置分开 | 保存配置不等于启动；暴露网络接口须显式审阅；不显示已有密钥原文 |
| 高级与诊断 | Hooks、错误、日志、媒体处理、浏览器环境、AEP 诊断、导出 | 应用 / 项目服务；日志脱敏且有范围 | 默认只读检查；执行钩子和启动连接是显式动作，错误有重试与定位 |
| 开发者 | 测试计划 / eval 数据集、路线、预算、超时、报告 | 独立作业 owner；不依赖当前聊天 | 预览不调用模型；Run 才执行；Stop 明确取消作业，关页面只结束观察 |

每个可编辑项都有“当前值、来源、作用域、何时生效、能力是否可用、保存结果”。本阶段不发明会在核心 SDK 中落地的全局 Settings 类型；产品按现有 owner 调用自己的服务，必要的新产品能力再单独实现。

## 区域的显示与退出规则

| 区域 | 默认 | 可以隐藏 / 折叠 | 恢复入口 | 弹出 / 独立窗口 | 滚动与约束 |
|---|---|---|---|---|---|
| 窗口栏 | 常驻 | 不能隐藏系统关闭与移动窗口能力 | 系统窗口操作 | 系统全屏 | 原生拥有拖动、缩放和系统菜单 |
| 导航 | 展开 | 可折叠到图标轨，可进入专注模式完全隐藏 | 页头按钮、快捷键、命令面板 | 窄窗口用导航抽屉 | 最近列表独立滚动；隐藏不能清掉选中会话 |
| 页头 / 会话身份 | 常驻 | 可收起路径、次要操作文字 | 会话菜单、详情 | 会话设置可开抽屉 | 当前归属始终明确；长路径省略并可复制 |
| 任务状态 | 有活动 / 等待 / 失败时可见 | 可收起活动明细，状态和 Stop / 继续入口不可消失 | 输入区状态、消息内待处理卡片、会话标记；设置页可返回活动任务 | 点击打开任务详情 | 信息不是只放在可被关掉的 toast 中 |
| 对话阅读区 | 主区域 | 消息工具明细可折叠；原文与结果引用可找回 | 消息展开、来源 | 动态内容可扩展至面板或独立窗口 | 一条连续主滚动链；流式增长保留阅读锚点 |
| 输入区 | 当前可交互会话下方 | 历史只读视图可收起；不能丢草稿 | 返回会话、输入快捷键 | 可展开长输入 | 输入内部最大高度，达到上限才内部滚动 |
| 右侧工作区 | 默认收起 | 可收起、关闭标签、切换、调宽 | 页头“面板”、消息中的资源入口 | 标签可按平台能力弹出；可专注 / 还原；窄时抽屉 | 保留左侧阅读宽度；每个面板独立滚动，切换保留位置；关闭不停止执行 |
| 动态卡片 | 消息内摘要 | 可折叠、关闭展开态 | 原消息的“展开” | 统一内容面板、全屏或独立窗口 | 小内容随父滚动；编辑器 / 大表格有明确独立滚动边界 |
| 提问 / 审批 | 有待办提示，详情按需打开 | 详情可以关闭，未决入口不能隐藏 | 固定状态条、等待页、带计数图标 | 表单 / 审批对话框 | 关闭不会提交、授权、取消或恢复；按请求 ID 防过期回答 |
| 设置 | 独立页 | 可离开；有未保存改动时明确处置 | 左下设置、命令面板 | 部分凭据 / 导入编辑采用对话框 | 设置内容一个主滚动区；导航独立 |
| Toast | 只作即时反馈 | 可关闭、超时消失 | 重要错误有持久入口 | 不承载必需决策 | 不抢焦点，不堆叠遮住 Stop / 提交 |

窄模式不能把“新建会话”只剩一个隐藏按钮；原型图标轨仍保留新建入口，生产补充 tooltip、无障碍名称和键盘操作。重要操作若从页头移入菜单，菜单必须有等待计数与状态提示。专注模式保留恢复导航、任务状态和停止操作。

## 弹出层和独立窗口

- Popover：模型选择、附件、会话菜单，锚定发起控件；内容太大升级为抽屉。Esc 只关闭最上层，关闭后焦点回发起点。
- Drawer：会话配置、Inspector、插件详情、资料来源。非破坏性观察不锁住整个应用；窄窗口覆盖阅读区，关闭返回原滚动位置。
- Dialog：提问表单、具体审批、创建、导入审阅、不可逆确认。限制宽度，长内容本体滚动、标题与提交固定；模态时焦点受限，背景不可点击。
- 一个窗口有统一 overlay host、层级和焦点策略，不允许原生窗口弹层与 DOM 弹层互相覆盖。不同技术表面只通过明确事件与参数协作。
- Detach：Inspector / 成果只迁移观察视图，使用同一会话和引用 ID；关闭独立窗口不停止任务，不复制执行 owner。插件不能自行创建不受管的系统窗口。

SmallPen alpha.18 拒绝跨 Presentation 的 `overlay`，而命名式 CLI 的目标是整页。因此包内用 `navigate` 连到独立弹层状态画板，验证打开 / 返回路径；`preview.html` 用同一份 SmallPen 导出图与原生坐标热点叠加展示预期弹层，不另画一套 UI。两者都不证明生产焦点捕获、右侧停靠、拖拽或真实系统独立窗口。停靠与专注状态另画布局状态，真实键盘、滚动和多窗口仍须产品实现验证。

## 关键状态与流程

**提问**：运行 → 请求产生 → 固定等待提示 → 打开表单 → 单选 / 多选 / Other → 校验 → 保存回答 → 保持暂停 → 明确继续原任务。未选不提交；多选需验证数量；Other 单独文本，空白无效；已过期请求不接受回答。问题名称随模型适配，不改变产品的 typed question 契约。

**权限**：工具意图 → 显示来源 / 动作 / 路径或域 / 有效范围 → 允许本次或拒绝本次 → 等待后端确认。先记录后执行，错误不伪装已允许。不能通过回答、插件 UI 或模型输出扩大权限。原型不提供虚假的永久允许按钮。

**动态内容**：消息保留版本化类型与来源引用 → 信任的 renderer 内联摘要 → 用户展开 → 产品加载持久结果 → 与原会话绑定的操作请求 → 后端检查 → 更新结果。不能识别的模块仍有原文与下载入口；加载、空、失败与版本不支持均可恢复，不让整条消息白屏。

**配置**：打开查看 → 编辑草稿 → 展示作用域与差异 → 保存 → 后端确认成功或具体错误 → 标注立即 / 下轮生效。保存插件配置不等于安装、启用或授予权限。

## Go / MyGo 与 HTML 的落地边界

Go 不限制这些布局和交互。关键约束是渲染和资源归属。采用**自绘外壳，HTML 只作为消息流里的一种块**（[ADR 0013](../../adr/0013-module-freeze-and-v1-scope.md) §2）：窗口外壳、导航、会话列表、消息列表、输入区、设置与所有已知类型的内容块都由 MyGo 自绘；只有模型在运行时生成的 HTML 才进入隔离的 WebView，且是消息流里的一块，不是容器。每窗口一个完整 HTML 工作区的方向已被否决：常驻网页进程、原生组件进不了网页文档流、文字流畅度交给了网页。判据是内容由谁写：编译进 Alva 的类型自绘，模型现场写的代码进沙箱。避免每条消息一个 WebView，也不让原生列表持续追踪每条 HTML 卡片高度。

生产栈仍是 ADR-0012，`go.mod` 不变。早先针对“整窗 HTML 工作区”的 MyGo v0.6.0 隔离验证（现仅作 HTML 块 spike 的参考）见 [`../../MYGO-WEBVIEW-VALIDATION.md`](../../MYGO-WEBVIEW-VALIDATION.md)：支持继续验证粗粒度混合；它没有证明完整产品、跨平台输入法或复杂列表滚动已通过验收。

实现依赖方向：Token（可导出 CSS / Go 值）→ Controls → Regions / Surfaces → Domain → Pages。共享值和领域动作，不强迫 DOM 组件与 MyGo view 共用一个渲染 API。不要把 HTML、MyGo、CSS、窗口句柄放进 kernel / agent SDK。

| 责任 | 放在哪里 |
|---|---|
| 布局、滚动、焦点、输入草稿、可见性、主题、弹层 | 窗口工作区；一个视图只由一个渲染系统掌握几何 |
| 会话、引擎分支、消息引用、媒体回执、任务恢复 | 现有 Hub / direct 产品服务；UI 读投影、发送 typed actions |
| 工具、权限、审批、提问与执行 owner | 现有 host / harness / 产品执行服务；关闭观察者不结束执行 |
| UI bridge | 产品适配层；有版本、请求 ID、作用域、超时、取消、错误和背压；不从浏览器暴露任意 Go 调用 |
| 插件动态 HTML | 隔离且受管的内容表面；显式 capability、来源、大小 / CPU / 网络 / 导航限制，默认无宿主特权 |
| 通用 kernel / SDK | 保持已有运行与扩展契约，不知道页签、弹窗、主题或某插件的 UI |

执行插件与 UI 插件是两个表面，可以属于同一包，但启用后端不自动赋予前端权限。现有可信消息模块先继续提供原文降级；MCP UI 元数据、HTML sandbox、资源加载与 UI 工具桥要分别补齐，不能因原型有卡片就宣称支持。

## 照图实现的顺序与验收

1. Token 和共享组件：键盘焦点、禁用、错误、加载、长路径、中文、明暗主题；同时输出 CSS / Go 消费格式的映射规范。
2. 主窗口：先接现有只读服务，完成导航、主滚动、专注 / 窄模式、面板和全局弹层；空会话不发请求。
3. 输入与执行：流式文字、取消、停止、待办 / Other / 审批、原任务继续；验证关闭视图不结束执行、过期响应拒绝。
4. 设置迁移：按表逐组接现有 service，配置来源、检查、保存 / 启用 / 下轮生效分别反馈。
5. 动态内容与多窗口：先可信模块，再插件隔离表面；成果 / Inspector 共用服务和引用，不创建第二个执行 owner。

每阶段检查：连续 resize、DPI / 缩放、中文输入法、键盘 / 屏幕阅读、滚动锚点与回看、长工具结果、粘贴 / 拖放、主题、焦点恢复、桥接取消与背压。macOS 的样本通过不能代表 Windows / Linux，静态 SmallPen 图也不能替代这些运行时测试。

## 旧细化草稿的审阅记录

最终 Product revision：`7c36e47dac955402859eb6de1b1a292351d0be9948b5e6e5a65c08a35fbdbd76`；Foundation：`c61ac6a9556821a41ed3cb9d1b9b380e4d5984f766613836aead9cb2f0e86649`。完整记录与精确目标在 `review.json`。

| 检查 | 实际范围与结果 |
|---|---|
| 设计定义 | 90 个 Token、10 个组件 / 52 个变体、34 个产品页面或状态画板；单一 Foundation / 单一 Product / 同一画布 |
| 产品自动检查 | 34 × 6 = 204 目标：明暗 × 宽窄的舒适布局四组合，另加浅色桌面紧凑、深色窄窗紧凑；完整完成，0 问题 |
| 组件自动检查 | 52 × 4 = 208 目标：浅色桌面、深色桌面、浅色紧凑、深色窄窗紧凑；目标均执行完成，0 报告问题。透明 SettingRow 的标题 / 说明在四组合中共 8 项独立对比度检查跳过，故组件 coverage.complete=false；它们在实际页面父卡片背景下的检查已通过 |
| 导出 | 34 × 4 = 136 套原生 PNG / spec / wireframe / geometry；另有生成的明暗 Design System 与有效 Token JSON |
| 原型连线 | 382 条实际保存的连线全部匹配；创建、Other→保存→继续→停止、审批拒绝、审批允许→停止、测试/评估切页，共 5 条图路径检查 |
| 浏览器走查 | 本地预览 24 条检查通过：图片加载、真实热点、Esc 和焦点恢复、Other、保存后暂停、明确继续 / 停止、独立审批、非模态 Inspector、主题 / 宽度和测试 / 评估；无 JS 异常、无外部请求 |
| 已安装 App | alpha.15 实际打开并显示画板，选中 Conversation 后捕获渲染画布；退出成功，包 revision 未改变。不是逐页 / 逐像素与 CLI 的一致性证明 |
| 字体兼容 | 改导入 WOFF 后，TTF / WOFF 的 glyf/cmap/hmtx/hhea/vmtx/vhea/GSUB/GPOS 全一致，136 / 136 页面位图 renderHash 不变；旧 TTF 打开失败和此前报告保留在 exports/archive/history/ |
| 仓库边界 | `go test -mod=readonly ./tests/...`：276 项、3 个包通过；`git diff --check` 通过；go.mod / go.sum 的既有 SHA-256 未改变 |

人工图像检查已覆盖对话、明暗与窄窗、停靠与窄窗回收、失败与等待提示、设置、工具连接、审批、Other、开发者评估、Design System 总图和浏览器 / App 画布。具体文件列在 `review.json`；没有把全部 136 个页面组合逐个做 App 全尺寸视觉回归。

浏览器 / App 检查可分别通过 `node docs/design/alva/source/browser_review.mjs`、`node docs/design/alva/source/app_review.mjs` 重做，使用本机已安装的 macOS Chrome / SmallPen 和临时配置目录，不改变设计。工具检查不验证真实用户输入、配置落盘、后端执行、插件隔离、拖拽、系统多窗口或连续尺寸 / 跨平台表现。设计阶段不连接模型账户，不消耗推理费用。
