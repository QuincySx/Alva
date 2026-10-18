# Cindy Desktop 0.1.45 — UX/UI 产品规格

> 规格版本：1.0；审计日期：2026-08-13；目标版本：Cindy 0.1.45。本文是产品与验收规格，不是对所有分支都已实机复现的声明。证据状态与逐流程截图映射见 [03-user-flows-and-state-matrix.md](/Users/smallraw/.cache/cindy-deep-dive/product-docs/03-user-flows-and-state-matrix.md)。

## 1. 范围、证据与规范词

本规格合并四类证据：0.1.45 的 43 个只读实机状态（`LIVE`）、0.1.44 的 67 图参考集（`BASELINE`）、0.1.44→0.1.45 代码差异与当前源码清单（`SOURCE`）、需要账号、凭据、运行中事件、破坏性操作或故障注入才能进入的分支（`GATED`）。实现发生冲突时，优先级为当前源码与 0.1.45 实机行为，其次是现行设计规范，最后才是旧基线。

规范词：**必须**表示验收硬条件；**应**表示无特殊理由不得偏离；**可以**表示可选实现。任何只由 `SOURCE` 支持的 UI 都必须在测试报告中保留证据标签，不得伪称为 `LIVE`。

### 1.1 产品心智模型

Cindy 是“会话主线 + 上下文导航 + 可组合工作台”，不是仪表盘集合：

- 左栏回答“我在哪个项目/任务”。
- 中栏回答“人与 Agent 发生了什么、下一步输入什么”。
- 右栏回答“为当前任务打开哪些工具”。
- 辅助窗口让会话或工具离开主窗口，但不复制业务真值。
- 设置、自动化、插件与 SkillHub 是管理面；进入这些页面时不应无条件初始化沉重的会话工作台。

### 1.2 全局不变量

1. `chat-main` 必须且只能存在一个，最小宽度 400 px，不可折叠或关闭。
2. 新任务在第一次有效提交前只是本地草稿；切换目标、模型或权限不得创建空后端会话。
3. 布局树是全局结构，右栏折叠与标签内容按会话分桶；未知插件面板隐藏并保留槽位，不得摧毁整个布局。
4. Renderer 不持有凭据、高权限文件/进程/SQLite/更新能力；敏感动作经 preload 白名单与主进程复验。
5. 权限请求、AskUserQuestion、计划评审在没有 resolver 时一律 fail closed。
6. 删除、退出、终止进程、结束 Orca 协作、回退代码、覆盖脏文件等不可逆或高影响动作必须明确确认，并说明影响范围与恢复能力。
7. 任何局部 tab/媒体/路由错误都应局部隔离；不得因为单个工具崩溃清空聊天或导航。

## 2. 信息架构与全局壳

### 2.1 主窗口 Global Shell

| 字段 | 规格 |
|---|---|
| 目的 | 在一个持久窗口中协调导航、主聊天和工具工作台，并承载全局 banner、查找、toast、lightbox 与系统 chrome。 |
| 入口 | 启动后通过认证/本地模式与 `LocalDbGate`；主路由 `/` 恢复最近有效页面。 |
| 布局 | 默认 1280×800，最小 800×600；左侧 `session-list` 全高；右侧为递归 `LayoutRoot`，默认 `chat-main 0.5` 与 `right-tabs 0.5`。布局最大深度 8，子节点 fraction 不低于 0.05。 |
| 控件 | 平台窗口控制、全局菜单、侧栏折叠、查找、更新提示、toast；分隔条可拖动；右栏可折叠、最大化、分离。 |
| 状态 | splash、认证门、DB 加载、常规页面、section error、top-level error、全局 modal/lightbox、窗口失焦/恢复。 |
| 规则 | 路由按 `dataOwnerId:dataOwnerRecoveryEpoch` 重建；设置/市场页隐藏会话左栏或工作台；非法布局只回退坏节点或默认树。 |
| 危险动作 | 关闭窗口可能退出或缩至托盘；重置布局会覆盖当前布局偏好。关闭行为必须遵循平台与用户设置。 |
| 验收 | 800×600 无关键控件不可达；主聊天不可被拖没；单个右栏崩溃不影响聊天；账号切换后不残留上个 owner 的页面状态；恢复窗口位置不越出当前屏幕。 |

### 2.2 导航与左侧栏

| 字段 | 规格 |
|---|---|
| 目的 | 提供新建、项目/任务定位、运行态识别，以及自动化、SkillHub、插件、设置、更新日志等全局入口。 |
| 入口 | 主壳左侧固定区；`Mod+B` 折叠/展开；`Mod+1…9` 切换前九个会话。 |
| 布局 | 顶部新建与机器/远程上下文，中部项目和任务树，底部全局功能与版本/下载入口；滚动区与固定区分离。 |
| 控件 | 新建、整理/筛选、项目展开、任务选中、hover 更多、全局搜索、机器切换、自动化、插件/技能、设置。 |
| 状态 | 展开/折叠、项目展开/收起、任务 selected/hover/running/waiting/error/remote/worktree/collaboration、搜索过滤为空、拖动预览。 |
| 规则 | 默认保持安静，低频管理动作藏在 hover/右键菜单；选中态与运行态必须可并存辨认；拖出任务可创建副窗口。 |
| 危险动作 | 项目移除/归档、任务归档/删除；必须二次确认或提供明确的归档恢复路径。 |
| 验收 | 键盘可到达所有顶级入口；项目/任务层级和选中态在 Light/Dark 均清楚；过滤后不得改变原排序或业务数据；拖动预览不截获鼠标。 |

### 2.3 路由与页面边界

顶级路由包括 `/cc-agent`、`/skillhub/local`、`/skillhub/market`、`/plugins`、`/settings`、`/issues` 与诊断/实验页面。旧 billing、SkillHub 详情和设置 key 必须兼容重定向；未知设置 tab 回到 `general`。路由错误分两层：应用级全屏恢复页与保留导航 chrome 的 section 恢复页。

