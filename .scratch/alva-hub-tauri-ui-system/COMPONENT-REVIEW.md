# Component Review

## 审阅方式

每个组件同时审阅四件事：它归哪一层、谁拥有尺寸和滚动、需要哪些状态、动效是否真的有目的。页面不得绕过这些结论自行修补。

## L1 Controls

| 组件 | 当前问题 | 目标边界 | 动效决策 | 验收重点 |
|---|---|---|---|---|
| `Button` | 按压反馈过弱，pointer 与 keyboard 未区分 | 统一 size/tone/loading/disabled；页面只传语义 | pointer `scale(.97)` 120ms；键盘只变色 | hover、focus、disabled、loading 不跳宽 |
| `IconButton` | 图标热区和 tooltip 依赖调用方 | 固定方形热区、图标尺寸、可访问名称 | 与 Button 相同，不单独发明动画 | 触屏至少 40px 热区，focus ring 不裁切 |
| `FieldFrame` | label、hint、error 间距尚未形成唯一规范 | 拥有字段垂直节奏和错误槽位 | 只做颜色/opacity 状态 | 错误出现不遮挡输入，不造成横向位移 |
| `TextField` | 搜索与普通输入容易各自改高度 | 用 variant 表达 default/search/compact | focus 仅颜色和 ring | 输入时无尺寸变化，键盘操作无位移 |
| `TextArea` | Composer 与表单输入职责可能混合 | 仅负责基础 textarea；Composer 单独组合 | 高度跟内容直接更新，不逐帧动画 height | 输入、换行、IME、长文本稳定 |
| `SelectField` | 原生选择与模型复杂选择视觉语义容易混淆 | 仅用于短静态枚举；模型选择用 Popover | 箭头旋转可选，菜单 180ms ease-out | disabled 原因可读，宽度不由页面逐个调 |
| `Checkbox` | label、description、disabled 的层级需统一 | 控件与文字点击区由组件拥有 | check opacity/scale 120ms | indeterminate、disabled、focus 状态完整 |
| `Switch` | track/thumb 状态可用，但需统一速度 | 只表达即时布尔设置 | thumb translateX + track color 160ms | reduced motion 仍看得出开关状态 |
| `Tabs` | 高频点击存在 active scale 风险 | 拥有 tab list、active、focus、overflow | 只变颜色/背景，不移动、不缩放 | 键盘方向键、窄宽横向处理一致 |
| `Badge` | 容易被业务拿来承载过长解释 | 只承载短状态词；原因放邻近正文/tooltip | 仅颜色；状态切换可短 opacity | 文案不截断核心状态，tone 有语义 |

## L1 Overlays

| 组件 | 当前问题 | 目标边界 | 动效决策 | 验收重点 |
|---|---|---|---|---|
| `Tooltip` | 连续浏览是否即时主要依赖 Provider | 首次延迟、连续跳转、碰撞和 portal 统一 | 首次 opacity + scale 140ms；连续即时 | 不遮住 trigger，触屏不产生 sticky hover |
| `Menu` | 菜单宽度、item 密度可能由业务覆盖 | compact/default/wide 三种宽度；统一 item | trigger-origin 180ms ease-out | 键盘循环、submenu、危险项、关闭路径 |
| `Popover` | keyframes 难反向，退出曾使用 ease-in | 拥有 portal、碰撞、origin 和 size variant | 可中断 transition，进入/退出均 ease-out | 快速开关不闪回，靠近边缘不越界 |
| `Dialog` | 各业务弹窗宽度和内容结构不统一 | compact/default/wide/full-height；固定 header/body/footer | center-origin 220ms ease-out | Allow/Deny、自动化、设置弹窗同一宽度体系 |
| `Disclosure` | 动画 height 触发布局且难反向 | 拥有 trigger、chevron、content lifecycle | chevron 旋转；内容使用可中断展开 | 连续点击不跳，焦点不丢失 |

## L2 Regions and Surfaces

