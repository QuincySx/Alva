# Alva 常用组件覆盖与复用

Avoid unnecessary secondary labels and descriptions. Let the layout do the explaining

以当前 23 张正式页面及已明确的工作台、对话、资料、协作、自动化、设置和诊断需求为范围。共享 Foundation 继续复用，页面与业务流程留在 Product；这是设计资源的分工。Go 侧的组件库是 Alva Kit（`apps/ui/kit`，[ADR 0014](../../adr/0014-alva-kit-component-library.md)）：token 与组件目录以本设计为单一来源，经导出脚本进入 Kit，不对外独立发布。

正式目录为 **56 个 Alva 组件、481 个原生变体**（2026-11-09 起在新格式包 `alva.smallpen` 中由 `source/build_next.py` 生成，命令写法见 [CLI-GUIDE](./CLI-GUIDE.md)；下文提到的 `page draw`、intent 等旧 CLI 用法仅为历史记录）。ProviderForm仅区分Preset/Custom结构，ProviderModels独立表达6种目录状态，不将供应商与状态相乘。输入区的Composer使用14个视觉样本；草稿、执行、录音、浮层在运行时独立，不照抄为互斥枚举。Icon保留68个原始符号，并用原生flipX表达唯一PanelLeft语义选择；已有Button的112个组合计入总数，数量不是完整度指标。历史组件另行保留。

## 原生顶部壳复用 · 2026-11-05

`Alva PageHeader.Navigation=Expanded/Collapsed` 是本轮唯一新增局部结构轴；Header36绑定既有 `ui.control.height`，图标动作32，标题16/来源12横排。Expanded的开关在Sidebar顶部，Collapsed由外层隐藏整棵Sidebar，将同语义PanelLeft开关放入页头。Sidebar不再以Narrow主题代替收起状态；展开统一220且保留标签。SettingsNav208顶行36，主设置页头的HeaderReturn复用Ghost/Leading Button，不建立新的返回组件；连接Sheet搬原WizardBack节点，不复制按钮。