验收：后退/前进不生成空会话；旧深链不落到开发者错误页；切 owner 后旧历史栈不可穿透；辅助窗口只加载自身最小模块图。

## 3. 会话核心 Surface

### 3.1 任务时间线

| 字段 | 规格 |
|---|---|
| 目的 | 以时间顺序呈现用户消息、Agent 文本/思考、工具活动、文件变化、计划、人机交互、错误与终态。 |
| 入口 | 选中已有任务；副窗口可直接 boot 到普通任务或 Orca lead/worker。 |
| 布局 | 标题区 + 可滚动消息流 + 固定 Composer/状态栏；工具活动默认紧凑折叠，展开后显示输入、输出与进度；底部保留自动滚动/回到底部反馈。 |
| 控件 | 消息复制/分享、活动展开、文件 Review/Undo、计划 chip、问题导航、重试/继续、中断、标题更多菜单。 |
| 状态 | idle、streaming、thinking、tool running/success/error、waiting permission/question/plan、queued、compacting、aborting、done、recoverable/terminal error。 |
| 规则 | 事件顺序以宿主流为真值；切任务不得重播既有 streamed-word 动效；0.1.45 默认开启流式淡入；工具结果与计划失败必须对账到稳定终态。 |
| 危险动作 | 中断运行、Undo/rewind 文件更改、删除任务；必须显示作用域，回退前提供预览且不得在 Git savepoint 不可用时承诺可恢复。 |
| 验收 | 长聊天可虚拟化且不跳动；等待交互时明确停在哪一步；terminal error 不显示为仍运行；问题导航能定位上一问题；文件卡路径、数量与 Review/Undo 一致。 |

#### 时间线内容类型

- 用户/Agent 文本、thinking、image/video、Markdown、Mermaid、3D/model、纯文本或 tool payload lightbox。
- tool use/result/full result、后台任务、Subagent 更新、Orca worker 回传。
- permission、AskUserQuestion、plan review、plan progress、queued input、compact、status、done/error。
- generated/changed files、turn diff、usage/cost、retry/continuation 与自动恢复标记。

### 3.2 Composer

| 字段 | 规格 |
|---|---|
| 目的 | 收集用户意图与上下文，并在提交前明确目标、Agent、模型、effort/Fast、权限和计划/协作模式。 |
| 入口 | 新任务草稿或已有任务底部；插件可带预填 mention 打开草稿。 |
| 布局 | 多行编辑器为主；左/下方为附件与模式入口，右/下方为权限、Agent/模型、语音和发送；pending queue/pinned plan 贴近输入区。 |
| 控件 | 输入、发送、停止、附件、目录、额外只读目录、slash、`@mention`、Goal、Plan、Collaboration、插件、权限、Agent、模型、effort、Fast、语音。 |
| 状态 | empty、focused、typed、IME composing、palette open、attachment resolving、busy/queueing、sending、disabled、error。 |
| 规则 | 默认 Enter 发送、Shift+Enter 换行，可设置反转；IME composing 禁止误发；忙时输入进入可持久队列；slash/mention 支持方向键、Enter、Esc；只读目录不升级写权限。 |
| 危险动作 | Full access、上传文件/媒体、发送会造成外部模型传输；选择时必须有清晰标签，权限升级不可由预填内容暗中完成。 |
| 验收 | 空输入禁用发送；切换任何草稿选项不创建 session；粘贴/附件失败保留文本；队列 stop 后保留并标记 paused；键盘与屏幕阅读器均可操作 palette。 |

### 3.3 新任务草稿

| 字段 | 规格 |
|---|---|
| 目的 | 用最低认知成本开始普通对话、项目任务、远程/worktree 任务或协作任务。 |
| 入口 | 左栏 New Maker、`Mod+N`、File > New Maker、插件启动或合法深链 `/cc-agent/new`。 |
| 布局 | 空态标题/提示 + 目标选择器 + Composer；高级配置不应在首屏铺开。 |
| 控件 | Conversation/项目、其他目录、远程主机、worktree、Agent/vendor、provider/model、effort/Fast、权限、Goal/Plan/Collaboration。 |
| 状态 | transient draft、目标菜单、目录选择、remote gate、worktree creating、first-submit creating、creation error、已转会话。 |
| 规则 | delayed-create；首条提交是唯一创建边界；0.1.45 当前可见目标没有手机设备；review session 不可作为 automation target。 |
| 危险动作 | 连接远程、选择 Full access、创建 worktree；必须在提交前显示上下文和权限。 |
| 验收 | 无消息离开草稿不留下空任务；首发失败可原地重试且不重复创建；无效/丢失目录给出修复入口；项目和 Conversation 选择可撤销。 |

### 3.4 Goal

| 字段 | 规格 |
|---|---|
| 目的 | 将开放目标转换为具有限额与无进展停止条件的长程任务。 |
| 入口 | Composer 操作菜单 > New Goal。 |
| 布局 | 目标描述为主，高级区按需展开；Start 为明确主按钮。 |
| 控件 | 目标文本、最大 turns、token budget、连续无进展 turns、取消、Start。 |
| 状态 | empty/Start disabled、configured、starting、running、budget/turn/no-progress exhausted、completed/failed/cancelled。 |
| 规则 | 限额必须在启动前冻结并可读；空描述不可启动；Goal 不得隐式等同 Full access。 |
| 危险动作 | 长时间/高 token 运行；启动前显示资源边界，运行中允许安全停止。 |
| 验收 | 所有数值有范围与错误文案；触发任何停止条件时显示具体原因；取消不丢失未提交草稿。 |

### 3.5 项目与任务管理

