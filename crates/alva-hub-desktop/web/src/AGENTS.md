# alva-hub-desktop/web/src
> Hub Desktop React session list、workspace、composer 与产品 timeline。

## 地位

Tauri webview 的纯表现层。只消费稳定 IPC DTO 和 `hub_timeline` 产品事件，
不理解 BaseAgent 或 adapter-native stream。

## 逻辑

1. `bridge` 创建独立会话 workspace 或选择项目 workspace、提交/恢复 prompt、请求手动 engine switch、global/session fallback policy 与 engine readiness，并读取持久化 session。
2. `StartupGate` 提供对话与项目两种入口；对话创建专属受管目录，项目显示持久化项目及只读发现的工作树。`App` 与 Sidebar 可选择有效工作树作为下一会话 cwd，缺失/bare/prunable 工作树不可启动；不迁移已有会话、不隐式创建仓库。随后 App 管理 session list、EngineBranch、fallback chain、switch queue、历史 Turn/Attempt 和用户输入。
2.1. Settings 的 provider 连接先保存到后端私有 catalog，再通过 IPC 做远程模型发现或 exact route 的最小推理测试；同一连接由后端按协议自动派生 ALVA/Claude Code/Codex/Pi route，web 只拿脱敏的发现候选与 non-authoritative registry binding preview，用户逐项或批量确认后才进入 draft，Save 时后端重新校验 binding，随后才成为 Composer route。
2.2. `i18n` 是产品文案入口：默认简体中文，支持 English 与跟随系统，选择持久化到本地 WebView；领域组件使用 typed message key，不在组件内按语言写条件分支。
2.3. `appearance` 在应用级装配持久化外观偏好：深色/浅色/实时跟随系统、default/compact 全局 Tokens 与减弱动效；OS 无障碍偏好不能被应用关闭。设置修改立即生效，存储失败可见，不声称重启后保留；卸载清理媒体/跨窗口监听与根属性。
3. `App` 的 Engines dashboard 展示每个 engine 的版本/能力/auth/stream health 与结构化不可用原因；Composer 保留全部已配置 route，可用项在前，不可用项在底部独立分组并显示具体原因；可用 route 的悬浮选项面板只展示后端按 provider × model × engine 声明的 reasoning 档位与 Fast 能力，未知模型不猜测选项、不展示 Fast，选择写回真实 EngineTarget。
4. 自动 fallback 以非阻塞 `fallback` timeline card 呈现 source→target、原因与 handoff 摘要；skip 复用 fallback 投影，链路耗尽以 `exhausted` card 展示全 cycle 终态（live event 与持久化 fact 同一投影）。
5. fallback card 携带 Stop / Switch back / View handoff 控件：Stop 仅在运行中可用，Switch back 在已回源时禁用，View handoff 展开结构化 HandoffPackage；`attempt_cancelled` fact 投影为 cancelled card。
6. 新 session 在 composer 顶栏选择 PermissionProfile（默认 workspace_write），session 打开后只读；engine selector 按 `get_permission_eligibility` 把只能弱化 profile 的 engine 标记 ineligible（禁用 + 原因），可执行映射的等价/更严格摘要就地展示。
7. `permission_required` timeline card 携带 Allow/Deny 控件，经 `respond_hub_permission` 按 request_id 路由决定；已回答的卡片本地锁定不可重发。
8. Team panel：为 active session 创建 TeamRun、Add worker（多 engine 并行批次）、按 worker 队列派发/Message；queued 任务就地 Edit/Cancel；每 worker Focus（打开其 durable session）与 Interrupt；`worker_outcome` fact 投影为 `worker` card，live worker 事件触发队列刷新（teamLeadRef 防 stale closure）。
9. 派发任务可声明 ResourceClaims（`src/lib.rs` / `read:docs` / `facility:x` / `tool:git`），队列项展示各自 claim；`resource_wait` fact 投影为 worker card 显示等待资源与持有者。
10. Add worker 可勾选 Isolated worktree；隔离 worker 卡片带 isolated 徽标与 worktree 分支，提供 Review changes / Adopt / Discard worktree，预览展示变更、冲突与验证证据后才可 Adopt。
11. worker 卡片按 lifecycle 提供 Hibernate/Archive（active 时）或 Restore（否则），非 active 带状态徽标；archived worker 默认不出现在活跃面，"Show archived" 可显示；"End run" 归档全部 worker。
12. 等价并发审批合并为一张 approval-group 卡（逐条列出 requester/工具/资源），一次 Allow all / Deny all；Team panel 有独立的 Autonomous run 开关。
13. 左栏支持 New session、按 workspace/prompt/engine 搜索过滤；composer footer 一处集中 engine、model、PermissionProfile 与 fallback 链（含未授权标注）。
14. Details 抽屉（顶栏 Details 开关）检视 EngineBranches、EngineAttempts（失败原因/用量/工具证据）、handoffs（含 team snapshot 与 delivered/pending）与 engine diagnostics；timeline 本身只渲染产品 TimelineEvent，并从 `created_at`/合并时保留的 `started_at` 投影 Input/Agent/Tools 三泳道概览，绝不读取 raw adapter stream。
14.1. Workbench Inspector 与 Details/timeline 分离：`bridge.getHubInspection` 从 Tauri command 或同构 HTTP endpoint 读取稳定 `HubInspectionSnapshot`，运行中每秒刷新当前 native session；`EngineInspectorPanel` 展示 Run/LLM/Tool/Middleware/raw durable records，点击 child session 压入 Agent 页栈，Back/Esc 依次退出详情页、当前 Agent、返回父 Agent。非 Alva session 只展示 truthful unsupported 与产品级 Attempt。
15. 卡片控件按"本窗口正在跑 **或** store 说 session 在 running"启用（只看快照会在自己这轮运行期间把 Stop 关掉——sessionDetail 是运行后才刷新的）：终态时 Stop 禁用并显示"该 attempt 已结束"，Switch back / View handoff 仍可用；终态事件到达时回刷 sessionDetail，抽屉与闸门不会停留在旧状态。
16. `resetSessionSurface` 是 New session 与切换 workspace 共用的清场：profile、engine、fallback 链、transcript 开关、已答审批、按索引存的 handoff 展开态一并复位——否则上一个 session 的 profile/链会被当成新 session 的默认值（甚至被存成全局默认）。
17. `ui/` 是唯一基础组件入口，封装 Radix 的焦点、键盘、碰撞和 Portal 行为；业务页面不得直接从 `radix-ui` 导入。
18. `styles/tokens.css` 是颜色、间距、圆角、阴影和动效唯一来源；`styles/primitives.css` 只服务 `ui/`，页面不得复制基础组件规则。
19. `index.css` 暂时保留旧业务 surface 的兼容样式，迁移完成后只负责导入 tokens、primitives 和页面布局。
20. UI 严格按 Tokens → Controls → Regions/Surfaces → Domain Components → Pages 五层依赖；页面不得越级改 primitive 状态。
21. Cindy 参考图约束结构、层级、密度、弹出位置和操作路径；颜色、字体语义、图标和品牌强调由 ALVA token 决定。
22. Region 明确拥有 width、scroll、dock 和 responsive；一个区域只能有一个 scroll owner，断点只改变区域关系。
23. 动效遵守 `docs/ALVA-HUB-UI-MOTION.md`：高频导航无位移，交互动画可中断、低于 300ms，并支持 reduced motion。
24. 组件只提供 composition、slots 与有限 typed variants；仅支持 default/compact 两档密度，页面不得手写控件高度。
25. 页面完成必须同时通过确定性 fixture 与真实 store/Tauri command；视觉 review 使用 reference/linework/current 三联图。
26. Settings 的 Inspector 操作打开真实 Workbench 页；⌘/Ctrl K/N/B 连接搜索/新会话/工作台，Esc 只在无覆盖层的中央对话停止当前会话。Stop ack 不是终态，真实 terminal event 才收起可取消状态；未接通的独立 Workbench 窗口与扩展样例明确不可用。
26.1. ALVA 在当前会话的已提交 user_message 到达后即开放启动期 Stop，adapter 消费者关闭会中断组装；新会话仍必须先匹配 run_token，不因后台 worker 事件抢身份。其他 engine 继续等待 running，不提前声称其启动可取消；布局/动效不变。
27. Extensions 的 Skills 标签接真实 session-scoped 文件目录，不冒充全局或完整运行时 registry。可创建/编辑受管项目 SKILL.md、显式确认归档与恢复，生态源只读；保存失败保留草稿、不假报成功，session 切换丢弃迟到目录/文档。仅后续 ALVA 运行读取变更。
27.1. Plugins 标签与 Settings/Tools 共享真实 ALVA catalog 能力配置和 stdio MCP 管理；不展示伪 GitHub 样例或 Installed 状态，第三方 AEP 安装明确未接入。开关使用后端 revision，失败保留事实；MCP create/edit 重新禁用，enable/test 显式确认宿主访问，删除确认只删配置。参数每行一个，不解析 shell/安装包/继承凭据；test 绑定已存 session cwd，列出工具并关闭，结果不冒充实时状态。UI 不声称配置控制其他 engine 或热改当前 Agent；安全 substrate 无关闭入口。
28. Automations 接真实 SQLite 配置与状态：模板只预填 form，新建/编辑后暂停；启用弹窗展示 model/provider/account/billing 和固定权限，显式勾选计费授权。间隔不是 cron/本地时间，主窗口/服务关闭不运行；失败/Stop/route change 暂停，busy/missed 跳过，不补跑。暂停不代替当前 session 的 Stop；可打开对应 session 处理审批、结果和取消。轮询卸载清理，失败保留草稿与真实 revision，不伪造成功。
29. 首页有真实最近会话；选择目录后显示仅预填草稿的起步动作。用户消息保留原文，助手使用安全 Markdown，失败原因/partial text、工具、审批与 fallback 分开呈现。搜索/复制覆盖已载入消息及工具预览，不冒充分页工具全文检索；窗口外搜索通过 canonical key 定位并退出底部跟随。
30. 上传传原始字节，单文件 20 MiB、每 Turn 总计 40 MiB/8 文件；后端保存 session 文件，prompt 仅元数据/引用和处理清单，不自动送正文/媒体。草稿预览/移除、失败恢复、会话切换隔离；历史预览走 session-owned API。上传本身不压缩/转码/抽帧/分析；后续 hub_media 的独立派生清单/receipt 可在消息内只读查看，4 项索引/点击私有 BLOB、5 项固定高水位回执/显式刷新、失败重试与迟到结果隔离。Blob 本地预览不算模型输入，响应不证明理解，UI 不处理或投递媒体。
31. 消息模块以 alva.display v1 纯数据 envelope 进入可信编译 renderer registry；表格/图表/图示有真实 ChatTranscript/ToolContent 消费者。未知/无效/renderer 失败保留原文，不从远端安装代码；范围与协议见 docs/ALVA-HUB-MESSAGE-MODULES.md。

