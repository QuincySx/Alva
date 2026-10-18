# Cindy Desktop UI 源码清单

> 审计对象：`makecindy/cindy`，本地浅克隆 commit `240247e52e938710f409c921a7a393097dcef52b`。
> 范围：`apps/desktop`、直接影响桌面展示/交互的 `packages/*`、仓库内桌面设计规范。
> 口径：这是源码实现审计，不是二进制逆向；不读取凭据、用户数据或运行中应用状态。

## 1. 一句话结论

Cindy Desktop 是 Electron 41 + React 19 + TypeScript 的多窗口本地客户端。主 UI 不是固定三栏，而是“树外会话栏 + 递归内容布局树 + 会话级工具标签页 + 可分离辅助窗口”；页面层承载新建/执行会话、Orca 多智能体协作、项目文件、自动化、SkillHub、插件、设置和登录，主进程承载 SQLite、进程/终端、浏览器安全、文件系统、插件沙箱、更新与原生辅助窗口。

## 2. 源码目录与职责

| 路径 | 用途 | 关键入口/导出 |
|---|---|---|
| `apps/desktop/src/main/` | Electron 主进程；窗口、菜单、IPC、SQLite、PTY、Agent/插件/调度、更新、系统能力 | `bootstrap-electron.ts`，各 `*/window.ts` |
| `apps/desktop/src/preload/` | 受控 renderer ↔ main 桥；向 `window.electronAPI` 暴露白名单能力 | preload entry 与 domain bridges |
| `apps/desktop/src/renderer/` | React UI、路由、布局、页面、状态、主题、右侧栏插件 | `index.tsx`、`App.tsx`、`router.tsx` |
| `apps/desktop/src/renderer/components/` | 跨功能组件：布局、登录、聊天、设置、侧栏、标题栏、弹层、反馈 | `MainLayout`、`LoginPage`、`SettingsView` |
| `apps/desktop/src/renderer/features/` | 产品功能页：Agent、自动化、SkillHub、插件、远程设备、右侧栏 | 各 FeatureLayout/Page/View |
| `apps/desktop/src/renderer/panels/` | 主内容递归布局的 panelKind 注册表与内置面板适配 | `registerPanelKind`、`registerBuiltinPanels` |
| `apps/desktop/src/renderer/layout/` | 布局树渲染、分隔拖动、面板拖放/原型 | `LayoutRoot`、`PanelDragController` |
| `apps/desktop/src/renderer/themes/` | 颜色注册、主题解析/注入、内置与本地主题 | `ColorRegistry`、`ThemeService`、`builtinThemes` |
| `apps/desktop/src/renderer/styles/` | 全局 CSS、语义变量、动效与平台表面 | `globals.css` |
| `apps/desktop/src/renderer/cindy-brain/` | “意识/Ghost”插件可视面板、Bubble、确认/媒体/审查宿主 | `ghostPanels`、`GhostConfirmDialogHost` |
| `apps/desktop/src/renderer/voice-input/` | 全局语音浮窗与词典提示的渲染模块 | `VoiceInputOverlay`、`VoiceInputDictionaryToast` |
| `apps/desktop/src/shared/` | main/preload/renderer 共用纯类型、schema、快捷键、布局树、IPC 契约 | `layoutTree.ts`、`appShortcuts.ts` |
| `apps/desktop/native/agent-island/` | macOS SwiftUI 灵动岛 helper、角色素材与预览工具 | `macos-agent-island-helper.swift` |
| `apps/desktop/cindy-updater/` | Windows Tauri 更新器及独立 WebView UI | `tauri.conf.json`、`ui/app.js` |
| `packages/maker-shared/` | 纯展示/协议模型：消息、会话、权限、文件、调度、subagent、usage | package exports |
| `packages/maker-core/` | 无 Electron 的 agent/session 编排核心 | package exports |
| `packages/plugin-protocol/` | 插件 manifest、交付、成员上传等共享契约 | package exports |
| `packages/file-browser-core/` | 本地/远端复用的扫描、ignore、ripgrep 文件浏览逻辑 | package exports |
| `packages/voice-input-core/` | provider-neutral 听写状态机、润色与词典 | package exports |
| `packages/auth-client/` | 平台无关认证 schema/客户端契约 | package exports |
| `packages/device-link/`、`device-link-protocol/` | 跨设备控制/WebSocket 协议；WebSocket 依赖注入 | package exports |
| `packages/model-providers/` | Provider 目录、模型路由与纯逻辑 | package exports |
| `packages/orca-workflow/` | Orca lead prompt/MCP 协作工作流 | package exports |
| `packages/browser-control-runtime/` | 浏览器控制运行时，UI 通过主进程安全面调用 | package exports |