| 字段 | 规格 |
|---|---|
| 目的 | 将会话按工作目录/项目组织，并提供低频管理、分享、导出与窗口化能力。 |
| 入口 | 左栏项目/任务 hover 或右键；任务标题更多菜单。 |
| 布局 | 紧凑上下文菜单；危险项置底并与安全项分隔；标题菜单与左栏菜单动作语义一致。 |
| 控件 | 重命名、置顶、移动、项目搜索/文件、复制链接、打开新窗口、导出、分享、归档、删除；项目还有同步/移除。 |
| 状态 | normal/selected/pinned/running/error、menu open、rename、move picker、share selection、archive/delete confirm、drag preview。 |
| 规则 | 0.1.45 任务菜单不得出现“分栏打开”；保留“新窗口打开”；拖出窗口与显式菜单得到等价会话；复制链接不更改任务。 |
| 危险动作 | 项目移除、任务归档/删除、分享/导出外传；分别确认本地影响、可恢复性和输出范围。 |
| 验收 | 菜单动作对当前目标生效；移动后选中态稳定；分享可选单条/多条/全选长聊天；取消确认零副作用；拖动取消不创建副窗。 |

## 4. 右侧工作台

### 4.1 工作台壳

| 字段 | 规格 |
|---|---|
| 目的 | 让文件、审查、运行监控和辅助工具与聊天并排，同时保持每个标签独立生命周期。 |
| 入口 | 任务页展开按钮、Add Tab、从文件/工具事件自动聚焦；可分离到独立窗口。 |
| 布局 | tab strip + active body + 新增/更多/最大化/分离；空 tab 与无 session 使用不同空态。 |
| 控件 | 激活、关闭、拖动重排、新增、最大化、detach/merge；`Mod+W` 在工作台焦点时优先关闭 tab。 |
| 状态 | collapsed、open、empty、loading、active、tab error、maximized、detached、rehydrating。 |
| 规则 | 状态按会话持久化；插件拥有 hydrate/serialize/default 与关闭拦截；单 tab error boundary；未知 kind 隐藏但不毁布局。 |
| 危险动作 | 关闭 terminal 会 dispose PTY；关闭 Orca Workers 等于结束协作，必须拦截并 fail closed。 |
| 验收 | 切任务恢复各自 tab；重排稳定；分离后 tab 和资源数据预热恢复；merge 不重复 tab；局部崩溃仅重试当前 tab。 |

### 4.2 九类 Tab 规格

这里的“九类”按当前源码内置 `TabKindPlugin` 注册表计算。`resource-usage` 是九类之一，但仅用于恢复旧持久化 tab，不再出现在新建菜单；`ghost:<id>` 属于布局树的运行时插件 panel，不计入这九类。

| Tab kind | 目的与入口 | 布局与控件 | 状态与规则 | 危险动作 | 验收 |
|---|---|---|---|---|---|
| `file-browser` | 浏览当前 local/SSH/device 工作区；Add Tab、文件事件或 reveal。 | 文件树 + 预览/编辑，可多开；展开、搜索、选中、保存、新建、重命名。 | loading/empty/selected/dirty/unsupported/binary/large/error；2 MiB 读取上限与路径/软链边界由主进程执行。 | 保存、覆盖、创建、重命名、删除；脏文件切换必须确认。 | 不能逃逸工作区；取消切换保留 dirty；远程失败可重试/显示 stale cache；`Mod+S` 只作用当前文档。 |
| `review` | 审查当前任务/轮次的代码变化；Add Tab 或文件变化卡 Review。 | 文件树 + diff；unified/split、wrap、word diff、ignore whitespace、Markdown 预览、branch base。 | loading/no changes/git unavailable/diff error/image/markdown fallback；单例。 | 接受后触发 Undo/rewind 属时间线流程，不在 Review 中暗中执行。 | 切视图不改变 diff；缺 Git 显式降级；文件计数与时间线卡一致；大 diff 保持滚动与定位。 |
| `subagents` | 展示当前会话子 Agent 及运行/变更 fence；Add Tab 或 runtime event。 | 列表/树、状态、聚焦与详情；单例。 | empty/running/waiting/done/error/restoring；0.1.45 已恢复聊天流交互，但空任务只见空态。 | 取消子 Agent 或权限回应；必须针对正确 session/instance。 | 空态不伪装加载；事件恢复后不丢项；切任务不串数据；终态可追溯到父任务。 |
| `background-tasks` | 监控会话后台任务与 workflow；Add Tab 或后台事件。 | 任务列表 + `WorkflowProgressTree` + focus detail；单例。 | empty/queued/running/waiting/completed/failed/cancelled。 | 取消任务；不得误取消前台 turn。 | 等待交互/后台任务时 45 分钟 no-event watchdog 不误报；失败定位具体节点；恢复后顺序一致。 |
| `orca-workers` | 管理 Orca team/worker；由 Orca 程序创建，菜单隐藏。 | Worker 列表、角色、focused worker、创建/批量、消息与结束协作；单例。 | no team/active/worker idle-running-done-error/team completed-cancelled-failed。 | 关闭=结束 team；Full Access 提升、archive/cancel worker 均需确认或明确权限。 | 非 Orca 会话不可出现；无关闭 interceptor 必须拒绝；每 Lead 仅一个 active team/一个 focused worker；当前 LIVE 未见，按 SOURCE/GATED 验收。 |
| `web-browser` | 在任务内浏览并供 browser-control 使用；Add Tab。 | Chrome、omnibox、back/forward/reload、页面、favicon/title/audible、评论；默认 `about:blank`。 | loading/ready/navigation error/crashed/background reclaimed/pinned；持久 partition。 | 打开外站、提交表单、下载；必须遵循 webview 安全策略和用户确认边界。 | `Mod+L`、Alt+Left/Right、`Mod+R` 可用；crash 自清理；pinned automation 不被 watchdog 回收；外部导航来源可见。 |
| `ios-simulator` | 查看与操控 iOS Simulator 实例；平台/能力具备时 Add Tab。 | 实例网格 + 设备状态/控制；main 持有 actor，renderer 只消费 typed IPC。 | unavailable/no runtime/booting/ready/error/disconnected。 | 启停/擦除模拟器可能影响状态；擦除必须另行强确认。 | 非 macOS 或无 Apple tooling 显示明确不可用而非空白；当前 0.1.45 Add Tab 未出现，按 SOURCE/GATED 验收。 |
| `terminal` | 提供绑定工作区的 PTY；Add Tab 或 <kbd>Ctrl</kbd>+<kbd>&#96;</kbd>。 | xterm 区 + shell/重启/状态；单例或实现允许的会话实例。 | creating/ready/running/exited/restarting/error；重启后 hydrate 强制 `created=false`。 | 任意命令、关 tab dispose PTY；不得继承 secret-like 环境变量。 | create/attach 幂等；exit 保留可重启记录；owner 窗口销毁先转 fallback；关闭 tab 确认/明确终止语义。 |
| `resource-usage` | 兼容恢复旧会话保存的资源 tab；不从 Add Tab 新建。 | CPU/内存/PID 表、刷新、进程操作；单例。 | loading/populated/stale/error；新产品入口改为独立资源窗口。 | Terminate process；必须精确确认 PID/进程实例。 | 旧 tab 可 hydrate；新菜单不出现；关闭不影响 Agent；0.1.45 实机验证数据在分离窗口预热恢复。 |