## 约束

- 不直接渲染 raw adapter event。
- browser-only 模式必须给出清楚错误，不产生未捕获异常。
- 提供 ALVA/Claude Code/Codex/Pi EngineTarget；session list 不泄露 engine-native session ID。
- 运行中选择目标 engine 只显示 queued；用户选回 active engine 会撤销 pending switch。
- 保存 fallback target 前必须展示 model/provider、账号来源、账单路径与自动使用授权；手动 switch 不改变授权。
- interrupted 会话保持可选择、可继续，不能伪装成 running 或普通 completed。
- readiness 原因渲染结构化 reason code，不透传 stderr 文本；Refresh 只读，不改任何配置。
- `dependency_unavailable` 表示运行时安装缺失或不安全，详情显示后端固定说明、摘要引导安装；不能当普通登录错误。
- 基础视觉与交互决策以 `docs/adr/0006-hub-tauri-ui-system.md` 为准；不得恢复旧 GPUI 产品 UI 路线。
- 所有交互必须键盘可达、focus 可见并满足 WCAG AA；中英文 fixture 不得破版。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| React entry | `main.tsx` | 挂载应用 |
| Vite types | `vite-env.d.ts` | `import.meta.env` 与开发 UI Lab 环境类型 |
| I18N | `i18n.tsx` | 简体中文主语言、英文回退、系统语言解析、持久化偏好与 typed 翻译函数 |
| Appearance | `appearance.tsx` / `appearance.test.tsx` | 全局主题、密度与可访问动效的持久化/跨窗口同步、保存失败和 AA 色彩/居中契约回归 |
| Product UI | `App.tsx` | 新会话 workspace mode、engine selector、permission profile、fallback、composer、timeline、permission card 与上下文限定快捷键 |
| IPC bridge | `bridge.ts` | workspace 创建/选择与 run/switch/policy/readiness/session/permission/Inspector 的 Tauri/HTTP 封装及稳定 DTO |
| Style entry | `index.css` | 按 tokens → primitives → domain → app 顺序装配全部样式 |
| Design tokens | `styles/tokens.css` | ALVA 颜色、间距、圆角、阴影和动效唯一来源 |
| Primitive styles | `styles/primitives.css` | 基础组件 hover/focus/open/disabled 状态 |
| UI facade | `ui/index.ts` | 页面和领域组件唯一 UI import 入口 |
| Actions | `ui/button.tsx` | Button、IconButton 与 Tooltip 约束 |
| Fields | `ui/fields.tsx` | Input、Textarea、Select、Checkbox 与 FieldFrame |
| Overlays | `ui/overlays.tsx` | Popover、Menu、Dialog 和 UIProvider |
| Surfaces | `ui/surface.tsx` | Surface、PanelSection、Badge、Notice、StatusListItem、Tabs、Switch、EmptyState |
| Layout | `ui/layout.tsx` | 可调宽分屏 AppFrame、响应式 ContentFrame、ScrollRegion、DockRegion 与 WorkbenchPage |
| Navigation | `ui/navigation.tsx` | NavGroup、NavList、NavItem 与统一 hover/active 语义 |
| Disclosure | `ui/disclosure.tsx` | Radix Collapsible 封装的展开区和活动行 |
| UI tests | `ui/ui.test.tsx` | 不依赖 Hub 业务状态的组件行为回归 |
| Model picker | `components/model-picker.tsx` | 锚定 Composer 的右侧运行时/模型目录；可用 route 悬停或聚焦时在左侧只浮出该模型真实支持的 Fast/思考强度，未知模型不猜测，不可用 route 置底且不触发面板 |
| Provider settings | `components/provider-settings.tsx` | 供应商连接、发现候选确认、已识别/自定义模型身份、派生 route 测试/默认值与可用性设置 |
| Composer | `components/hub-composer.tsx` | 聊天输入、上下文动作、模型与发送动作 |
| Conversation foundations | `components/conversation-start.tsx` / `components/safe-markdown.tsx` / `components/copy-text-button.tsx` | 起步草稿、安全 Markdown/原生链接、明确复制反馈 |
| Input attachments | `components/input-attachments.tsx` / `components/input-attachments.test.tsx` | 原字节上传、限额/失败草稿与会话归属历史预览、真实未处理说明、媒体 URL 生命周期 |
| Attachment preview | `components/attachment-preview.tsx` | 原件/派生共享惰性文本和本地媒体预览，惰性处理清单与可回收 Blob |
| Media evidence | `components/attachment-media.tsx` / `components/attachment-media.test.tsx` | 消息内分页派生索引/显式预览/来源清单/回执与重试、隔离、焦点回归，不发送模型请求 |
| Message modules | `components/message-modules.tsx` / `components/message-modules.test.tsx` | 可信有界数据 renderer registry、表格/图表/图示与逐模块错误隔离 |
| Workspace gate | `components/workspace-gate.tsx` | 新会话中央“独立会话 / 项目”双入口，不在确认前创建目录 |
| Sidebar | `components/hub-sidebar.tsx` | 产品导航、会话搜索与持久会话列表，不承载 Workspace 创建/切换 |
| Topbar | `components/hub-topbar.tsx` | 对话标题、状态、详情与右侧工作台入口 |
| ChatTranscript | `components/chat-transcript.tsx` | 中央消息/工具/审批/状态共享一个有界窗口；消息使用安全 Markdown、代码复制与跟随/回底，execution renderer 消费非消息行 |
| Timeline | `components/hub-timeline.tsx` | 中央执行行与 Workbench 执行概览；工具参数/输出默认展示，审批/失败/fallback 动作保留，全文引用透传有界分页组件 |
| Tool content | `components/tool-content.tsx` | 自动读首块、前后翻页、失败重试；只持有当前块，旧记录明确标注摘要 |
| Tool content tests | `components/tool-content.test.tsx` | 分页不累积 DOM、惰性 HTML 文本、迟到响应隔离、失败重试与旧记录提示 |
| Session projection/window | `session/` | 稳定条目合并与动态高度虚拟窗口；ResizeObserver 测量、阅读锚点补偿、会话卸载清理 |
| Message recovery | `session/message-replay.ts` | 初次打开与运行期游标补页、固定快照高水位、去重和中断终态；失败可见并自动重试 |
| Startup selection tests | `components/startup-gate.test.tsx` | 工作树选择准确传递 cwd，缺失/bare/prunable 目录禁用 |
| Engine Inspector | `components/engine-inspector.tsx` | 稳定 durable snapshot 的运行/轮次/LLM/Tool/Middleware/raw record 视图、实时刷新和递归 Agent 页栈 |
| Session details | `components/session-details.tsx` | 会话记忆、分支、尝试、handoff envelope 与引擎诊断 |
| Engine readiness | `components/engine-readiness-panel.tsx` | 引擎健康、能力、修复计划与官方指引入口 |
| Fallback policy | `components/fallback-policy-panel.tsx` | 有序 fallback 目标、模型/provider 设置与自动使用授权 |
| Team panel | `components/team-panel.tsx` | Worker 生命周期、任务队列、worktree adoption、资源声明与自治开关 |
| Component tests | `components/components.test.tsx` | Composer、模型选择器、shared-clock timeline 与 Inspector 子 Agent 回退回归 |
| Timeline tests | `components/hub-timeline.test.tsx` | 审批卡工具名/资源/效果说明同屏与多语言保留回归（R76） |
| Product pages | `components/product-pages.tsx` | 自动化 re-export、真实内置插件/MCP、项目 Skills 与设置/搜索；外观/Inspector 接真实 callback，缺能力明确不可用 |
| ALVA tool settings | `components/tool-settings.tsx` / `components/tool-settings.test.tsx` | 真实 capability 开关、MCP 禁用优先 CRUD、授权测试、revision/失败草稿与迟到回复隔离；Extensions 与 Settings 共用 |
| Workbench tools | `components/workbench-tools.tsx` | Files、Text、Git、Review、Agent、产品 Attempt + native Inspector、Tasks、Usage、Chrome、Terminal 标签内容 |
| Workspace files | `components/workspace-files.tsx` | Files/Text 真实只读目录导航与 UTF-8 预览；会话切换 key 隔离迟到响应，刷新重试、二进制/受限/限额明确提示 |
| Workspace Git | `components/workspace-git.tsx` | 真实 Git 分支/status/diff，三种变更区分、任务终态快照刷新与手动刷新；Review 继续展示执行证据，非 Git 状态 |
| Native terminal | `components/workspace-terminal.tsx` | 显式 Open 后才创建 PTY，输入/Ctrl-C/resize/退出/关闭；按会话 key 卸载清理且迟到 open 也关闭，HTTP 明示不支持 |
| User browser | `components/workspace-browser.tsx` | 显式打开独立临时原生窗口，地址/导航/刷新/显示/关闭控件；轮询窗口事实，不伪造 Agent 自动化、iframe 或加载成功 |
| Workspace Skills | `components/workspace-skills.tsx` / `components/workspace-skills.test.tsx` | 真实工作区技能列表/搜索、源文档创建/编辑、冲突/失败/只读状态、归档确认/恢复与会话隔离；不联网安装样例 |
| Local automations | `components/automations.tsx` / `components/automations.test.tsx` | 真实 interval CRUD/状态轮询、暂停保存、计费 review、Pause/删除确认与 session 导航；不伪造 daemon/cron 或即时执行 |
| UI Lab | `ui-lab.tsx` | 开发/test-only 确定性组件/页面与真实只读媒体组件展台；HTTP fixtures 仅测试拦截 |
| Visual config | `../playwright.config.ts` | 1440×900 Chrome 截图环境与 Vite 生命周期 |
| Visual tests | `../tests/visual.spec.ts` | 七张总层架和模型弹层的真实浏览器回归 |
| Domain styles | `styles/hub-components.css` | Hub 领域组件组合样式，不重复基础控件行为 |
| App styles | `styles/app.css` | 页面布局、业务状态与响应式胶水，不定义可复用基础控件 |