架构边界的源码约束：packages 不依赖 desktop main/renderer；main 使用静态 import；renderer 视为不可信，Node 关闭、sandbox/contextIsolation 开启，敏感操作必须经最小 preload + 主进程校验。

## 3. 窗口与可见 Surface

| 窗口/Surface | 创建与入口 | 默认/最小尺寸 | UI 与生命周期 |
|---|---|---:|---|
| 主窗口 | `main/bootstrap-electron.ts` → `renderer/main-entry.tsx` | 1280×800；min 800×600 | 完整 App/Router/MainLayout；先隐藏，ready 后显示；保存窗口状态 |
| 会话副窗口 | `main/secondary-windows.ts` → 主模块图 `/cc-agent/boot` | 继承主窗或 1280×800；min 800×600 | 独立会话/Orca lead/worker；`SecondaryWindowBootGate` 解析 `bootSession` 后 replace 导航 |
| 分离右侧栏 | `main/right-sidebar-window/window.ts` → `?sidebarWindow=1` | 520×860；min 360×480 | 只挂 `SidebarWindowLayout` + `RightSidebarShell`，50px chrome；窗口状态单独持久化 |
| 分离 Ghost 面板 | `main/ghost-panel-window/window.ts` → `?ghostPanelWindow=<id>` | 480×720；min 320×400 | 只挂 `GhostPanelWindowLayout`，46px chrome + 插件面板体；每个 ghost 独立状态文件 |
| 资源用量 | `main/resource-usage-window/window.ts` → `?resourceUsageWindow=1` | 580×520；min 380×320 | CPU/内存快照独立窗口；旧 RSB `resource-usage` 页签仅为隐藏兼容恢复 |
| 语音输入浮窗 | `main/voice-input/global.ts` → voice renderer entry | 定宽高常量、屏幕边缘约束 | 透明、无框、录音/转写/取消；Windows 隐藏后销毁重建以保证鼠标命中 |
| 语音词典提示 | 同上 → `VoiceInputDictionaryToast` | 定宽高常量 | always-on-top；录音浮窗打开时抑制提示，避免遮挡 |
| 电脑权限引导 | `main/computer-permission-guide/window.ts` | 覆盖当前 workArea + 可移动 guide | 两个无框透明 always-on-top 窗口：不可聚焦背景层 + 可交互引导层 |
| 会话拖动预览 | `main/session-drag-preview.ts` | 320×68 | 无框透明、不可交互的拖拽反馈窗口 |
| macOS Agent Island | `native/agent-island/macos-agent-island-helper.swift`；`MacAgentIslandNativeHost.ts` 拉起 | 原生 pill/expanded surface | SwiftUI helper，stdin/stdout JSONL；idle/working/waitingApproval/completed 角色状态；设置页提供皮肤预览 |
| Windows 更新器 | `cindy-updater/src-tauri/tauri.conf.json` | 480×220、不可缩放 | 无装饰、always-on-top、启动隐藏；等待/授权/备份/解压/替换/启动/回滚/完成/失败，日志与终态关闭按钮 |
| 插件逻辑沙箱 | `cindy-brain/runtime/electronSandboxAdapter.ts` | 480×320、永不显示 | 每插件独立 partition 的离屏 BrowserWindow；可见 UI 由面板 webview 提供，不属于用户窗口 |

`renderer/index.tsx` 在加载任何主应用静态依赖前按 query 参数分发四个独立模块图，避免辅助窗口意外初始化语音、诊断和设置等重量逻辑。