运行时 `ghost:<id>` 面板可停靠或分离，使用标准布局 chrome，但由 Ghost 插件声明和持有生命周期。插件 unavailable/off/starting/running/crashed/fused/error 必须局部呈现；新增 network/fs/node/agent 权限须 review，未知或已卸载面板只隐藏槽位而不破坏布局。

## 5. 自动化

| 字段 | 规格 |
|---|---|
| 目的 | 创建、查看和运行定时或手动任务；支持 Agent 与受限、零 token 的 script runner。 |
| 入口 | 左栏 Automation；模板卡；已有 schedule 编辑或 Run now。 |
| 布局 | 列表/筛选/空态 + 创建编辑弹层；表单分为内容、runner、时间、目标会话/项目、route、通知与高级 hook。 |
| 控件 | 模板、AI/Script、automatic/manual、one-time/cron/interval、new/continuous/bound session、project/worktree、engine/model/effort/Fast、silent/Feishu/WeCom、pre-run hook、Create/Save/Run/Pause。 |
| 状态 | schedule active/paused/expired；run loading/claiming/persisting/queued/running/cancelling/finalizing/success/failed/aborted/interrupted/skipped；筛选空态。 |
| 规则 | queued 不占 Agent/MCP/token 并发；同 schedule 串行；`nextFireAt` CAS claim；旧 running 启动时收敛为 interrupted；script capability 默认拒绝；pre-hook exit 0=运行、2=skipped、其余 fail closed。 |
| 危险动作 | Run now、脚本能力、外部通知、删除 schedule；创建/保存前必须呈现目标、权限、外传渠道与下次运行时间。 |
| 验收 | cron/时区预览准确；目标忙 defer 时不生成虚假 run row；persistent target archived 后清绑/重建，非持久 target 则暂停；失败历史显示阶段、原因与可重试性。 |

## 6. 插件、SkillHub 与 MCP

### 6.1 Ghost 插件

| 字段 | 规格 |
|---|---|
| 目的 | 安装、启停、更新和配置扩展 Cindy 的工具、卡片、页面、面板和后台能力。 |
| 入口 | 左栏 Plugins、市场/推荐卡、Add Plugin、插件深链。 |
| 布局 | 已安装/推荐/来源 + 详情浮层；详情显示版本、状态、工具、权限、设置和启动入口。 |
| 控件 | 安装、更新、启用、停用、配置、创建插件、添加市场、更新全部、权限 review。 |
| 状态 | empty/installed/update available/installing/quarantine/rejected/off/starting/running/stopping/crashed/fused。 |
| 规则 | `ghost.json` v2 严格校验；安装按 owner/插件串行；字节、来源、provenance 在 commit 前重验；新增权限显式 review；60 秒三次 crash 进入 fused。 |
| 危险动作 | 安装代码、network/fs/node/agent 权限、Full Access、删除市场源；不得把删除来源等同删除本地插件。 |
| 验收 | 权限 diff 清楚；取消安装零残留；fused 不自动重启风暴；停用幂等；插件错误局部隔离；秘密只经 safeStorage binding。 |

### 6.2 SkillHub

| 字段 | 规格 |
|---|---|
| 目的 | 管理 global/project 的 skill、command、MCP 等本地能力，并提供市场发现、发布和审核。 |
| 入口 | `/skillhub/local`、`/skillhub/market`、左栏 Skills、插件或历史深链。 |
| 布局 | Local 按 scope/kind/engine 分组；Market 提供搜索、分类、排序和详情浮层；项目/全局详情使用稳定深链。 |
| 控件 | 扫描、导入、同步、安装、忽略、发布、编辑市场信息、可见性、审核、diff。 |
| 状态 | loading/empty/scanned/conflict/ignored/syncing/installing/published/review pending/rejected/error。 |
| 规则 | realpath 去重；bundled skill 链到 `~/.agents/skills`；自动同步只在登录后、每用户一次且受明确白名单；用户 ignore 与冲突项不得覆盖。 |
| 危险动作 | 安装会影响所有会话；发布/可见性会外传；必须明确 scope、来源与公开范围。 |
| 验收 | global/project 标签不可混；旧详情 URL 回退到市场列表而非报错；扫描结果可复核；冲突不静默覆盖；取消清理可重试。 |

### 6.3 自定义 MCP