| 组件 | 当前问题 | 目标边界 | 动效决策 | 验收重点 |
|---|---|---|---|---|
| `AppFrame` | 页面曾以局部宽度修补整体关系 | 唯一拥有 Sidebar/Main/Workbench 三栏与断点 | 只在 Workbench 模式切换时改变空间关系 | 窗口缩放时整体同步，不逐卡片调整 |
| `Sidebar` | item 层级、hover 背景与密度曾不一致 | 拥有导航区域宽度和内部分区，不拥有业务行细节 | 高频导航只变背景/文字色 | hover 覆盖整行，selected 与 hover 可区分 |
| `Workbench` | 右侧面板停靠、覆盖、宽度需统一 | 拥有 resize、dock/overlay、tab 生命周期 | translateX 240ms drawer curve | Main 同步收缩；窄屏切 Sheet；内容不中断 |
| `PaneHeader` | 标题和 action 对齐可能被页面改写 | 统一标题、副标题、动作区高度 | 无进入动画，仅状态色 | 所有页面标题轴线一致 |
| `ContentFrame` | Timeline、Notice、Composer 曾可能不同轴 | 唯一拥有主内容 max-width 和 gutter | 不动画 | 子内容左右边界完全一致 |
| `ScrollRegion` | 双滚动风险 | 当前区域唯一滚动拥有者 | 不动画滚动内容 | 长会话、设置、Workbench 均只有一个滚动条 |
| `DockRegion` | Composer/审批与正文归属容易混乱 | 拥有底部常驻区和安全间距 | Notice 进入可轻微 opacity/translate | 跟随窗口缩放且不覆盖正文 |
| `WorkbenchPage` | tab 内容可能自建第二滚动层 | 拥有 tab 页 padding 与唯一滚动 | tab 切换不做位移 | 切 tab 保留各自状态与滚动位置 |
| `Surface` | 页面仍可能复制边框/圆角/背景 | 提供 plain/raised/inset/interactive 语义 | interactive 仅颜色/阴影 | 各页面卡片视觉来自同一 variant |
| `PanelSection` | header/body/action 间距可能分叉 | 拥有 section 标题、说明、meta、action | Disclosure 场景才展开动画 | Settings、Details、Team 结构一致 |
| `Notice` | approval/fallback/error 宽度曾各自调整 | 统一 tone、title/body/actions/details | 新到达 180ms；已有状态只变色 | Allow/Deny 等动作位置固定，详情渐进披露 |
| `StatusListItem` | 可用/不可用原因展示不统一 | 统一 icon、main、meta、status、action 槽位 | hover 只变背景 | disabled 项仍可理解原因但不可误选 |
| `EmptyState` | 页面可能各写图标、文案、按钮间距 | 统一紧凑/页面级两种密度 | 无必要进入动画 | 空状态不抢主操作注意力 |

## L3 Domain Components

| 组件 | 归属区域 | 应组合的积木 | 禁止自行拥有 |
|---|---|---|---|
| `HubSidebar` | `Sidebar` | NavGroup、NavItem、TextField、Menu | 全局断点、基础 hover、菜单动画 |
| `HubTopbar` | `MainPane` | PaneHeader、Tabs、IconButton | 主内容宽度、Workbench 动画 |
| `HubTimeline` | `ScrollRegion` | Surface、Disclosure、Badge、Notice | 页面滚动、Composer 停靠 |
| `HubActivityRow` | `HubTimeline` | Badge、Disclosure、Menu | 时间线 max-width、基础状态色 |
| `HubComposer` | `DockRegion` | TextArea、IconButton、Popover、Notice | Dock 定位、弹层碰撞和宽度常量 |
| `ModelPicker` | `Popover` | Tabs、TextField、StatusListItem、Disclosure | portal、整体弹层动画、任意像素定位 |
| `SessionDetails` | `ScrollRegion` 或 Workbench tab | PanelSection、Disclosure、StatusListItem | 第二滚动层、独立卡片体系 |
| `EngineReadinessPanel` | Settings/Workbench | PanelSection、StatusListItem、Button | 新状态组件、页面宽度 |
| `FallbackPolicyPanel` | Settings/Workbench | PanelSection、StatusListItem、SelectField | 自定义按钮/字段样式 |
| `TeamPanel` | Workbench Agent tab | PanelSection、StatusListItem、Disclosure | Workbench 生命周期和滚动 |
| `AutomationForm` | Dialog/WorkbenchPage | FieldFrame、TextField、SelectField、Switch | Dialog 尺寸和 footer 布局 |
| `ExtensionCard` | CardGrid | Surface、Badge、Button、Menu | 独立 hover 曲线和卡片间距 |

## 审阅结论

1. 当前问题主要不是 React、Tauri 或 Web 能力不足，而是区域所有权、组件尺寸和动效词汇没有被同一套契约约束。
2. 先修 L0–L2，再拼 L3–L4；否则每个页面仍会靠局部宽高补丁对齐。
3. 动效不是越多越好：Sidebar、Tabs、搜索和列表选择属于高频路径，应尽量无位移；Popover、Dialog、Workbench 才需要明确空间关系。
4. Cindy 线稿用于校验层级、位置和交互路径，不要求复制其技术栈或每一项视觉参数。