## 4. 路由、页面与主流程

### 4.1 路由门

```text
/login
  GuestRoute -> LoginPage

/
  ProtectedRoute -> LocalDbGate -> MainLayout
      -> cc-agent / issues / skillhub / settings / plugins / diagnostics

sidebar-window / ghost-panel-window
  ProtectedRoute -> LocalDbGate -> 精简窗口壳
```

- `ProtectedRoute` 先做账号/本地模式入口判断；`LocalDbGate` 再等待按 `dataOwnerId` 切换的本地数据库。
- 根级 `RouteErrorFallback` 为全屏恢复页；MainLayout 内 pathless boundary 用 section 变体，保住导航 chrome；辅助窗口也使用 section 兜底。
- `OwnerScopedRouter` 以 `dataOwnerId:dataOwnerRecoveryEpoch` 重建 Router，防止跨账号残留 UI 状态。

### 4.2 登录 `LoginPage`

关键文件：`components/login/LoginPage.tsx`、`LoginBrandStage.tsx`、`loginDesignTokens.ts`、`contexts/AuthContext.tsx`。

实现态包括：

- identifier 输入（区域/构建决定手机号或邮箱）、验证码与重新发送；
- 第三方方式、企业 SSO、个人/企业方式选择、账号选择/绑定；
- browser handoff 的 preparing/waiting/result；
- 服务条款 consent gate；
- loading、字段错误、服务错误、返回/取消；
- Desktop “跳过登录/本地模式”，与真正认证正交；真实凭据/ticket 保持在主进程；
- 首次启动亮色门、区域徽标（cn=`CN`、dev=`Dev`、global 不展示）。

设计源里曾画过移动端跳过登录，但决策日志明确移动实现已剥离；本审计以 Desktop 实际 `LoginPage` 为准。

### 4.3 Agent 工作区 `/cc-agent`

| 页面 | 关键导出 | 业务/UI 逻辑 |
|---|---|---|
| Feature 壳 | `CCAgentFeatureLayout` | 注入 Agent 专用会话侧栏；向 `MainLayout` 输出 outlet context；记录最近 chat/doc 路由 |
| 索引恢复 | `CCAgentIndexRedirect` | 恢复最近 new/session；否则找最新合法 session；再退到 `/new` |
| 新建页 | `NewMakerDraftRoute` | 未发送前无后端 session；选择 agent/vendor、provider/model、effort/fast、权限模式、工作目录、普通/远程、worktree、协作；第一次发送时才创建 |
| 会话页 | `CCAgentSessionView` | 单一主结构：消息流 `MessageStream` + `ChatInput` + `StatusBar`；实时工具、权限、queued input、成本/状态 |
| Orca | `OrcaWorkflowRoute` | lead/worker 协作路由；协作面板、worker 状态/创建、角色、工作流进度；可在副窗打开 |
| 文件浏览 | `WorkdirBrowseRoute` | VS Code 风格文件树/文档视图、搜索、保存、dirty 切换拦截；支持图像/PDF/Markdown/HTML/model/drawio 等预览 |
| 自动化 | `SchedulerPage` | 自动化列表、筛选、创建/编辑、运行历史、立即运行；agent 模式或零 token 的 script 模式 |
| 副窗启动 | `SecondaryWindowBootGate` | 从 query 解析 session，决定常规/Orca 路由，replace 防止启动页残留 |

新建页的核心业务特点是 delayed-create：改变选项不会制造空会话；创建只发生于第一条消息提交。会话运行时允许将后续输入排队、切换权限模式、查看后台任务/子 agent、审查改动，并通过右侧栏把工具 surface 与聊天并排。

### 4.4 其他顶层页面