| 字段 | 规格 |
|---|---|
| 目的 | 添加或编辑用户自定义 HTTP/SSE 工具服务。 |
| 入口 | Settings > 工具 > MCP Servers > Add/Edit。 |
| 布局 | 服务器名称、transport、URL、headers/auth 与验证/保存；敏感字段不回显完整值。 |
| 控件 | HTTP/SSE、URL、header、Bearer/token、Test、Save、Enable/Delete。 |
| 状态 | empty/editing/testing/success/auth required/error/saving/enabled/disabled。 |
| 规则 | 远程 URL 仅 HTTPS 或 loopback；ID 路径安全且不碰内置 ID；header 值仅字符串；token 存 safeStorage；变更后广播并按引擎生命周期生效。 |
| 危险动作 | 凭据、外部网络、删除配置；测试/保存必须由用户明确触发。 |
| 验收 | 非法 URL 与重复 ID 在提交前阻止；秘密不进入日志/截图；Claude 下个 session 生效、Pi 以 session-create 快照，UI 应说明何时生效。 |

## 7. 设置

### 7.1 设置壳

入口为左栏 Settings、macOS `⌘,`、应用菜单或深链 `/settings?tab=<key>`。布局是左侧 14 页签目录 + 右侧滚动内容；设置页隐藏会话侧栏。页签切换使用 150 ms 淡入并保留稳定 scrollbar gutter。未知/不可见 key 回 `general`；旧 key 兼容重定向。

### 7.2 十四个页签

| 页签 | 目的/入口 | 布局与控件 | 状态/规则 | 危险动作 | 验收 |
|---|---|---|---|---|---|
| 1. 通用 `general` | 账户与全局行为；Settings 默认页。 | Profile、Appearance、Language、Notification、Window Behavior、Composer Send、Collaboration、Agent Resource、Git Safety、Experimental、Logout。 | admin/registry 可隐藏实验项；平台无关项优先。 | 退出、删账号、Git 安全降级、关闭/托盘策略。 | 变更即时或明确需重启；退出/删号确认；平台不支持项隐藏而非可点失败。 |
| 2. 个性化 `personalization` | 定义提示、记忆与交互偏好。 | User Prompt、Memory、Subagent Model、Contacts、Compaction、Terminal Shell、Link Open、Stream Fade、Tips。 | 本地模式可用本机联系人；stream fade 0.1.45 默认开。 | 清记忆、导入联系人、外链策略。 | 长提示可编辑恢复；默认值/重置清楚；敏感联系人不泄露；reduced motion 可覆盖流式淡入。 |
| 3. 模型供应商 `providers` | 配置 provider/model/credential 与价格。 | Provider 列表/排序、余额、模型显隐、更多菜单、Add catalog、Custom endpoint、price override。 | loading balance、auth required、custom test/error/saved；`provider_id` 是权威 route。 | API key、删除 provider、切凭据、价格覆盖。 | 密钥不回显；测试失败不保存；模型与 provider 操作不串目标；余额 loading 有稳定占位。 |
| 4. 用量和计费 `billing` | 查看套餐、余额、消费和账单。 | Plan/balance、daily/model/session attribution、checkout/change plan。 | 仅账号能力允许时显示；actual 与 estimated 严格区分。 | 购买、变更套餐、外部支付。 | 金额币种与归因准确；subscription 不伪装实际成本；失败可返回且不重复订单。 |
| 5. 语音输入 `voice-input` | 配置 provider、语言、麦克风、快捷键、润色和词典。 | Section cards + permission/status。 | recording/transcribing/error；Linux 无全局快捷键，外部词典学习仅 macOS。 | 麦克风/辅助功能/Input Monitoring 权限、音频外传。 | 请求前解释权限；拒绝后可重试/打开系统设置；overlay 不遮挡词典 toast；平台文案准确。 |
| 6. IM 机器人 `im-bot` | 管理官方/个人 IM channel 与绑定。 | Personal WeChat、WeCom、Feishu、DingTalk、Discord、Telegram 分组/深链。 | disconnected/authorizing/waiting confirmation/connected/reconnecting/needs reauth/policy disabled/error。 | 授权、外部消息、解绑、联系人导入。 | 未连接显示 gate；授权不得自动开始；冲突有专用 dialog；凭据 owner-scoped；退出后旧 generation 任务失效。 |
| 7. 键盘快捷键 `shortcuts` | 查看与改绑可配置动作。 | 搜索/分组、录制、冲突提示、恢复默认。 | idle/recording/conflict/invalid/saved；系统惯例隐藏项仍生效。 | 无破坏性；冲突可能使关键入口不可达。 | 保留组合与平台校验；同 scope 冲突阻止保存；键盘可恢复默认。 |
| 8. 灵动岛 `agent-island` | 配置 macOS Agent Island。 | enable、screen position、skin preview、角色状态。 | idle/working/waitingApproval/completed；仅 macOS。 | 启动原生 helper、屏幕常驻。 | 非 macOS 隐藏；helper 异常不拖垮 app；预览与实际位置/皮肤一致。 |
| 9. 任务导入 `import` | 扫描并导入 Claude/Codex 历史。 | 固定高度 non-scroll 内容、扫描、选择、冲突/结果。 | idle/scanning/results/importing/success/partial/error。 | 写入会话 DB、重复导入。 | 固定 gutter 不抖动；冲突/去重可预览；取消不产生半条会话；partial 给出明细。 |
| 10. 远程连接 `remote-control` | 管理设备与 SSH 主机/密钥。 | My Devices、SSH list、Add Host、Key manager/agent/private key、host pinning。 | disconnected/testing/connected/auth error/host key mismatch/offline。 | 保存主机/密钥、接受 host key、连接远程。 | 不记录私钥；host key 首次/变化明确；测试失败不保存凭据；删除/断开确认作用域。 |
| 11. 工具 `builtin-tools` | 管理内置工具与 MCP。 | BuiltinToolsSection + McpServersSection。 | enabled/disabled/permission required/error；MCP 见 §6.3。 | 启用高权限工具、保存 MCP secret、删除服务器。 | 能力/作用域可见；变更生效时点明确；无 resolver 时权限拒绝。 |
| 12. 自动操作 `computer-use` | 安装与授权浏览器/电脑/Android 自动化。 | 工作目录上下文、安装状态、OS 权限引导、设备状态。 | not installed/installing/ready/permission missing/error/unavailable。 | OS 辅助功能/录屏/输入控制、设备操作。 | 不代用户接受系统权限；引导窗口不抢焦点背景层；拒绝后可安全退出；按平台隐藏无效项。 |
| 13. 帮助 `help` | 提供快速开始、推荐工作流与问答。 | Help sections + Assistant panel、Release Notes/Issues/CLI links。 | loading/ready/no result/error/offline。 | 外链和向帮助 Agent 发送问题。 | 外传前可见；离线保留静态帮助；辅助面板错误不影响设置。 |
| 14. 关于 `about` | 展示版本、更新、协议、日志与存储。 | Version、Check Update、Release Notes、licenses、logs/storage。 | idle/checking/up-to-date/downloading/ready/error。 | 应用更新、打开日志/存储、重启替换。 | 版本准确；更新状态可恢复；忙任务时不自动重启；日志入口不泄露 secret。 |