`Alva Icon.Name=PanelLeft` 使用已有Lucide panel-right字形的原生水平镜像，仍保持同一24网格、字体度量、双轴居中；原Panel继续表示右侧资源面板。没有手画图形、修改68个字体字形或逐图标偏移。运行端使用标准Lucide panel-left语义。源维护入口为 `source/native_shell_refresh.py` 的一次性revision守卫迁移；系统safe-area不是静态Token，具体规则和最新修订见[原生壳定义](./DESIGN.md#已保存的原生内嵌标题栏与导航壳--2026-11-05)。

## 组件维护与业务组合

先通过 `smallpen help skills --json` 加载 bundled `smallpen-ui-design`，再按 [DESIGN 的现行工作方式](./DESIGN.md#现行工作方式基础规范spec与组合预览--2026-10-26) 确定维护基础库、组合预览或正式页面的范围。使用下面的现有组件，先读允许的覆盖与尺寸约束；普通容器承载业务内容，功能 SPEC 引用通用定义并记录差异。包外 intent、`page draw --preview`、证据留存与 Review 字段统一见 [CLI-GUIDE](./CLI-GUIDE.md#组合预览复用定义不保存样例页面)，不为每个使用位置再建一套弹层状态。

## 去哪里改

| 修改内容 | 位置 |
|---|---|
| 颜色、字体角色、字号、间距、圆角、控件高度、明暗主题 | SmallPen Web 的 **Foundation → Tokens**；作者定义在 `source/design_system.py` 的 `token_definitions()` |
| 共用区块的 padding／gap／布局约定 | 同文件的 `block()`；复用现有标尺，不为每个区块再建 Token |
| 按钮、输入、选择、阅读内容等共享组件与变体 | **Foundation → 原生 Design System／组件目录**；作者定义在 `definitions()`、`complete_common_states()`、`content_definitions()` |
| 页面内容、组合、导航、具体交互流程 | **Product → 01 产品界面**；对应 `source/product_pages.py` 等页面作者文件 |
| 输入区生命周期、权限／添加／引用／命令菜单、录音 | **Foundation → Alva Composer / PermissionMenu / ComposerMenu / VoiceInput / RunStatus**；`source/composer_states.py`。菜单文案是内容，权限值不与运行状态相乘 |
| 首页触发、退出、异步回执、实现范围 | **Product → requirements/home-composer.json**；原生 requirements / annotations 绑定 21/22 真实节点。不是页面上的辅助文案；改 UI 同步核对行为与未定项 |
| 连接表单与可选模型目录 | **Foundation → Alva ProviderForm / ProviderModels**；`source/design_system.py` 的 `provider_definitions()`。具体来源、端点和模式是实例数据；24/25/26装配在 `product_pages.provider_setup()` |
| 连接来源、折叠/测试、获取/手填、保存/返回 | **Product → requirements/providers.json / navigation.json**；含真实CRUD/CAS、秘密草稿、启停/删除与尚缺Test/自动目录/账户认证，绑定真实节点，不增加画布说明或浮层副本 |

先读取保存包与 revision，再按 [CLI-GUIDE](./CLI-GUIDE.md) 增量修改。代码是作者输入，不能用全量重新生成覆盖 Web 中的最新编辑。两个包保持相邻与稳定身份，原来的 Web 页面可以刷新继续使用。

正式视觉以 [2026-11-04 已确认基准](./DESIGN.md#当前正式视觉基准--2026-11-04) 为准。布局增量由 `source/boundary_refresh.py` 维护，保留旧正式节点和功能绑定；源定义的默认值不替代保存包的后续 native finishing。修改共享组件后也要检查实例是否冻结了旧颜色，迁移时保留真实文案、语义图标、几何、可见状态与 interactions。

## Button 加载态与复用

`cmp_alva_button` 的 Loading / LoadingFocus 保留原动作文字与布局宽度。Text 将原文字设为透明但仍参与布局，Loader 绝对居中；不能把文字隐藏退出布局。Leading / Trailing 在原图标槽显示 Loader，后置图标加载时仍在文字右侧；Icon 保持原正方形。实例切状态时沿用相同文案与父容器的宽度分配，不把动作名统一替换成“处理中”。需要描述进度时使用所属流程的状态内容。

运行控件负责保留可访问名称和焦点、暴露 busy、抑制重复激活及落实减弱动效。静态透明文字和 Loader 不证明这些运行行为已经实现。VoiceInput 的“转写中”是需持续可读的状态内容，使用带前置 Loader 的结构；ProviderModels 获取按钮沿用原动作名。

共享定义仍为同一组 Kind / State / Content，没有新增 Token、组件或变体。新增 Icon 实例必须显式携带共享宽高；CLI copy 默认 hug，不能从源样本为正方形推断新实例也自动保宽。[组件审查](./exports/archive/component-audit-2026-10-26/controls-audit.md)记录修正前的宽度风险、Dialog 与菜单检查；[隔离验证](./exports/archive/button-loading-2026-10-26/report.md)记录短／长文案及 fill 的加载态方案，正式保存后的证据由 `review.json.currentFoundationControlsReview` 关联，不能把隔离图改标为正式修订。

## 已有的共享组件

表内省略 `Alva ` 前缀。变体表达状态和必要结构；标签、图标名称、内容、是否显示说明以及宽度通常是实例设置。

| 类别 | 组件 | 使用边界与关键状态 |
|---|---|---|
| 动作 | Button、IconButton、ToggleButton | 主次／危险／轻量动作，hover、pressed、focus、disabled、loading；切换项保留选中与选中后焦点／禁用 |
| 输入 | Input、Field、TextArea | 文本、搜索、选择入口、密码；默认、焦点、错误、错误时焦点、禁用、只读；Input／Field 另有加载。Field 管标签、提示和错误 |
| 选择 | Checkbox、Radio、Switch、SelectOption、Segment、QuestionOption | 多选含混合态；单选／开关保留选中后焦点与禁用；下拉选中和键盘焦点独立；选项说明默认不展示 |
| 导航 | NavItem、Tab、MenuItem、Pagination、Sidebar、SettingsNav、PageHeader、PanelHeader | 当前位置、焦点、选中后焦点／禁用；页签忙碌；翻页加载／结束。首尾按钮由实际分页状态覆盖 |
| 标记 | Icon、Avatar、Badge、Step | 共享符号、身份、语义状态与流程步骤。静态状态用 Badge；可操作筛选用 ToggleButton／Segment，不把 Badge 当按钮 |
| 反馈 | Notice、Toast、Tooltip、EmptyState、Progress、Skeleton | 就地错误与恢复、短回执、必要提示、空／无结果／加载／错误、进度及骨架。具体重试动作由所在流程提供 |
| 列表与详情 | ConnectionRow、ModelRow、SkillRow、ResourceRow、TaskRow、CompanionRow、TreeRow、DetailRow | 连接、模型、技能、资料、任务与伙伴；排队、运行、等待、完成、失败分别表达。树的类型、选中和焦点分开；详情值可选择／复制 |
| Agent 对话与资料 | Composer、PermissionMenu、ComposerMenu、VoiceInput、RunStatus、MessageHeader、ToolActivity、Attachment、CodeBlock、Citation | 输入、菜单、录音转写、任务收尾、工具活动、附件、复制回执与来源定位。长正文用共享文字角色排版 |
| 问答与表单容器 | Approval、Question、DialogHeader、DialogFooter、SettingRow、ModelForm、ProviderForm、ProviderModels | 待处理／提交／过期等业务事实；模型Add/Edit共用，编辑时ID只读；连接Preset/Custom结构与目录状态独立 |

ConnectionRow 的尾部 Check 表示当前选择，使用 accent；普通/未配置行用中性 ChevronRight，不把默认行画成测试成功或等待。保存、启用、目录访问与推理成功分别来自实际事实。RunStatus 的 Metrics 默认隐藏且内容为空，只有得到真实测量数据才提供文字并显示；正文工作状态和必要操作仍保留。正式页面本轮装配与证据见 [页面更新](./DESIGN.md#正式页面稿更新--2026-10-26)。

## 设置行与填写表单

偏好设置使用SettingRow：左侧是名称和必要说明，右侧是Switch、Action或Select。Select是新增的结构变体，复用Ghost/Trailing Button和既有SelectOption菜单；当前值、菜单内容、展开与焦点属于调用数据及宿主行为，不增加状态轴或每项弹窗画板。输入名称、地址、密钥等填写场景继续用Field/Input。

通用页31的外观采用连续设置行，值按钮宽度随内容，标签区填充剩余空间；组内gap为0，行内上下padding均为8，末行不再画分隔线。SettingRow标签用常规字重，必要说明间距为4。Switch的On使用accent/onAccent，Focus/OnFocus沿用共享的主题色焦点环和表面色间隔；现有6种状态及组件身份保留。

编辑入口为 `design_system.definitions()` 的SettingRow/Switch、`product_pages.preference()` / `general()`；调用规则在原生 `requirements/preferences.json`。SmallPen编辑器的对象选中轮廓不是组件焦点样式，判断颜色应读保存稿的tokenBindings和实际导出。

## 图标只用一套

全套界面符号使用 **Lucide 1.61.0**，不混搭其他图标库，不再手画近似版本。使用官方 24×24 画布、2px 圆角线条；在现有 16／20 图标槽内等比缩放，颜色跟随文字／状态角色。来源与 ISC／MIT 许可见 `fonts/lucide/`；`fonts/icons.json` 是 68 个稳定 Alva 名称到官方图标的完整映射。

常用映射：Agent → `bot`，Chat → `message-square`，File → `file-text`，Task → `list-checks`，Plugin → `plug`，Shield → `shield-check`，Loader → `loader-circle`，Refresh → `refresh-cw`。加载与重试保持不同含义；不会给所有 AI 功能都加 Sparkles，也不靠装饰图标填满留白。

Go／HTML 优先按需使用本地 SVG 或同源 sprite；不需要 React、npm 或全量图标字体。SmallPen 的子集字体只是当前设计工具的适配格式，原始 SVG 仍是可复用资源。新增符号优先从同一固定来源选择，保留命名和许可映射，不自行调整单个图标的笔画或位置。

## 本轮新增项的契约

| 组件 | 原生状态 | 实例内容与运行责任 |
|---|---|---|
| SelectOption | Default、Hover、Selected、Focus、SelectedFocus、Disabled、SelectedDisabled | 标签、可选说明；选中打勾，焦点描边。列表弹出、搜索、键盘移动由宿主负责，多选复用 Checkbox |
| Attachment | Ready、Uploading、Error、Unavailable | 文件名、大小、真实进度与原因。失败时才提供重试；移除的是草稿引用，上传中取消须等待实际清理 |
| CodeBlock | Default、Copied、CopyError | 语言与原始代码／日志。保留空白，复制反馈来自实际结果；横向滚动、长日志虚拟化、折叠由运行层实现 |
| Citation | Default、Focus、Unavailable | 序号、来源标题、具体位置；不可用仍保留来源身份与位置。打开来源不等于已向模型投递或授予权限 |
| DetailRow | 默认布局 | 标签、可选择的值、可选复制。值随宽度换行；窄容器由页面将标签和值纵排，不为宽窄新增变体 |

代码、日志与标识符共用唯一新增的 `ui.font.mono`，使用 [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono)；字体许可和固定来源记录在 `fonts/JetBrainsMono-OFL.txt` 与 `fonts/JetBrainsMono-NOTICE.json`。中文继续使用项目正文的字体／回退策略；代码样本的 ASCII 等宽渲染已纳入本轮检查，混合脚本与平台回退需运行验收。颜色、字号、间距和圆角档位没有扩张。

## 用现有组件组合的部分

这些是页面组合约定，不是已经实现了全部行为的独立控件。

ProviderForm的Preset默认收起完整连接设置、突出Key；Custom直接显示名称/认证、协议和完整端点。展开是现有组的显隐和自然增高，不额外复制组件。创建/编辑、供应商名称、端点、认证集合和返回目标由调用方携带；测试和保存的忙碌/错误复用已有Button、Field和Notice。

ProviderModels的Idle、Loading、Ready、Empty、Unavailable、Error分别表示尚未请求、获取中、目录可用、成功空目录、不提供目录和获取失败。手填始终可用；有效且不重复的ID才允许添加。目录获取可在保存前显式发起，但不是保存前置条件。长目录滚动/虚拟化、真实搜索、异步过期结果、联网、凭据存储及保存回执仍由产品实现；静态Ready的两个勾选只是样例。

输入区现有状态与原型范围：

| 层 | 设计状态／关系 |
|---|---|
| Composer | Idle、Focus、Ready、Attaching、Submitting、Running、Waiting、Stopping、Stopped、Complete、Failed、Recording、Transcribing、Disabled。空输入禁发；提交中避免重复发送；工作中保留独立停止动作；Failed 表示提交失败保留草稿，RunStatus.Error 表示执行失败；收到真实清理回执才从 Stopping 进入 Stopped |
| VoiceInput | Idle、Hover、Focus、Recording、RecordingHover、Transcribing、Error、Disabled。计时是内容，停止录音与停止任务是两个动作 |
| PermissionMenu | Ask / Auto / Full 三种选中值；选中勾与左侧图标；权限边界放 Tooltip。更改设计变体不授予真实权限 |
| ComposerMenu | Add、References、Commands、Empty、Loading、Error。MenuItem 隐藏 Description 与 Shortcut 后按内容撑高，上下/左右均 8；列表项间隔 0，外壳 padding 8/gap 4，页脚 margin 上下4/左右8。空状态复用相同20px图标列、8px图文间隔和对称padding8；详情padding12/gap8。不增加高度 Token。命令选中项有详情，窄容器在实现中纵排详情 |
| Hover / Tooltip / Popover | Hover 是控件状态；Tooltip 提供必要解释；Popover/Menu 是点击后可操作的面板。键盘 focus 同样需要提示，Esc 关闭并恢复焦点，触屏不能只靠 hover |

21 首页和 22 会话页通过原生备注引用共享菜单和 Tooltip；原来每页 11 个浮层示例已撤下。41 添加菜单保留一处 ComposerMenu 代表装配，其他菜单内容在 Foundation 维护。通用容器共用 `design_system.popover()` 配方，调用位置只写触发条件、组件/内容引用和状态值，不为每句提示重画。SmallPen alpha.18 原型支持 open/close-overlay，但不需要每处都制作可点击演示；`@`、`/`、焦点、筛选、录音和任务动作仍需在产品中实现与验收。

输入区对齐修订：Editor对称padding8、工具栏高32，Send/Stop为32圆形/圆角16/图标18，Add图标18，Permission/Model下箭头14。`@`/`/`菜单在整个编辑卡片左上方6，紧凑工具栏菜单优先在各自触发点上方6，按视口翻转/夹紧，独立于布局流。普通Button及全局Token保持。运行Permission/Model标签对称padding4/12；SmallPen不支持嵌套Instance局部layout-padding，静态稿使用共享Button居中布局及左右12，不声称逐像素一致，此限制见`review.json.currentComposerAlignmentReview`。

| 场景 | 组合方式 | 页面／运行层负责 |
|---|---|---|
| 下拉、Combobox、模型／指令搜索 | Input + SelectOption／ModelRow／MenuItem + EmptyState + 浮层容器 | 弹层定位、搜索结果、IME、焦点移动、Escape、关闭后焦点恢复 |
| 对话框、确认框、抽屉 | DialogHeader／PanelHeader + 内容容器 + DialogFooter／Button | 滚动、焦点约束、背景可交互性、关闭规则、未保存内容处理 |
| Markdown／长回复 | 共享标题和正文角色 + 列表／引用布局 + CodeBlock + Citation + ResourceRow | 解析、安全呈现、链接、流式增量、选择／复制与滚动定位；不复制每篇内容为新组件 |
| 结果表格／批量操作 | 共用列布局 + 文字／Badge／Checkbox／Button + Pagination + EmptyState | 列定义、排序、筛选、选择范围、宽度与虚拟化；列结构随业务确定，不预制固定三列表格 |
| 文件与资料树 | TreeRow + 缩进／子项容器 + MenuItem | Folder／File 与 Selected／Focus 独立；展开时覆写箭头，父层管理层级、键盘和按需加载 |
| 工具调用与 Inspector | ToolActivity + DetailRow + CodeBlock + Citation | 展开内容、原始事实引用、参数／结果、耗时、错误恢复；观察与停止分开 |
| 计划、队列与执行记录 | TaskRow + Step／Checkbox + Progress + 详情容器 | 已排队、待回答、待审批与执行中不能混为同一状态；停止、重试按真实 owner 的生命周期处理 |
| 可移除筛选与已选资源 | ToggleButton／SelectOption + IconButton，或 Attachment | 名称、选中语义、清除动作与键盘顺序；无须再造颜色／尺寸档位 |
| 分隔线、键盘提示、面包屑 | 现有边界／文字角色 + 图标与布局 | 按信息层级装配，当前无需单独组件或 Token |

## 页面覆盖与后续缺口

| 页面族 | 当前可直接使用 | 尚需随具体页面确定 |
|---|---|---|
| 工作台、会话、模型选择 | 导航、输入、列表、筛选、空态、Composer | 全局搜索的分组／历史／快捷键及真实结果排序 |
| 对话、成果、资料与记忆 | 消息头、代码、附件、引用、资源行、树、详情 | Markdown 完整排版、文档／图片／音视频查看器、文内搜索、长内容滚动 |
| 模型、连接、技能、插件、设置 | 表单、选择、开关、ModelForm、ProviderForm、ProviderModels、状态、弹窗首尾；本波原生连接/手填模型CRUD与权限策略按共享product接线 | 自动目录/测试、账户认证分支、39联合选择器与42/43完整模型默认参数、每个新插件配置的字段校验；详见DESIGN当前实施状态 |
| 伙伴、群组、任务、问答、审批 | CompanionRow、TaskRow、Question、Approval、Step | 实时状态、异常恢复、任务调度与真实权限处理 |
| 自动化与日程 | 表单、列表、任务记录、详情、反馈 | 时区、重复规则、日期／时间选择、下一次运行与冲突校验；尚无专用日历组件 |
| Inspector、诊断、测试／评估 | 详情、日志代码块、筛选、状态、分页与结果表格组合 | 大结果集、差异查看、图表／执行图与性能指标的具体交互 |

当前常用基础层已覆盖上述页面的骨架；未来所有专业页面不能仅凭组件目录承诺完整。日期时间选择、图表／执行图、差异编辑器、富文本编辑和媒体播放器应在出现明确字段与交互要求时补充。先验证实际页面，再判断是否值得成为共享组件；不会为了“可能用到”先放一批空壳。

## 实现与验收约定

- 宽度由父容器分配，正文与说明自然换行；控件内容居中使用共享行框、对称 padding 和明确的两个轴。目录样本宽度不能硬编码成所有页面宽度。
- 有选择就保留选择；有键盘焦点就明确表达焦点。错误时仍需可聚焦；只读值可选择／复制，不能一概当作禁用。Loading 使用 Loader，重试使用 Refresh；旋转与减弱动效在运行层落实。
- 按钮名称、输入标签、错误关联、焦点顺序、读屏语义、IME、缩放和触屏目标需在实际 Go／HTML 或对应平台验收。静态变体不执行这些行为。
- 新增内容先复用现有颜色与标尺；需要新状态时只补适用变体，不把主题、内容长度、宽度、任意图标和尺寸相乘。
- 本轮的保存修订、组件／页面检查、视觉样本、字体来源与保留身份记录在 旧 `review.json` 的 `currentComponentCoverageReview`（已随旧包删除，见 git 提交 690c4525）。与前轮全页审查分别记录，不以重新加载 Web 成功代替浏览器交互验收。