| 路由 | 导出/源文件 | 页面职责 |
|---|---|---|
| `/issues` | `IssueTrackerFeatureLayout` | 原 Issue Tracker 已迁 GitHub；保留说明/跳转引导，不再是完整 tracker |
| `/skillhub/local` | `SkillhubHomeView` | 按 global/project、本地 skill/command/MCP 等呈现与同步；历史入口为 local |
| `/skillhub/local/:kind/global/:name` | `SkillhubDetailView` | 全局安装项详情 |
| `/skillhub/local/:kind/project/:projectHash/:name` | `SkillhubDetailView` | 项目安装项详情 |
| `/skillhub/market` | `SkillhubMarketListView` | 市场列表、搜索/分类/排序、详情浮窗、安装/发布/编辑/可见性/审核 |
| `/settings` | `SettingsView` | 14 个设置页签，URL query 深链 |
| `/plugins` | `GhostPluginPage` | 意识/Ghost 插件管理、市场来源、安装/更新与页面面板 |
| `/billing` | redirect | 统一到 `/settings?tab=billing` |
| `/maker-experimental` | `MakerExperimentalView` | 原始 IPC/AgentEvent 诊断页；明显硬编码样式，设计债已登记 |

SkillHub 旧的市场详情/管理整页 URL 会回退到市场列表，因为详情与管理已统一为列表内浮窗。

## 5. 主窗口结构与布局树

### 5.1 页面结构

```text
MainLayout
├─ Title/drag chrome + 全局 banner/find/toast/lightbox
├─ session-list（树外左栏，全高）
└─ LayoutRoot（递归 content tree）
   ├─ chat-main（必须且只允许一个，minWidth=400，不可折叠）
   ├─ right-tabs（会话级折叠记忆，可分离）
   └─ ghost:<id>（运行时注册，可停靠/分离）
```

默认是 `session-list + (chat-main 0.5 | right-tabs 0.5)`；`Layout` schema v1 支持 row/column 递归 split，最大深度 8，每子节点 fraction 下限 0.05，float 结构预留但 v1 恒空。未知 `panelKind` 不会让整个布局回退，卸载的 ghost 仅隐藏并保留槽位；布局重置也会保存 ghost 面板。

内置 panel 注册：

- `SessionListPanel` → `collapseMemory: global`；
- `ChatMainPanel` → `collapseMemory: none`；
- `RightTabsPanel` → `collapseMemory: per-session`。

设置页会隐藏主会话侧栏；只有完整 chat view 声明右侧栏，普通设置/市场页面不会无条件挂载沉重 RSB。

### 5.2 左侧栏

关键区域：`components/sidebar/`、`features/cc-agent/sidebar/`。

- 新建 Maker、会话列表、分组/项目、搜索过滤、scheduler/skills/settings 等入口；
- 会话运行/等待/错误/远程/工作树/协作标记；
- session 1–9 快捷切换；
- 机器/远程设备切换 `MachineSwitcherMenu`；
- 分享/导出 `SessionShareExportDialog`；
- 筛选 `SidebarFilterPopover`；
- 拖动 session 时使用原生透明预览窗口，并可打开会话副窗。

## 6. 右侧栏插件系统

`features/right-sidebar/registry.ts` 定义 `TabKindPlugin` 注册；`plugins/index.ts` 仅通过 import side effect 汇总。状态按会话持久化，插件负责 default/hydrate/serialize 与可选关闭拦截，Shell/TabBar 无需为每个新插件改结构。

| kind | order | 关键 UI | 单例/菜单/状态 |
|---|---:|---|---|
| `file-browser` | 10 | `FileBrowserBody` | 可多开；选中文件、目录/文件 reveal、外部文件请求；dirty 切换确认 |
| `review` | 15 | `ReviewTabBody` | 单例；unified/split、文件树、wrap、word diff、忽略空白、Markdown 富预览、branch base、按轮次审查 |
| `subagents` | 16 | `SubagentsBody` | 单例；子 agent 运行/变更 fence |
| `background-tasks` | 17 | `BackgroundTasksBody`、`WorkflowProgressTree` | 单例；会话后台任务、workflow 详情 focus |
| `orca-workers` | 18 | `OrcaWorkersTabBody` | 隐藏菜单、程序创建、单例；关闭=结束协作，未挂载 interceptor 时 fail-closed |
| `resource-usage` | 18 | `ResourceUsageBody` | 隐藏菜单、单例；只为旧持久化 tab 兼容，新入口是独立窗口 |
| `web-browser` | 20 | `BrowserChrome`、`BrowserTabBody` | 默认 `about:blank`；URL/title/favicon/audible；持久 partition；omnibox、前进后退刷新、评论 |
| `ios-simulator` | 25 | `IOSSimulatorTabBody`、实例网格 | main 持有 simulator actor；renderer 消费 typed IPC |
| `terminal` | 30 | `TerminalTabBody` | PTY 由 main 创建；重启后强制 `created=false`；关 tab 先 dispose PTY/xterm |