## 8. 搜索与查找

### 8.1 全局任务搜索

| 字段 | 规格 |
|---|---|
| 目的 | 在项目/任务历史中按文本、状态、时间和排序快速定位。 |
| 入口 | 左栏搜索；筛选/排序菜单；旧 `/search` 深链如有应兼容。 |
| 布局 | 内联搜索框 + 筛选/排序 popover + 结果列表，尽量保留左栏上下文。 |
| 控件 | query、clear、scope/status/time/sort、结果选中。 |
| 状态 | idle/typing/loading/results/no results/error/menu open。 |
| 规则 | 过滤不更改原任务顺序/数据；结果选中恢复任务路由；Esc 关闭 popover/搜索层。 |
| 危险动作 | 无；搜索内容仍属于本地隐私，不进入遥测正文。 |
| 验收 | 输入响应及时；空结果给清筛选入口；键盘遍历与高亮可读；清空后恢复原列表。 |

### 8.2 页内查找与项目搜索

`Mod+F` 打开全局 `FindInPageBar`；文档/项目上下文用 `Mod+Shift+F`；浏览器 `Mod+F` 遵循焦点路由。查找条必须有 query、上/下一个、计数、关闭与 no match；不可覆盖 Composer 焦点或误提交。Linux 也必须提供应用内入口。

## 9. 登录、迁移与更新

| Surface | 目的/入口 | 布局与控件 | 状态/规则 | 危险动作 | 验收 |
|---|---|---|---|---|---|
| Login | 建立账号会话或进入本地模式；signed-out `/login`。 | Brand stage + identifier、验证码/重发、第三方、SSO、账号选择/绑定、consent、skip login。 | identifier/code/preparing browser/waiting/result/loading/field-service error/back/cancel；区域决定手机号/邮箱；凭据留主进程。 | 登录、绑定、同意条款、外部浏览器 handoff。 | skip login 与认证正交；返回不丢可保留字段；验证码倒计时准确；错误不暴露账号存在性；首次亮色门稳定。 |
| Legacy migration | 将旧数据安全迁到 owner-scoped DB；启动 gate。 | 说明、可迁项目/进度、重试/退出；必要时 backup 提示。 | detecting/backup/migrating/success/partial/fatal；无遗留数据跳过。 | 数据转换、覆盖/降级。 | 迁移前备份；失败保持旧数据；不可继续时给日志/恢复路径；当前实机未强制触发。 |
| In-app update notice | 解释版本变化；升级后首次或 Release Notes。 | 版本标题、变更分组、关闭/了解更多。 | unseen/visible/dismissed。 | 外链；无系统改变。 | 每版本按规则出现一次；关闭恢复原焦点；0.1.45 实机可见。 |
| Update service | 检查/下载/准备/重启。 | About/菜单状态、banner/progress、ready CTA。 | idle/checking/downloading/ready/superseding/error；保留旧 ready patch 直到新 patch 成功。 | 下载、重启替换。 | 进度单调；忙任务/锁屏条件不满足不自动重启；三次 apply 失败放弃并可诊断。 |
| Windows updater | 提升权限并替换包。 | 480×220 always-on-top：阶段、progress、error、log、终态 Close。 | waiting/authorized/backup/unzip/replace/launch/rollback/done/failed。 | elevation、文件替换、回滚。 | failed 隐藏进度、显示错误与关闭；日志始终可用；rollback 后能启动旧版。 |

## 10. 辅助窗口与跨窗口

