# SmallPen CLI 使用与踩坑指南

Avoid unnecessary secondary labels and descriptions. Let the layout do the explaining

当前密度使用[一套正常值](./DESIGN.md#当前一套正常信息密度--2026-11-06)：body13/caption11、普通控件32、图标动作28/图标16、导航行28；原生工具行36独立于表单控件，不能随control.height一起缩小。保持上一轮左移/右固定，旧32动作和36列表的截图不再作为尺寸正本；不新增三档或逐页面Token。

2026-11-05最新开合定义见[当前三栏与开合定义](./DESIGN.md#当前三栏圆角独立开合与原生过渡--2026-11-05)。用户已纠正运动关系：左开关展开在左栏右上，收起才移到红绿灯右边；右开关每帧固定于窗口右上，不能放在动画父容器内随框移动。More迁入右栏，分栏资源头收敛为36；最低宽度资源专注仍由窗口壳保留唯一同位收起动作。macOS的TitleBarHeight不配置红绿灯，须核对实际TrafficLightPosition和更新后的安全区；不能把默认标题栏总高度或静态样例当成按钮中心的实测。下方早先窗口壳说明按新规则修订，不从旧页面覆盖恢复已隐藏的More。

窗口壳最新增量见 [原生内嵌标题栏定义](./DESIGN.md#已保存的原生内嵌标题栏与导航壳--2026-11-05)：先读正式 PageHeader 的 Navigation 两态和原生 `req_alva_navigation_native_shell`，不从旧32自绘空条、72窄侧栏或字体 seed 高度推断现行布局。原生 Left/Right/Height 是运行时事实；设计的52/86/0只是样例。需要文字自适应宽度时，用 TEXT 的 `growType=auto-width` 与 fixed sizing；直接把 TEXT 写成 layout-item-h-sizing=auto 会报 layout_not_in_app。实例/Frame 的 hug 与 TEXT 的自动宽度表达不同。

给接手 Alva 设计的 AI：先确定组件和布局规则，再让 CLI 执行。用户要的是极简、克制、有少量细节的 Agent 产品；“成熟”指真实场景下行为正确、可复用，不指颜色、尺寸、组件或变体数量多。

边界与圆角沿用 [已确认的正式视觉基准](./DESIGN.md#当前正式视觉基准--2026-11-04)。正式页的最新增量入口是 `source/boundary_refresh.py`，先读保存包及实例覆盖，不从旧图或旧生成器恢复已撤去的分隔线。Typed 覆盖路径须读取真实选中变体的节点 ID；不要从显示名拼接 ID，深层控件名称不一定等于显示路径。

先问每段辅助文案是否影响选择：来源、作用域、生效时机、权限边界、错误和禁用原因需要保留；重复标题、显而易见的操作教学和同一事实的第二次展示应收起。删除说明后重查容器的 hug / fill 与对齐；例如隐藏弹窗左侧说明后，取消和保存仍须靠右。先用布局分组，再决定是否需要说明或边框。

当前入口已按 2026-10-26 的 `0.1.0-alpha.21` 与 bundled `smallpen-ui-design` Skill 整理；后文 alpha.18/19/20 的问题与回执保留其历史适用范围。工具行为以接手时安装版本的 `help` / `schema` 为准；当前保存的设计与验收记录见 [DESIGN.md](./DESIGN.md) 和 [review.json](./review.json)。使用流程更新不表示旧组件建模、生产实现或平台验收已经完成。

## 先加载 Skill，再选择交付范围

先执行 `smallpen help skills --json`，读取它返回且 `available=true` 的 `smallpen-ui-design/SKILL.md`。安装目录随 CLI 版本变化，不在项目中复制一份 Skill，也不把某次安装路径写死为永久入口。读过本文不等于已加载工具 Skill；Skill 管操作流程，项目规范管视觉与业务决定，实际命令参数仍查当前 help/schema。

默认沿用 Foundation 的 Token、组件和必要变体，业务差异用 SPEC 说明。先判断本次请求是哪一种：

| 请求 | 保存与交付 |
|---|---|
| 维护基础规范或组件 | 增量修改已有定义，检查受影响消费者；不自动重画所有页面 |
| 用组件展示业务内容、比较方案或准备 Review | 在包外保存组合 intent，用 `page draw --preview` 渲染；样例不写入正式设计包 |
| 维护明确要求的正式页面或可点击原型 | 复用组件装配必要页面/连线，保存并验证这次实际涉及的范围 |

规范职责与位置以 [DESIGN 的现行工作方式](./DESIGN.md#现行工作方式基础规范spec与组合预览--2026-10-26) 为准。现有业务 SPEC 在 Product 的原生 requirements 中，继续按稳定 ID 维护；不因为工具新增 Skill 再建一份相同正文。CLI 导出的 visual specification 描述设计约束、来源和几何，不能代替功能 SPEC。

## 组合预览：复用定义，不保存样例页面

先读相关组件的属性、默认选择、Token 绑定与允许的实例覆盖，再用普通页面容器组合现有组件和业务样例。布局意图保留 `hug` / `fill`；共享固定控件高度等约束需要在实例上按实际 schema 明确携带，不能只抄源组件的测量高度。当前 Instance 没有任意内容槽；业务正文放在组合容器中，不向 Instance 强塞子树。

例如 `Alva Button` 的 `Content=Icon` 新实例应同时设置 `width` / `height` 为 `{ui.control.height}`；仅设高度时，copy 默认 hug 可能变成16×36。普通文字按钮继续 hug，需要拉伸的由父容器设置 fill。共享加载态的结构和运行责任见 [Button 约定](./COMPONENTS.md#button-加载态与复用)，不要用逐文案固定宽度修补切状态后的跳动。

```sh
smallpen help page draw --json
smallpen schema page-draw --json
smallpen page draw PACKAGE --intent PREVIEW_INTENT.json --preview --json
```

`--preview` 原生渲染候选，返回 PNG、specification、wireframe、evidence 与受 revision 保护的待提交批次。读取回执里的实际路径，不猜输出文件名；临时结果会在约 30 分钟后被清理，需留存的 intent/产物放在本轮 `exports/` 中并记录出处。主题、context 和 scale 只影响本次预览，不保存选择。只需要 Review 时不 apply；本次也要求维护正式稿时，使用回执的 `preview.apply.argv` 提交同一候选。revision 已变化就先读 changes、重新预览，不换 batch ID 绕过拒绝。

每次 Review 关联：功能 requirement ID/正本位置、组件 ID/变体、实例内容与覆盖、两包源 revision、显式主题/context、约束与解析值、同次布局的 `layout.hash`、实际查看的图片及未覆盖项。组件文字相同或数值相同不证明绑定相同；确认来源需要读 spec。标准设计渲染与各端实际运行结果分别采集、分别标记，不能把静态预览写成业务通过。

跨平台沿用同一功能含义、组件角色和布局意图。值差异优先用现有主题组合，局部结构差异用必要变体，整体结构确有差异再建 Presentation 并说明原因。平台的字体、单位、触控和系统行为可以明确适配，不要求不同运行端逐像素相同，也不自动生成全套平台页面。本流程可供 Go/MyGo、Electron、Android 或 Apple 原生等实现引用；Alva 当前生产栈仍由根 README/ADR 决定。

## 开始画交互前，先读取行为

CLI 已有原生 requirement / annotation，但普通帮助和默认概览没有把它们带进画图流程。不能因为默认输出没提到，就当作没有行为说明。先用 `smallpen project list PACKAGE --kind requirements --full --json` 读功能规则；需要节点标注时用 `smallpen project show PACKAGE --full --json`，立即消费返回的 `resultFile`。先按目标节点、页面和 requirement ID 缩小范围，不把全部项目数据塞进上下文。

交互控件的触发、动作、退出、异常和未定项，与布局一起维护；缺信息就明确标注，不替用户猜。写入复用公开 `put-requirement-file`，具体约束见下文“功能如何跟 UI 同步”。本文的提醒是当前项目的防漏措施。CLI 本身仍应改进入口引导、按控件关联读取、局部写入和行为覆盖检查，不能靠下一位 AI 更细心；当前版本复现与可验收改进建议见 [发现性审查](./exports/archive/cli-behavior-discovery-2026-10-23.json)。这些 CLI 改进尚未实现。

子页返回放在所属页面或弹窗左上，使用箭头与明确的目标文字；不要把退出入口跟在 fill Spacer 后推到左下。先区分“返回工作台”“返回向导上一步”和“取消/关闭”，检查它们各自的目标与草稿规则；不把取消当后退，也不把静态跳转当真实状态保持。当前契约在原生 `requirements/navigation.json`，共用设置返回在 SettingsNav，向导复用 Button。

连接流程先读 `requirements/providers.json`：ProviderForm只区分Preset/Custom结构，供应商、完整端点和创建/编辑模式属于实例数据。目录的Idle/Loading/Ready/Empty/Unavailable/Error由独立ProviderModels表达，不与来源做全排列；获取目录不是保存前置步骤。折叠只改变现有组的显隐，须另查展开后的实际高度，不为每个供应商复制页面或浮层。

偏好设置先读 `requirements/preferences.json`：SettingRow用Switch/Action/Select表达右侧控件，当前值按钮随内容收缩，复用Ghost Button和SelectOption；填写名称、地址、密钥才用Field/Input。不要把设置偏好铺成满宽输入框，也不要从编辑器对象选中轮廓推断组件的Focus颜色；读tokenBindings并用原生焦点样本核对。

## 先避免这次最主要的错误

这次把“整理成熟的按钮”理解成了“把尺寸、内容形式、样式、状态全部列举”，先展开大量组合，再批量铺到页面。用户指出复杂后，又主要靠删 Token、删尺寸档位解决，仍没有先纠正属性的归属。

`component define` 会把 `properties` 的取值做全排列。Button 曾经有 `3 个尺寸 × 4 种样式 × 4 种内容 × 7 种状态 = 336` 个变体；取消尺寸轴后仍有 112 个。全排列是该命令的行为，选择这么多轴是作者的决定。不能据此归咎于 CLI 不支持自适应，也不能把 336 降到 112 当作设计已经合理。

新增一个维度之前，先回答：它改变共享样式、交互状态、实例内容，还是父容器的布局？放在对应位置，不要全部塞进变体。

## 把属性放对地方

| 内容 | 应该怎么做 | 不要怎么做 |
|---|---|---|
| 颜色 | 复用表面、文字、边界、强调和必要状态等语义色；明暗主题用 Token 值切换 | 给每个组件、每个状态单独造颜色；把主题再乘进组件轴 |
| 尺寸 | 维护有限的控件高度、图标框、间距、圆角标尺；其余交给布局 | 为每个按钮文案、卡片、页面宽度新建尺寸 Token |
| 业务文案 | 覆盖实例文字 | 为“保存”“取消”“创建任务”分别定义组件或变体 |
| 图标 | 复用图标组件，用名称选择；可选图标优先使用实例配置或可见性覆盖 | 每换一个图标就复制按钮，或默认把所有图标位置乘到所有状态 |
| 样式与状态 | 用原生变体表达真实需要的主次样式、hover、focus、disabled、loading、selected 等 | 用复制页面代替变体；把彼此冲突的状态做成任意组合 |
| 宽度 | 默认随内容；需要占满时由实例及父容器设置 `fill` | 为 fixed / hug / fill 再造一个变体轴，或给每个按钮写死宽度 |
| 内容容器 | 页面布局容器装配业务内容，内部复用组件 | 为追求“全部组件化”向不支持内容槽的 Instance 强塞任意子树 |
| 动效 | 明确什么状态开始、结束，实际运行层实现；Loader 和业务图标独立 | 用更多静态画板冒充旋转、展开或真实加载行为 |

浮层也按这个原则复用：容器和 Tooltip 的外观、布局与开关规则维护一次；使用位置只声明触发点、内容/组件引用、当前选择值和必要的关闭条件。提示语、不同按钮 hover、权限说明文字的变化不新建画板。复杂菜单、选择器或表单的独立业务内容需要一次代表设计，不在每个调用页面重新铺一套。21/22 的重复浮层已撤下；保留 Foundation 的共用组件以及 41 添加菜单代表稿。

当前 CLI 的 Instance 没有任意内容槽。通用 Popover 容器在作者层用 `design_system.popover()` 共用已有 Token 和布局配方，业务内容复用 PermissionMenu/ComposerMenu；Tooltip 直接使用同一个 Alva Tooltip 实例配置。不要为凑一个空壳组件名称强塞子树，也不要为了让静态原型每处都可点而克隆按钮、提示或整页。未来产品组件接受内容与触发配置，文字备注引用这些定义，不替代需要设计的业务内容。

是否带图标确实影响结构或编辑体验时，可以保留必要的内容变体；先证明需要，再算组合数。用户要求“用变体”不等于要求每个属性都是变体轴。

实例覆盖也需要检查状态组合：更换图标或修改显隐后，重查 Loading / Disabled / Focus，不能让内容配置破坏状态。若条件显隐无法可靠表达，就保留必要的内容变体；不要为减少数量把组件正确性删掉。

Text Loading 的透明 Label 用来保留 intrinsic 占位；Loader 使用公开 node schema 的 `layout-item-absolute` 与双轴 center constraints，作者收尾用受 revision 保护的 `put-component-set`，不直接改保存 JSON。不要逐实例写 x/y 或把 Label 改成 `visible=false`。原生检查可能把这一对透明文字／Loader 报为 informational overlap，须按具体节点、透明度与 PNG 逐项确认；这不构成忽略其他重叠的理由。测量文字框恰好等于某个 Token 数值，也不应将它绑定为无关的控件或页头尺寸。

Token 也不是越少越好。同色但不同语义的角色可以共享基础值或别名；同为数值 20 的字号和图标尺寸仍属于不同类型。`design_token_value_unmatched` / `design_token_not_used` 是审阅信号，不是“看到一个字面值就必须新增一个 Token”的命令。先检查是否应使用 `hug` / `fill`、复用已有值，或保留有明确用途的局部数值。

当前正式体系采用六档间距（4 / 8 / 12 / 16 / 24 / 32）与少量区块组合，具体表格见 DESIGN 的“当前规则”。`design_system.block()` 组合共享标尺，不产生每组件专用 Token。新增 Token 必须解释跨区块职责、现有规则为何不能满足、实际消费者与迁移方式；不可为了每次绘制或局部微调扩张目录。组件目录的示例宽度、图标几何和预览窗口尺寸也不能冒充运行布局标准。

合并 Token 后必须迁移实际 `tokenBindings`，包括嵌套实例覆盖；仅把显示数值改成相同值没有完成收敛。合并颜色绑定时应保留已有字体、尺寸绑定，不能用源组件的整份绑定覆盖实例的小图标字号。把一个值从主题组移到默认组时，还要清理原来的同名主题覆盖并迁移其 ID，否则旧覆盖仍会优先解析。

高层 intent 的文字 `hug` 不等于原生 TEXT 节点的 `layout-item-v-sizing=auto`。本轮 alpha.21 实查：换行正文应保留 `growType=auto-height` 与纵向 `fix`；原生 `auto` 会报 `layout_not_in_app`。同样，`maxWidth` 只用于横向 `fill` 的节点，不能批量套到随内容收缩的 Badge。先核对 node schema 与实际导出诊断，再改原生字段；隐藏的空容器也须保持合法 sizing，不能只看 PNG 没移动就接受。

## 尺寸先选行为，再选数值

SmallPen CLI 已支持 `hug`、`fill`、`align: stretch` 和最小／最大尺寸。不要因为输入是 JSON 就把它当成只能画固定坐标的工具。

| 场景 | 默认布局规则 |
|---|---|
| 普通文字按钮 | 宽度 `hug`，统一控件高度，水平内边距；只在业务确实需要时 `fill` |
| 纯图标按钮 | 正方形点击区域，图标居中；复用现有 IconButton |
| 输入框与表单行 | 宽度 `fill`；标签、说明、错误提示自然增高 |
| 对话正文与内容卡片 | 宽度 `fill`，必要时限制最大阅读宽度；高度 `hug` |
| 主工作区 | 父容器 row / column 分配空间，主区域 `fill`，侧栏承担自己的宽度约束 |
| 弹层与右侧面板 | 根据内容和宿主空间设边界；允许合理的固定基准，不能把截图宽度当全部窗口的规则 |

目前 Alva 的桌面基准是普通动作高 36、工具栏图标动作 32。这是项目选择，不是 CLI 或所有产品必须遵守的标准；也不意味着长内容、输入区和容器都要固定高度。

2026-10-23工作台紧凑修订统一导航220/行36/条目4/分组12、页头52、外边距24；首页限宽760、标题到输入16、建议12。后续输入区对齐修订将Composer对称内边距收至8/间隔8，工具栏控件32、Send/Stop为32圆形/圆角16/图标18；正文仍16/1.75、至少两行56。Permission/Model的运行标签padding为对称上下4/左右12；SmallPen嵌套Instance不支持局部layout-padding覆盖，静态样本使用共享Button的居中布局与左右12，不能把字段表达当作逐像素一致或用宽度补偿冒充。不要通过缩字、负间距或新增Token模拟紧凑；当前增量入口是`source/density_refresh.py`，`composer_states.align_geometry`维护局部几何，原作者定义已同步。先读`review.json.currentComposerAlignmentReview`和前轮`currentDensityReview`的真实revision和范围，再决定是否重新应用。

实际 API 约束：

- `fill` 需要 row / column 父容器；先检查父子两层，不能只改子节点。
- `hug` 用于 row / column 容器、相应组件实例或文字。文字宽度 `hug` 时高度也随内容。
- `minWidth` / `maxWidth` / `minHeight` / `maxHeight` 只可约束设为 `fill` 的对应轴；其他组合会报 `invalid_size_limit`。
- 文本默认 `hug`，但有固定宽度的 row 中唯一的文字会填充该行；需要固定语义时显式声明。
- `align: stretch` 是容器对齐规则，配合父容器和子节点尺寸检查效果，不是给每个节点设置一个固定宽度。
- Desktop / Narrow 两种配置只证明两个采样，不证明连续缩放、长文案和任意窗口尺寸已经成立。
- 原生模型可存储的布局能力与 CLI 导出投影的覆盖范围不同。当前 `schema node` 提醒 auto / min-max / stretch 等可能属于部分投影；遇到 `layout_projection_partial` 或 skipped reflow 时，要用实际 Web / App 核对，不能仅凭“字段可写”宣称动态布局验收完成。

## 文字对齐：先约定对象，再测量

先审 CLI 设计自身，不先用平台差异解释偏移。居中控件的上下 padding 及绑定必须相同；统一文字样式提供行框，图标资产提供标准画布，组件通过 center 对齐。禁止用上下不同的 padding、负 margin、逐标签 y 偏移硬凑效果。外框正确但内容一致偏移时，问题回到共享字体样式／渲染器或图标资产内部查；平台适配在设计基准成立后另行验证。

容器居中、文本水平居中、基线对齐与笔画的视觉中心是不同的事。按钮的 row 居中只定位子节点的框，不能替代固定文字槽里的 `textAlign=center`；文字节点高度为 hug、没有额外空间时，加 `verticalAlign=center` 也不会改变位置。不要看到墨迹略偏下就把每段文字 y 减 1，或把所有导航标签都改成水平居中。

### 两个方向必须显式表达，并与渲染核对

| 目标 | row 容器的 CLI intent | column 容器的 CLI intent |
|---|---|---|
| 内容组水平居中 | `justify: center` | `align: center` |
| 内容组垂直居中 | `align: center` | `justify: center` |
| 文字在文字框内水平居中 | 文字节点 `textAlign: center` | 文字节点 `textAlign: center` |

Button、Tag／Badge、ToggleButton、Segment、IconButton 和 Step 的数字容器需要双轴居中。Navigation／Choice／Step 的主标签沿阅读方向排列，整行只要求垂直居中，不能为了统一字段把整行都改成水平居中。不要把一种布局方向下的 align 直接解释为永远垂直；方向改变时主轴和交叉轴随之改变。

2026-10-22 复核发现 StatusBadge／Alva Badge／Alva Segment 原来只写 align，横向依赖 hug 时没有剩余空间；Alva Button 的 Label 也漏写了 textAlign。现已补全这些共享定义。CLI 能表达双轴居中，之前的缺陷是作者没有完整声明与验证；只看原尺寸样本和“上下 padding 相同”不足以证明拉伸后仍正确。

验收至少包含原尺寸以及隔离副本中的拉宽／增高样本。用 `export --format png --evidence` 从同一次 native 布局得到 PNG、spec、wireframe 与 JSON；spec 中的 row/column、justify、align、padding 必须与意图一致，解析后内容组中心应等于内容区中心。渲染检查还需确认图标／文字的框在同一中心线上。拉伸探针的尺寸只属于验收，不写回组件，不新增尺寸 Token 或变体。Web 重新载入同一保存包后，再区分“重新加载成功”和“浏览器画面已实测”两种结论。

约定优先级是：内容区 padding → 子组的布局与 hug/fill → 文本行高、基线与水平对齐 → 实际字体测量 → 渲染验收。正式规则见 [DESIGN](./DESIGN.md#文字对齐契约--2026-10-22)。字体角色、字重、字号、行高、换行／截断、阅读方向、图标槽和缩放规则属于可移植契约；具体 x/y、行宽及取整值属于某次渲染结果。文字宽度应按 shaping 后的占用宽度布局，不能按可见笔画的包围盒计算 padding。非对称文字没有“所有文案的墨迹中心都必须为 0”这一验收要求。

安装版 alpha.18 的高层作者命令会先规划，再调用 native `measureProjectionText`，然后按真实测量值重新规划，并对保存的尺寸向上取整；fill 文本会在分配宽度下重新测量。不能说 CLI 只能猜字宽。公开 spec 可检查框与样式，但本次导出没有提供精确 baseline、ascent/descent 和逐字回退字体证据，跨平台一致性仍需要平台测试。`schema node` 的布局 align-items 目前列出 start/end/center/stretch，没有 baseline；原生 textStyle 有 verticalAlign，但高层 component/page intent 并未同样列出它。不要猜字段或用像素偏移伪装成工具尚未表达的 baseline 语义。

| 实现层 | 同一约定如何落地 |
|---|---|
| Go＋HTML | Go 提供内容与状态；浏览器按字体和 CSS 排版。用 flex/grid 对齐内容框、显式 line-height 定义行框，text-align 管文字在框内的位置；混合字号需要 baseline 对齐。不要把 CLI 测得的标签坐标硬编码进 HTML |
| Android | 由平台文本组件测量；Compose 在组件层统一 includeFontPadding 与 LineHeightStyle 的 Alignment／Trim 策略，不能逐个控件随意切换。TextAlign 控制文字框内部，父布局 alignment 控制整个框的位置；Start/End 随阅读方向变化。参见 [Android 段落排版文档](https://developer.android.com/develop/ui/compose/text/style-paragraph) |
| iOS | 由 UIKit／SwiftUI 使用实际字体与内容尺寸排版；整体使用容器对齐，同一行文字使用 first／last baseline。保留字体缩放后的 intrinsic size 和换行能力，检查 hugging／compression 约束，不能强拉文字框后仍假定基线正确。参见 [Apple 内容尺寸与基线约束](https://developer.apple.com/library/archive/documentation/UserExperience/Conceptual/AutolayoutPG/ViewswithIntrinsicContentSize.html) |

行框由字体度量与行高构成，并非当前字符串的笔画外接框；相关概念见 [CSS Inline Layout](https://www.w3.org/TR/css-inline-3/)。同样的字体文件能减少差异，但 shaping、回退字体、栅格化与像素取整仍可能不同。目标是关系一致、基线稳定、不裁字、缩放可用，不承诺各端像素完全相同。必要的视觉修正必须在字体／图标角色层统一验证，不增加每字符串／每状态偏移。

本轮只补全 8 个组件的水平居中语义；普通文字的字体度量实验只存在临时副本，没有写入正式资产。NavigationItem 的 ○ 与 ChoiceOption 的 ● 是普通字体字符，之前的图标字体修复不会影响它们；这次在固定 20 槽内显式居中解决了遗漏，不需要再改字形或新增 Token。

后续输入区复查（2026-10-23）纠正了这项历史结论的范围：框居中仍可能出现一致的图文墨迹偏差。两处首页标签比图标低约 1.2–1.4px。本轮让**正式 UI 字体角色**使用原 Noto 字体已有的 OS/2 typographic 度量（880 / −120 / 0），hhea 与其一致并启用 USE_TYPO_METRICS；使用单独命名的 Alva UI Sans SC，保留历史 Prototype 字体及原始线稿资源。不是按当前文案调一个 y 值。字形、字宽、Windows 裁切范围和 Lucide 图标保持不变；原生样本偏差缩小到约 0.3px，跨平台仍需实测。依据见 [OpenType 度量建议](https://learn.microsoft.com/en-us/typography/opentype/spec/recom) 与 `fonts/UI-NOTICE.json`。

## 什么时候需要脚本

单个组件或页面的小改动直接调用公共 CLI。跨页面的 Token 合并、嵌套引用迁移、身份与导航保留，可以用项目级脚本生成 intent 并检查结果；脚本的写入终点仍是 CLI，不绕过它改包。当前批量收敛入口是 `rules_refresh.py`，共享规则在 `design_system.py`，页面负责内容装配。临时定位／验收脚本放临时目录，不为每个页面新增维护程序，也不把这些离线作者工具接入 Go 产品运行依赖。

## 先做一个真实切片，再扩展

1. **读取现状。** 看目录 AGENTS、DESIGN、两个包的 revision 和已有组件。区分原始线稿、正式界面、旧草稿；不要把它们重新生成一遍。
2. **写下少量规则。** 明确颜色角色、控件基准、hug / fill 分工、需要的状态。每个新增 Token、组件、轴都应有当前消费者或明确需求。
3. **先做一个组件和一个真实页面。** 优先 Button、输入区及新任务／对话页面，确认视觉层级和复用方式后才推广。
4. **先查组合数。** 每增加一个轴都算乘积，检查是否在把内容或布局重复编码。只生成适用状态；不可用组合不要靠继续加规则掩盖。
5. **用异常内容检查布局。** 中文、较长文案、有无图标、加载、键盘焦点、窄窗口、暗色主题；加载时运行实现须保持动作名称、布局稳定和焦点，避免重复触发。
6. **只推广已验证的定义。** 页面引用共享组件；使用原生生成的 Design System，不另外制作大量展示页、状态页或另一套 HTML 预览来掩盖源设计问题。

极简不等于线稿：通过字号、留白、表面和少量强调建立层次，不给每个区域都套描边卡片。“稍微精致”也不意味着渐变、常驻动画和每项阴影。先看实际页面的整体感受，不能只看组件目录。

当前常用组件清单与组合边界见 [COMPONENTS.md](./COMPONENTS.md)。图标统一使用固定版本的 Lucide；先查 `fonts/icons.json` 的语义映射，新增符号也从同一套官方资源选择，不能手画近似图或混入另一套风格。保留 24 单位画布、16／20 显示槽及颜色绑定；SmallPen 的字体子集只是格式适配，Go／HTML 可直接用同源 SVG。不要把字体包装名称误认为图形仍是先前的原创符号，来源和许可以当前 catalog 为准。

## 写包之前先读协议

以下命令可从仓库根目录用于只读定位；没有跨命令的“当前选中页面”，每次都明确包和目标。

```sh
smallpen version --json
smallpen project show docs/design/alva/alva-foundation.smallpen --json
smallpen project show docs/design/alva/alva.smallpen --json
smallpen schema component-define --json
smallpen schema component-set --json
smallpen help view --json
smallpen help validate --json
```

遇到某个操作时，再读 `smallpen schema operation <type> --json`；命令参数用 `smallpen schema command <command> --json`。先小范围文本／spec 读取，按返回的分页或 `resultFile` 获取需要的部分；重要导出用 `--output` 持久保存，临时结果文件不是长期证据。

Foundation 持有共享 Tokens、字体／图标和组件，Product 持有页面装配与导航。两个 `.smallpen` 目录是不同职责的包，不是两份同名 UI。一个 Web file 中可以有多张画布和配置，不能只凭 URL 不同或画面不同就断言产生了重复文件；检查实际 package、file、page / configuration 映射。

### 保留用户已有编辑

- 可读取 canonical JSON 检查真实身份和引用；**所有写入，包括临时设计包，都通过公共 CLI 或 typed operations**。不要直接改 `.smallpen` 内 JSON，也不要用全量旧生成结果覆盖 App 中的新编辑。
- 写前记录两个包的 revision；编译完成后、正式写入前再次核对。有变化就用 `changes --since` 查清并重新合并，不只换一个 revision 继续强行写。
- `baseRevision` 是并发保护，`batchId` 是同一批操作的重试身份。超时先查回执和当前状态。遇到 `batch_superseded`，读取现状后构造必要的新修改，不重发旧批次或单纯换 ID 绕过拒绝。
- 小改动只修改受影响定义。需要隔离编译时，连同相邻依赖、字体和资源复制，并保留仍被引用的旧组件。曾经清空旧定义导致临时包无法正常继续处理，不能把删除重建当成通用编译步骤。
- 修改轴或删除变体时，同步准备 Foundation 内的嵌套引用和 Product 实例迁移；保留 node ID、文案覆盖、图标选择及交互。`stale_instance_variant` 不能靠重画整页绕过。
- 页面重绘可能丢失原型导航。合并 presentation 的 interactions / prototypeFlows、节点 interactions 和嵌套实例交互覆盖；核对目标与身份，不能只看连线总数。
- 删除 Token 先用 `token impact` 查消费者，再迁移绑定。不要为了“精简”破坏保留线稿和旧草稿。
- 单个 `advanced apply` 批次的原子性不覆盖两个包。先准备完整的组件和实例迁移；任一包失败后核对双方状态再继续，不能报告两包已经一起成功。

现有 [controls_refresh.py](./source/controls_refresh.py) 和 [quiet_refresh.py](./source/quiet_refresh.py) 展示了隔离编译、revision 守卫和引用／导航保留。它们是有特定适用范围的维护脚本，不是每次工作都应该运行的重置命令。

### 已实际踩过的工具坑

| 现象 | 原因及处理 |
|---|---|
| 原生 PNG 正确，Web 浮层的 Tooltip 无 padding、单行 item 留双行高度、控件重叠 | alpha.18 Web 的 `projectScreen` 只对主根调用旧 reflow；隐藏 Description 不会重新计算 hug。先对照**实际 Web workspace 投影**与 native 结果，不能靠移动单个 label、改不等 padding 或重新导出孤立 PNG 宣称修复。当前服务的进程级修正见下文 |
| 页面里的菜单正常，组件目录仍然偏；空状态总览还有大片空白 | 目录的 Component Set 展开、按需 Design System 样例和页面实例是三个投影入口，必须分别查。alpha.18目录曾把隐藏说明的单行项保留59高，样例占位又统计隐藏子树。2026-10-26的修正对 MenuItem/ComposerMenu 共用native测量，并只在这两个组件的样例占位中忽略隐藏节点；见 `review.json.currentComposerMenuRefreshReview`。不能只重导PNG或修改源的缓存尺寸来补偿 |
| 修改 `Status.Label.text` 等路径报 `unknown_element` | 简写不能任意穿过嵌套 Instance。先读取真实 source node IDs，再用公共 typed instance overrides；不要展开复制子组件 |
| `update-presentation-node` 修改整个 `instance` 报 `unsupported_node_change` | 使用 `select-instance-variant`、`set-instance-override` 等专用操作；先读对应 schema |
| 对实例覆盖 `layout-padding` 被拒绝 | `schema instance` 的 override 字段不包含任意布局属性；复用共享内边距，或在确需调整时修改共享定义／外层装配，不猜字段 |
| 给实例只设置 `props: {State: ...}` 后按钮结构变化 | 高层 props 替换可能把未写的轴重置到默认值；Sender 的 State 改动必须同时保留 Kind=Primary、Content=Icon。检查解析后的 selection，不能仅检查 intent。直接 typed variant 覆盖也要保留完整选择 |
| Hover 按钮比原按钮矮，出现位置跳动 | 实例层把固定高度误写成 auto，覆盖了共享组件的约束。保留源组件的纵向 sizing 与 height Token；不能只把实例 height 数字设成 36，却继续让它 hug。浮层锚点用修复后的 native 高度重新计算 |
| 把 spacing Token 绑定到 height 后出现 `binding_type_mismatch` / `repair_required` | Token 类型必须匹配字段。用 padding / gap 表达间距；确有局部空白块时保留说明清楚的尺寸，不为消灭字面值新增一个专用 sizing Token |
| `--context Viewport=Narrow` 报 `invalid_context_selection` | 本项目 Viewport 是 Token 主题组，使用 `--theme Viewport/Narrow`；Context Axis 与 Token theme group 不是同一种配置 |
| 字体存在却报 `missing_font_variant` | 变体名称必须精确匹配，本次 `SemiBold` 与 `Semi Bold` 不同。读取已导入目录，不凭经验猜名称 |
| 图标容器 `center`，实际图案仍偏左且不同图标偏移不同 | 查字体字形的真实边界和 `hmtx` 左边距。本项目原图标字体错误地把左边距全部设为 0，使 Plus 左偏 3.5px、Close／ChevronDown 左偏约 4.375px；应保留 24 单位画布的左侧留白，写入对应字形的 `xMin`，不能逐个平移按钮或新增偏移 Token |
| 中文或图标变化，旧页面也跟着变 | 字体是共享资源。扩充字库要保留原字形、度量、码点和许可；Token、资产身份与哈希都要核对，不能只看新页面 |
| 同一字体／字重重新 import 报 `duplicate_font_variant` | 先在隔离副本验证。需要替换二进制且保留字体／变体 ID 时，可通过 CLI 临时名称导入 blob，再用 revision 守卫的 `replace-asset-library` 只替换原变体文件引用并移除暂存条目；不要直接改资产 JSON，也不要把暂存字体留下来 |
| 隔离副本与正式包 import 同一新字体后，资产 JSON 不相等 | 首次导入会分配新的 font / variant UUID。按 family、样式、字重和 blob SHA-256 核对资源，再通过受 revision 守卫的资产操作保留已审阅身份；不能把 UUID 不同误当成字体内容不同，也不能跳过内容核对 |
| 跨页面浮层报 `unsupported_cross_page_overlay` | 此版本 overlay 只支持同一 Presentation。按真实结构使用同页浮层或跨页 navigate；后者不能冒充实际叠加交互 |
| Focus 外环或阴影在组件导出中被裁掉 | 本次渲染的组件边界裁剪造成。实际查看 Focus 样本，当前用两道原生内描边处理；不要推广为所有运行平台都不能用外环 |
| 文档声称有旋转、点击、保存，实际只有静态样本或连线 | 设计状态、原型导航和运行实现是三件事，分别验收；不能由静态图推断行为已完成 |
| 自动检查无报错，但用户仍觉得像线稿、按钮不协调 | 结构／对比度检查不能判断产品观感。回到真实页面审阅，不用增加样本数量替代设计判断 |
| 长文本放入固定阅读视口后出现 `content_overflow` | 先核对是否按预期裁剪、输入区是否保持可见；滚动视口需要产品层的真实滚动。不要为消除报告就缩小正文、截掉原始内容，或把整个窗口改成 hug；也不能把静态裁剪称为滚动验收通过 |

## 验收与 Web 刷新

每轮先从实际页面选出最影响使用的少量问题，优先检查阅读层级、操作密度与状态反馈；沿用 Alva 的 Foundation、组件和图标，先修共享定义再核对消费者。验收按变化范围进行：先检查引用与布局，再检查受影响状态／主题，最后查看实际渲染。共享 Token 或全局组件变化再扩展到消费者；无需每个小改动都跑完整仓库运行测试。

- 阅读 `validate` 的 `coverage` 和 skipped 项。目标全部完成、0 issues，不等于全部规则覆盖，更不等于完整无障碍或视觉质量通过。
- 中文阅读单独检查：用真实中文对话、英文标识、代码和日志混排，核对行高、行宽、换行、字体回退与缩放；不把拉丁展示标题的负字距或紧行高套到中文。保留现有文字角色，按实际问题调整共享规则。
- 图标对齐要分三层检查：点击区域、图标框、字形墨迹。CLI spec 可读精确位置／尺寸／对齐；字体内部边距需结合字体资源和渲染确认。对称图标用 4× 原生导出测量墨迹中心，再在 16／20 图标、32／36 控件与真实页面中查看；整页缩略图不能替代这一步。非对称图案仍保留设计画布，不能按各自墨迹框随意拉伸或重心平移。
- 每份报告绑定实际 package revision；同一页面与配置比较修改前后渲染，再到实际 Web 核对受影响表现。检查后又改了颜色就重查相关对比度，改了布局就重看相关布局；旧截图不能改标签冒充最终版本。浏览器不可用时明确保留该项未验收。
- 当前工作区要求视觉／GUI 任务交给临时子代理，主代理只接收文字结论；脚本截图优先降采样。检查完立即关闭代理；工具未提供关闭能力时中断并结束任务，不跨后续工作继续持有图像上下文。
- Go 架构测试通过只证明相应代码／文档约束；SmallPen 导出通过只证明相应设计状态。连续缩放、真实输入、键盘、读屏、执行与性能需要在实际产品另外验证。

以上打磨与排版检查借鉴 Open Design 的 [局部打磨流程](https://github.com/nexu-io/open-design/blob/main/skills/impeccable-design-polish/SKILL.md)和[中英文排版规则](https://github.com/nexu-io/open-design/blob/main/craft/typography.md)。品牌模板的数值、通用超时和重试建议不直接变成 Alva 的 Token 或执行政策；状态覆盖通过共享组件和场景检查完成，不重新铺成逐项画板。

用户指定使用 SmallPen Web，复用 loopback `43128` 的现有服务，不自动打开桌面端。先检查 `/health`；保存后确保服务重新读取正确的包，再让用户刷新原页面。当前 Web 的重新加载接口虽然名为 `/desktop/open-package`，它只是正在运行的 Web 服务路由：请求体字段是 `locator`，用 `path` 会报 `invalid_package_locator`。这是本版本的已验证行为，升级后先检查接口再使用，不重启或替换其他现有进程来碰运气。

2026-10-23 的浮层问题已经给出确定复现，并为现有 43128 服务启用 [进程级布局修正](./source/smallpen_web_layout.mjs)。它只读取同版本 native 测量，保持组件引用、auto/fill、padding、align 和 canonical 数据，按完整投影输入缓存。适用范围包括 secondary roots、ComposerMenu，以及后续发现仍使用旧 hug 宽度或忽略 maxWidth 的 SettingsNav/WizardBack 消费页。启动方式是在已核对的 Web `bin/smallpen-web.cjs` 前使用 `node --import ./docs/design/alva/source/smallpen_web_layout.mjs`，再传原包路径、`--port 43128 --no-open --json`。只对本会话管理的预览进程执行有目的的重启；不能替换无关服务。未加载此修正的 alpha.18 仍有该问题；升级后先验证上游是否修复，模块哈希不同必须移除或重新审阅，不能关闭守卫。组件目录、其他版本和真实浏览器行为不在此修正的验收结论内。

输入区通过原生 requirement/annotation 引用共享浮层和 Tooltip，当前 21/22 不再逐项放置 overlay FRAME。`composer_surfaces.consolidate()` 只用于清理之前生成的重复示例和相应连线，后续重绘不得重新铺回。原型能力存在不等于每处都必须做可点击演示；保留模型选择等已有业务导航，其他触发/内容/关闭/焦点规则由 `req_alva_composer_focus` 统一声明。需要验证新的复杂业务流程时再增加一处代表稿。历史同 Presentation 浮层探针与 Web 修正证据保留，但不作为当前必须保留这些示例的理由。

本轮收尾时 CLI 已更新为 alpha.19，现有 Web 仍为 alpha.18。该原生导出只遍历 Presentation 主根；检查 secondary overlay 时，在隔离副本中通过 typed `put-screen` 临时选为主根，再导出整页。记录探针 revision 和原始节点内容，不能把探针 revision 当成正式稿，也不能为方便截图改变正式页面主根或画布顺序。

保留 package / file / page 身份，用户可以直接刷新；不要每次发新链接、重新释放一套 HTML 或另建包。历史定义保留会增加总目录数量，说明当前正式组件范围即可；未经明确范围确认不要清空历史，也不要把“保留历史”作为当前过度设计的借口。

Go／HTML 实现可以映射这些语义到颜色变量、布局约束、组件状态和轻量动效，但设计稿不证明生产实现与性能已完成。当前生产栈仍以根 README / ADR 为准，设计任务不能顺手改变架构。

## 下一个 AI 的接手清单

1. 读本指南、当前 DESIGN 与本轮涉及目录的 AGENTS；确认用户现在要诊断、改规则、改样式还是改运行行为。
2. 读取当前 revision 和已有定义；文档里的历史数量与成功报告不替代现状。
3. 提出最小必要的属性分工，先做一组真实场景，说明任何新增轴或 Token 的消费者。
4. 按公共 schema 增量修改，保留用户编辑、稳定身份、实例引用和导航。
5. 验证本次变化，记录真实工具限制与未覆盖行为；需要说“CLI 有问题”时给出最小复现和版本。
6. 告诉用户改了什么、为什么以及检查到哪里。对于已授权的小修继续完成，不把可自行核对的问题反复抛给用户。

### 功能如何跟 UI 同步

先读 Product 的 `manifest.entries.requirements`，当前首页契约在 [home-composer.json](./alva.smallpen/requirements/home-composer.json)。SmallPen 有原生 requirement（id/title/markdown/links）与 annotation（id/target/role/tags/description），可绑定 canonical 页面或节点；用公开 `put-requirement-file` 写入，不另造不存在的备注字段。当前并未验证 Web 编辑器有对应的备注面板；AI 可以直接读这些原生数据，不能承诺点选控件就一定能看到备注。

- 每条行为写清触发、前置条件、状态变化、实际动作/owner、退出/取消、异常与一条可复现验收；标明设计、原型、运行各自状态，未确认的决定单独标注。
- 按稳定 `req_` ID 关联真实节点。嵌套实例子节点未必是 Product 的 canonical 节点：绑定它的 Composer 实例，并在规则写 `Composer.Prompt` / `Composer.Send` 的路径，不拿临时展开 ID 伪造可编辑节点。
- `put-requirement-file` 替换整份文件：先读取当前内容，按 ID 合并本次变化；保留其他 requirement/annotation/flow，连同 UI 变更做 revision 守卫和引用检查。不要重新运行旧的全量 intent 覆盖后来备注。
- 变体描述状态外观，interaction 说明原型可执行的跳转，requirement 说明业务规则；三者同步，不以一个代替另一个。当前 native events 没有文本 `/` 触发，`set-state` schema 也不等于 Web 可执行；键盘规则存入 requirement，产品层再实现和测试。
- 14 个 Composer 样本不可当作完整互斥状态机；录音、任务和弹层独立。先审 Running + Recording、Waiting + Draft、Submitting + 新输入这类组合，不新增全排列变体或 Token。

当前 Button 的 Content 轴及 112 个组合仍在保存的设计中；这次新增的是踩坑指南，不是已经完成的组件迁移。下一次确需调整时，先审实际消费者，再决定哪些内容配置保留为变体、哪些移到实例；不要照着“减少数量”重新做一次全量重构。

本指南的依据是本轮用户反馈、安装版 CLI schema、[authoring-log.jsonl](./source/authoring-log.jsonl) 中的真实回执，以及 [DESIGN.md](./DESIGN.md) / [review.json](./review.json) 的分轮记录。生成器体现当前实现，也保留了此次要纠正的建模问题，不能把它当作无条件照抄的最佳实践。

2026-10-26补查了上述历史结论未覆盖的组件目录：修正现在也覆盖MenuItem/ComposerMenu的Component Set展开和按需Design System样例，并只在这两个组件的总览占位计算中忽略隐藏子树。三份安装模块分别校验SHA与唯一标记。native测量由短时工作线程串行派发，活动/等待最多16项，仅发送资产等渲染所需数据；空闲1秒退出，几何缓存最多32项。不要把整包/目录反复克隆排进原生测量队列，也不要把线程生命周期措施宣称为全应用内存问题已经解决。范围、来源及浏览器限制见[ComposerMenu证据](./exports/archive/composer-menu-2026-10-26/report.md)。