`RightSidebarShell` 负责 tab 激活/关闭/重排/新增、空态、错误 boundary、detachment；`TabBodyErrorBoundary` 把单个工具崩溃限制在 tab 内。Web browser 的 guest 不是普通 React iframe，而是受 main 侧 webview 安全策略约束的持久分区页面。

## 7. 设置页完整清单

`SettingsView` 的 URL `tab` 枚举来自 `lib/tabLabels.ts`，未知/不可见 tab 回到 `general`。旧 key `remote/devices`、`feishu/slack/tina`、`api-keys/connections/ghosts` 有兼容重定向。

| tab | 显示内容/导出 | 条件 |
|---|---|---|
| `general` | `UserProfileCard`、`AppearanceSection`、`LanguageSection`、`NotificationSection`、`WindowBehaviorSection`、`ComposerSendShortcutSection`、`CollaborationSection`、`AgentResourceSection`、`GitSafetySection`、`ExperimentalSection`、`LogoutSection` | 始终；实验项可按 admin/registry 隐藏 |
| `personalization` | `UserPromptSection`、`MemorySection`、`SubagentModelSection`、`ContactsSection`、`CompactionSection`、`TerminalShellSection`、`LinkOpenSection`、`StreamFadeSection`、`TipsSection` | 本地模式也可使用本机通讯录 |
| `providers` | `ProvidersSection` | Provider/model、custom provider、价格覆盖等 |
| `billing` | `BillingSettingsSection` | 仅账号/能力允许时显示 |
| `voice-input` | `VoiceInputSection` | Linux 无全局快捷键设置；外部词典学习仅 macOS |
| `im-bot` | `ImBotSection` | 官方/个人 IM bot 同页，深链可定位分组 |
| `shortcuts` | `KeyboardShortcutsSection` | 展示可改绑项；隐藏系统惯例项仍生效 |
| `agent-island` | `AgentIslandSection` | 仅 macOS |
| `import` | `SessionImportSection` | 使用固定高度非滚动布局，保留统一 scrollbar gutter |
| `remote-control` | `RemoteControlSection` | 设备/远程控制整合页 |
| `builtin-tools` | `BuiltinToolsSection` + `McpServersSection` | 内置工具与外部 MCP 同页 |
| `computer-use` | `ComputerUseSection` | 工作目录上下文 + 安装/权限引导 |
| `help` | `HelpSection` + `HelpAssistantPanel` | 帮助知识库与问答侧面板 |
| `about` | `AboutSection` | 版本、更新、协议/关于 |

常见设置弹层：`CustomProviderDialog`、`CustomProviderRuntimeFillOverlay`、`ModelPriceOverrideDialog`、`McpServerDialog`、`ProfileEditDialog`、`ContactsImportDialog`、`ContactsManagerDialog`、`MyDevicesPanel`、`SshKeySetupDialog`。

## 8. 菜单、上下文菜单与系统入口

### 8.1 macOS 应用菜单

`bootstrap-electron.ts` 只在 macOS 设置完整 application menu；Windows/Linux 为 `null`，由应用内 `MenuButton`/标题栏承担入口。

- Cindy：关于、设置、检查更新、Services、隐藏/隐藏其他/全部显示、退出；
- File：New Maker；
- Edit：原生 undo/redo/cut/copy/paste/select all 等 role；
- View：切换侧栏、缩放、全屏；开发版额外 reload/force reload/devtools；
- Window：最小化、缩放、关闭（Cmd+W accelerator 交给 renderer 做“先关 RSB tab”焦点分派）；
- Help：帮助、Release Notes、Issues、安装 Cindy CLI。