| 窗口 | 目的/入口 | 尺寸/布局 | 状态与规则 | 危险动作 | 验收 |
|---|---|---|---|---|---|
| 会话副窗口 | 独立查看普通/Orca 会话；标题菜单或拖出。 | 继承主窗或 1280×800，min 800×600；`SecondaryWindowBootGate` replace 到目标。 | booting/ready/missing session/error；共享业务 session，不复制真值。 | 关闭窗不等于删除任务。 | 正确恢复目标；拖取消不创建；关闭后主窗任务仍在。 |
| 分离右栏 | 独立放置工作台；Detach。 | 520×860，min 360×480；50 px chrome。 | loading/rehydrating/ready/error/merge；状态单独保存。 | 关 terminal/Orca tab 仍遵守拦截。 | 资源用量预热、tab 顺序一致；merge 不丢状态。 |
| 分离 Ghost | 独立插件面板。 | 480×720，min 320×400；46 px chrome。 | plugin unavailable/running/crashed/fused。 | 插件权限与外部行为。 | 每 ghost 独立状态文件；卸载后清楚不可用；主壳不受 crash 影响。 |
| 资源用量 | 独立监控 CPU/内存。 | 580×520，min 380×320；表格/刷新/终止。 | loading/populated/stale/error。 | Terminate process。 | PID/名称/CPU/内存对齐；终止确认精确目标；关闭不影响 Agent。 |
| 语音输入 | 全局录音/转写。 | 透明无框，约束在 workArea；词典 toast 独立。 | idle/listening/transcribing/cancel/error。 | 麦克风、外部转写。 | Windows 隐藏后重建不丢鼠标命中；录音时抑制词典 toast；Esc 可取消。 |
| 电脑权限引导 | 解释 OS 权限并定位系统操作。 | 全屏不可聚焦背景 + 可移动可交互 guide。 | step/permission missing/granted/denied/error。 | 打开系统设置；不得替用户接受。 | 背景不拦截正常恢复；拒绝可退出；多屏位置正确。 |
| Agent Island | 常驻显示 Agent 状态。 | macOS SwiftUI pill/expanded。 | idle/working/waitingApproval/completed/error。 | 原生 helper 常驻。 | JSONL 断开优雅降级；reduced motion；非 mac 隐藏。 |
| Session drag preview | 提供拖出会话反馈。 | 320×68 无框、透明、不可交互。 | dragging/accepted/cancelled。 | 释放创建副窗。 | 不接收点击；跟随光标不过屏；只在合法 drop 创建。 |

## 11. 媒体、空态、加载态与错误态

### 11.1 媒体与 Lightbox

| 类型 | 入口与控件 | 状态/规则 | 验收 |
|---|---|---|---|
| Image/Video | 消息缩略图；打开、缩放、下载/复制、关闭。 | loading/ready/error；外传/保存需用户动作。 | 保持宽高比；Esc 关闭并归还焦点；坏资源不撑破时间线。 |
| Model/3D | 生成文件卡或消息；旋转/缩放/重置。 | loading/ready/unsupported/error。 | 无 WebGL/格式不支持有 fallback；操作有键盘替代或文本说明。 |
| Mermaid | Markdown 图表；预览/源码/放大。 | parsing/rendering/ready/error。 | 不执行不可信脚本；错误显示源码或诊断，不丢消息正文。 |
| Text/Tool payload | 长文本或工具输入输出；复制、wrap、搜索。 | truncated/full/loading/error。 | 明确截断；请求完整内容失败仍保留摘要；敏感字段遮罩。 |
| Share image | 消息菜单进入选择；单选/多选/全选、预览、下载/复制。 | selecting/rendering/ready/error/cancelled。 | 选择数准确，长聊天可渲染；取消不输出文件；外传只在明确下载/复制。 |

### 11.2 通用状态语法

| 状态 | 必须显示 | 禁止 | 恢复 |
|---|---|---|---|
| Empty | 发生原因、下一步主动作，必要时辅助说明。 | 把“无数据”画成永久 loading。 | 创建/刷新/清筛选/选择上下文。 |
| Loading | 对象范围、稳定占位；超过感知阈值显示文字。 | 反复布局跳动；不确定进度伪装百分比。 | 可取消时给 Cancel；失败转 error。 |
| Success | 明确完成对象；短操作 toast，关键操作持久结果。 | 永久占位庆祝动画。 | 下一步/查看结果。 |
| Recoverable error | 人话原因、保留输入、Retry 和替代路径。 | 只给错误码；吞掉用户草稿。 | 重试/返回/打开设置/日志。 |
| Terminal/fatal | 影响范围、数据安全状态、不可继续原因、诊断与退出。 | 自动循环重试或把 fatal 当空态。 | 恢复备份、重启、导出日志或联系客服。 |
| Permission gate | 请求者、能力、目标、时限与允许范围。 | 无 resolver 自动放行；模糊“继续”。 | Allow once/按策略允许/Deny；拒绝后 Agent 可继续解释。 |

数据库损坏按 `.bak.clean`→最新迁移备份→fatal 的顺序恢复；路由错误优先局部重试；tab crash 仅重建 tab；provider/network 错误保留输入与明确重试；自动恢复不得重复计费并应显示标记。

## 12. 视觉系统

### 12.1 设计 Token

- 颜色必须来自 `ColorRegistry/registerColor` 的语义 token，组件禁止裸 hex；实验页是已登记债务而非先例。
- 字体：UI 为 Inter，代码与终端为 JetBrains Mono；数字表格应使用等宽数字特性。
- 圆角：8 px 内层 control，12 px 容器/card，9999 px pill；层级由 surface/border/selected state 建立，常规 Card/Board 默认无阴影。
- 间距应以 4 px 基线形成 4/8/12/16/24/32 序列；紧凑菜单允许 6/10 的光学修正，但必须 token 化。
- 边框/焦点/危险/成功/等待/错误必须使用语义角色，不得只靠 hue 区分。
- 图标与文本的可点击区桌面最小 28×28 px，关键/触摸兼容控件应达到 36×36 px；图标按钮必须有 tooltip 与 accessible name。

### 12.2 Light / Dark 与主题

内置 Default、Cindy、Atom One、Solarized、Eclipse、One Dark Pro、GitHub Dark、Monokai、Material Ocean HC 等主题通过 `ThemeService` 注入 `theme-vars`，并同步 `.dark`、`data-theme`、`colorScheme`。Cindy Light 为暖象牙，Cindy Dark 为中性近黑；macOS 可用 vibrancy，Windows 可用 backgroundMaterial。

验收：正文/小字/禁用/边框/焦点/选中/错误满足 WCAG 2.2 AA 对比；透明材质关闭时仍有实体背景；系统主题切换不闪白；自定义主题缺 token 时回退语义默认而非透明；编辑器、Mermaid、终端与 webview 同步合理明暗。

### 12.3 Motion

通用 duration 为 80/150/200/250 ms；设置 tab 150 ms fade，Composer morph 220 ms，更新器 progress 200 ms。允许 spinner、marquee、confetti、summon seal、streamed-word fade、品牌 sheen 等目的性例外。