Windows 托盘菜单含 Show 与 Quit；主窗关闭行为可在设置选择，托盘点击聚焦主窗。

### 8.2 文本右键菜单

`selection-context-menu.ts` 仅在 macOS/Windows 安装：

- 只读选择：Copy、可选“添加到对话”；macOS Look Up，Windows Bing 搜索；
- editable：Undo/Redo、Cut/Copy/Paste、富文本才有 Paste as Plain Text、Select All；若有选择再追加 Look Up/Web Search；
- 不出现 reload/view source/inspect；搜索文本最多 2000 字符；“添加到对话”只在同一 `data-selection-quote-context` 内允许。

## 9. 弹层、对话框、浮层与反馈

按用户可见目的整理（源文件均位于 `apps/desktop/src/renderer`）：

- 全局：`UpdateNoticeDialog`、`LegacyMigrationDialog`、ConfirmDialogProvider、ToastContainer、`FindInPageBar`、`RouteErrorFallback`、`TopLevelErrorBoundary`；
- 聊天媒体：`ImageLightbox`、`VideoLightbox`、`ModelLightbox`、`TextLightbox`、`MermaidLightbox`、`ToolPayloadLightbox`、Markdown/Mermaid host；
- 聊天恢复/错误：`ErrorBanner`、`ErrorMessageCard`、`RewindPreviewDialog`；
- 新建/输入：`AddRemoteProjectDialog`、`FolderPickerPopover`、`NewGoalDialog`、`AtMentionPanel`、`PendingQueuePanel`、`PinnedPlanPanel`、`WorktreeCreatingOverlay`；
- Agent/会话：`CreateWorkerPopover`、`RolePillDropdown`、`SessionBranchTreeDialog`、`MachineSwitcherMenu`、`SessionShareExportDialog`、`SidebarFilterPopover`；
- Billing：`BillingCheckoutDialog`、`PlanChangeDialog`；
- 插件：`AddMarketplaceDialog`、`MarketplaceGuideDialog`、`MarketplaceSourcesDialog`、`UpdateAllDialog`、`PluginMarketPermissionReviewHost`、`GhostPagePanelHost`；
- Scheduler：`ScheduleFormDialog`、`TaskListFilterPopover`；
- SkillHub：`PublishDialog`、`ScanResultDialog`、`ReviewVerdictDialog`、`MarketInfoEditDialog`、`VisibilityEditorDialog`、`SkillhubDiffPanel`、`SkillhubMarketPreviewPanel`；
- IM：`FeishuConflictDialog`/Host；
- 右侧栏：`AddTabDropdown`、`BrowserCommentPopover`、`EmptyState`、`TabBodyErrorBoundary`；
- 原生辅助 surface：语音 overlay/toast、computer permission guide、Agent Island、Windows updater。

## 10. 空态、加载态与错态

| 范围 | 空/加载/错误设计 | 关键证据 |
|---|---|---|
| App 启动 | Splash 覆盖已挂载界面；检查更新、下载/更新、完成等待、环境检查、失败/重试 | `SplashScreen.tsx`、DESIGN §15/§16 |
| 登录 | preparing/waiting/loading/验证码倒计时、字段错误、服务错误、取消/返回、skip login | `LoginPage.tsx` |
| 路由 | 全屏与 section 两级可恢复 fallback | `RouteErrorFallback.tsx`、`router.tsx` |
| 会话索引 | 无 session 时进 transient `/new`；不会创建空后端会话 | `CCAgentIndexRedirect.tsx`、`NewMakerDraftRoute.tsx` |
| 消息/工具 | provider/network error toast，消息错误卡/banner，permission/interaction 卡，pending queue | `App.tsx` listeners、chat components |
| 右侧栏 | 无 tab/无 session 独立空态；单 tab crash 局部兜底 | `EmptyState.tsx`、`TabBodyErrorBoundary.tsx` |
| 文件浏览 | 无选择提示、扫描/读取/预览错误、unsupported/binary、大文件、dirty 切换确认 | workdir-browse 与 file-browser plugin |
| Review | 无改动、Git 不可用、diff 读取错误、图片/Markdown fallback | review plugin |
| Scheduler | 空任务、筛选空、运行中/成功/失败/超时历史；script 协议错误结构化 | scheduler UI + python-client README |
| SkillHub/插件 | 空安装/空市场、同步/扫描/发布/审核/权限错误、隔离/quarantine/rejected 状态 | skillhub/plugin features |
| 更新器 | waiting→…→done；failed 隐藏进度条、显示错误条与关闭；日志按钮始终可用 | `cindy-updater/ui/app.js` |

## 11. 快捷键

权威注册表：`apps/desktop/src/shared/appShortcuts.ts`；用户改绑只保存 overrides，并校验冲突、保留组合与平台适用性。

| 动作 | 默认 | 范围/差异 |
|---|---|---|
| New Maker | mac `⌘N`；其他 `Ctrl+N` | app，菜单支持 |
| 切换左栏 | `Mod+B` | app |
| 轮换权限模式 | `Shift+Tab` | composer |
| 关闭右栏 tab/窗口 | `Mod+W` | 隐藏不可改绑；RSB 焦点优先关 tab；mac 否则关/藏窗，Win/Linux 不消费 |
| 前/后一个右栏 tab | mac `⌘⇧[` / `⌘⇧]`，另有 Ctrl+Shift+Tab/Ctrl+Tab；其他 Ctrl+PageUp/PageDown | app |
| 打开终端 | `Ctrl+`` | app |
| 设置 | mac `⌘,` | 隐藏设置项、菜单支持 |
| 页内查找 | `Mod+F` | app |
| 项目搜索 | `Mod+Shift+F` | workdir document |
| 保存文件 | `Mod+S` | workdir document，隐藏设置项 |
| 缩放 | Win/Linux `Ctrl +/−/0`，含小键盘 | mac 交给原生 View menu role |
| 浏览器地址栏 | `Mod+L` | browser |
| 浏览器前进/后退 | `Alt+Right/Left` | browser |
| 浏览器刷新 | `Mod+R` 或 `F5` | browser |
| 切换会话 1–9 | `Mod+1…9` | 隐藏设置项；对用户绑定让路 |
| 语音输入 | mac `Alt+Space`；Windows `Ctrl+Shift+Space` | Linux 不提供全局快捷键设置 |
| 发送/换行 | 默认 Enter 发送、Shift+Enter 换行，可在 General 改 | IME composing 有防误发；slash/mention palette 支持方向键、Enter、Esc |

## 12. 主题、Token、字体与动效

### 12.1 当前代码真值

- `ColorRegistry` + `registerColor` 构建语义 token 图；当前 `colors.ts` 有 496 个 `registerColor(` 调用点。
- `ThemeService.applyTheme` 生成 `<style id="theme-vars">:root{...}</style>`，切换 `.dark`、`data-theme` 与 `colorScheme`。
- 内置主题：Default Light/Dark、Cindy Light/Dark、Atom One Light、Solarized Light、Eclipse、One Dark Pro、GitHub Dark、Monokai Pro、Material Ocean HC；另支持本地/导入主题。
- UI 字体 Inter；代码/终端 JetBrains Mono。圆角三档：8px 内层、12px 容器、9999px pill。基础 Surface/Card/Board 扁平、默认无阴影。
- Cindy skin：浅色暖象牙、暗色中性近黑；macOS 使用 vibrancy，Windows 可使用 backgroundMaterial。
- 常规组件禁止裸 hex，必须走语义变量；`MakerExperimentalView` 是已登记例外/设计债。

### 12.2 动效

- 通用 duration token：80/150/200/250ms，另有 150ms fade；设置 tab 切换使用 150ms fade-in。
- `prefers-reduced-motion` 会关闭/压缩非必要 motion。
- 允许的目的性例外：spinner、侧栏 marquee、composer morph 220ms、完成 confetti、summon seal、streamed-word fade、品牌加载 sheen。
- 更新器进度 200ms；跨 phase 进度从 100 回 0 时临时关闭 transition，避免倒退动画像故障。

### 12.3 设计史与实现真值的分界

`figma-component-spec.md` 与 `token-decision-table.md` 包含旧红底登录、旧 Global pill、移动 skip-login 等历史记录；当前判断顺序应为：实际源码/`colors.ts` → `DESIGN.md` 现行条款 → decision log 的新日期 → Figma 读取记录。不能从旧稿直接反推今天 UI。

## 13. 平台差异

| 能力 | macOS | Windows | Linux |
|---|---|---|---|
| 顶部/菜单 | 原生 application menu、traffic lights、vibrancy | 自绘 `WindowControls`、应用内菜单、backgroundMaterial | 应用内菜单，无 application menu |
| 关窗 | Cmd+W 按焦点先关 RSB tab，否则原生 close/hide | Ctrl+W 不关主窗；可设置关闭行为与 tray | Ctrl+W 不关主窗；系统 Alt+F4 |
| 托盘 | 主逻辑重点在 Windows close-to-tray | Show/Quit tray menu | 非 Windows 分支不等同 Windows tray 产品行为 |
| Agent Island | SwiftUI 原生 helper，设置 tab 可见 | 不支持/隐藏 | 不支持/隐藏 |
| 语音全局快捷键 | Alt+Space；Input Monitoring/Accessibility 权限；外部词典学习 | Ctrl+Shift+Space；overlay 每次重建规避透明窗鼠标失效 | 设置明确显示不支持全局快捷键；不支持系统音频 mute |
| 激活首击不透传 | 可配置 | 可配置 | 设置项隐藏 |
| 更新 UI | Electron 更新路径 | 独立 Tauri updater，支持 elevation/backup/rollback | main 端按 Linux 安装信号处理，无 Windows updater UI |
| iOS Simulator | 有 Apple tooling 才真正可用 | 页面可注册但主能力受平台限制 | 同左 |
| 文本右键 | Copy/Add to chat/Look Up | Copy/Add to chat/Bing Search | 自定义原生菜单不安装 |

## 14. 关键业务规则（UI 层可见）

1. 新对话延迟创建：无首条消息就没有后端 session。
2. 主聊天永远存在且只能一个；最小宽 400，不可折叠/关闭。
3. 布局是全局树，内容跟随会话；右栏折叠/标签状态按会话分桶。
4. 未知/已卸载插件面板不会摧毁布局；坏布局才回默认。
5. RSB tab 可独立崩溃、独立关闭；terminal 关闭先回收 PTY；协作关闭会结束协同并 fail-closed。
6. 认证与本地模式并列放行；本地模式没有 token/user，不等于伪登录。
7. renderer 不拥有秘密与高权限：登录票据、文件、进程、终端、SQLite、更新、插件沙箱在 main。
8. 所有帮助/市场/设置旧深链都尽量兼容重定向，避免历史 URL 变成开发者错误页。
9. 对话可普通、远程、worktree 或 Orca；worker/lead 可以单独窗口或右栏面板查看。
10. 自动化可启动 Agent，也可直接运行受限脚本；script 权限默认全拒、按任务白名单、stdout 专用于 JSONL 协议。

## 15. 不确定项与已知债务

- 这是 commit 快照；仓库在 2026-08-13 仍高速更新，线上发行版可能与 commit 有小差异。
- 部分 UI 是否显示取决于账号能力、区域、admin、provider、平台或实验 feature registry；源码清单列出“可达实现”，不保证每个用户同时可见。
- iOS Simulator tab 在全平台注册，但实际主能力天然偏 macOS；应在运行态再确认不可用平台的空/引导态。
- `MakerExperimentalView` 裸硬编码颜色违反当前 token 规则，decision log 明确作为 backlog。
- 设计文档含大量历史勘误，本文只把 current code 当真值；Figma node 记录用于解释登录视觉，不代表所有移动实现已经落地。
- 本审计没有启动应用、没有读取本地数据库/偏好/登录态，因此无法验证 feature flag、账号 gating 与真实文案的每个运行分支；这些应由动态 UI 截图任务覆盖。