规则：motion 必须解释状态变化而不是装饰；切任务不重播历史 stream fade；计划执行中的呼吸点只绑定当前 running step，失败 reconciliation 后停止；progress 从 100 回 0 时暂时移除 transition；`prefers-reduced-motion` 关闭/压缩非必要动画和庆祝，保留必要状态瞬变。

## 13. 响应式、窗口与平台

### 13.1 响应式/窗口规则

- 主窗 ≥1180 px：左栏、聊天、工作台可并排；800–1179 px：优先收窄或折叠右栏，再折叠左栏；不得让 chat-main 小于 400 px。
- 主窗最小 800×600；弹层必须限制在 viewport 内，长内容内部滚动，主按钮保持可达。
- 递归 split 拖动必须显示最小值阻尼；fraction 持久化并在显示器变化后归一化。
- 分离/合并保留 active tab、scroll/selection 和会话归属；不同 DPI、屏幕移除后窗口回到可见 workArea。
- 50%/200% 缩放、长中文/英文文案、超长项目名、RTL 文本内容不得遮住危险确认或发送按钮。

### 13.2 macOS / Windows / Linux

| 范围 | macOS | Windows | Linux |
|---|---|---|---|
| Chrome/menu | 原生 application menu、traffic lights、vibrancy。 | 自绘 WindowControls、应用内菜单、backgroundMaterial。 | 应用内菜单，无 mac application menu。 |
| Close | `Cmd+W` 先按焦点关 RSB tab，否则 close/hide。 | `Ctrl+W` 不关主窗；用户可选 close-to-tray，tray Show/Quit。 | `Ctrl+W` 不关主窗；系统 Alt+F4。 |
| Voice | Alt+Space；Input Monitoring/Accessibility；外部词典。 | Ctrl+Shift+Space；overlay 每次重建。 | 无全局快捷键设置，不支持系统音频 mute。 |
| Native extras | Agent Island、Apple tooling 的 iOS Simulator。 | 独立 updater/elevation/rollback。 | Linux 安装信号路径；无 Windows updater UI。 |
| Text menu | Copy/Add to chat/Look Up。 | Copy/Add to chat/Bing。 | 不安装该自定义原生菜单。 |
| Zoom | mac 原生 View role。 | Ctrl +/-/0，含 numpad。 | Ctrl +/-/0，含 numpad。 |

验收不得以“源码注册”为平台可用证据：例如 iOS Simulator 在非 macOS 必须显示不可用或完全隐藏；Agent Island tab 只在 macOS；Linux 语音设置必须准确说明限制。

## 14. 无障碍

1. 所有图标按钮、tab、菜单、状态点和图表必须有可读名称；状态不能只靠颜色或动画。
2. 完整键盘路径覆盖：打开/折叠导航、会话切换、Composer、palette、菜单、modal、tab 切换/关闭、查找和 lightbox；焦点环始终可见。
3. Modal 打开时焦点进入首个有意义元素并形成 focus trap；关闭后返回触发器；toast 通过适当 live region 宣告但不抢焦点。
4. streaming 文本不应逐词触发屏幕阅读器；以合适批次或终态宣告。permission/question/plan 必须及时但不重复宣告。
5. 200% 缩放和系统大字体下允许纵向滚动；不得裁切按钮。图表、资源表和 diff 提供文本/表格等价信息。
6. reduced motion、高对比、Increase Contrast/Reduce Transparency 应有稳健回退；透明窗必须仍有边界可辨识。
7. 错误字段用文本、图标和 `aria-describedby` 关联；首次错误聚焦或汇总，避免只显示 toast。
8. 拖放会话、tab 重排、文件 drop 必须有键盘等价动作；拖动结果由 live region 宣告。

## 15. 文案原则

- 用对象 + 动作命名：`打开新窗口`、`结束协作`、`终止进程 1234`，避免只有“确定/继续”。
- 区分状态与动作：`正在等待权限`不是按钮；按钮写 `允许一次`、`拒绝`。
- 危险确认说明“将发生什么、影响谁、能否恢复”；最危险动作使用具体红色动词，Cancel 为安全默认。
- 不夸大可靠性：estimated cost 写“估算”，未验证连接写“尚未测试”，SOURCE/GATED 状态不写“已支持并可用”。
- 错误文案包含原因、保留了什么、用户下一步；内部 error code 可折叠复制，不替代人话。
- 权限文案指出请求者、资源、持续时间与 scope；“Full access”必须说明这是 Cindy/Agent 策略而非 OS sandbox 解除的万能承诺。
- 中英文术语保持产品词典一致：Task/任务、Project/项目、Subagent、Worker、Goal、Plan、Workbench；同一屏不混用“会话/任务”指同一对象。
- 计费、删除、分享、外部发送、登录与更新的主按钮不得用诱导性弱化词。

## 16. 发布级验收清单

- 43 个 0.1.45 LIVE 状态与对应截图/线稿一一映射；每个新页面有 empty/loading/error/permission 分支策略。
- 0.1.45 差异：任务菜单无分栏、资源窗口预热、新任务无手机目标、分享长聊天、默认问题导航均回归；stream fade、切任务不重播、计划呼吸点/失败对账以动态测试补齐。
- 九类工作台 tab 和兼容 resource kind 均有平台/上下文 gating、关闭语义与错误隔离测试。
- 14 设置页签均可深链、键盘到达、平台条件正确，秘密不出 renderer/log/screenshot。
- 登录/skip、迁移、更新、DB fatal、crash、provider error 使用 disposable fixture 或 mock 驱动，不以生产账号/数据强制制造。
- 所有危险动作都测试取消零副作用；所有异步提交都测试重复点击、窗口关闭、断网、重启与 owner 切换。
- Light/Dark、reduced motion、200% zoom、键盘 only、屏幕阅读器基本路径，以及 macOS/Windows/Linux 差异通过验收。
