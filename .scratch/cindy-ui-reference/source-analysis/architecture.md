# Cindy Desktop UI 架构

> Snapshot：`makecindy/cindy@240247e52e938710f409c921a7a393097dcef52b`

## 1. 技术栈

| 层 | 实现 |
|---|---|
| Desktop shell | Electron `41.10.3` |
| UI | React `19`、React Router `7`、TypeScript `5.7`、Vite `6` |
| 样式/控件 | Tailwind CSS `3`、Radix UI、Lucide、CSS semantic variables |
| 文档/编辑器 | CodeMirror、Tiptap、Mermaid、PDF.js、`<model-viewer>`、vendored draw.io viewer |
| 终端 | xterm.js + main-owned PTY |
| 本地数据 | main-owned SQLite/Drizzle；renderer 只走 IPC |
| 原生辅助 | macOS SwiftUI Agent Island；Windows Tauri updater |

## 2. 进程与信任边界

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ Electron main（特权）                                                    │
│ bootstrap / BrowserWindows / menu / IPC / SQLite / filesystem / PTY     │
│ agent runtime / scheduler / plugin sandbox / browser policy / updater    │
└───────────────┬──────────────────────────┬───────────────────────────────┘
                │ typed + validated IPC    │ 子进程 / native protocol
        ┌───────▼────────┐       ┌─────────▼──────────────────────────────┐
        │ preload         │       │ Native / helper                       │
        │ 最小白名单桥    │       │ macOS Agent Island(JSONL)             │
        │ electronAPI     │       │ Windows cindy-updater(Tauri events)   │
        └───────┬────────┘       │ agent/provider/CLI/script processes   │
                │                 └────────────────────────────────────────┘
┌───────────────▼──────────────────────────────────────────────────────────┐
│ Renderer（不可信；sandbox + contextIsolation；无 Node）                 │
│ React providers -> Router -> MainLayout -> LayoutRoot -> Page/Panel      │
│ local UI store / theme / i18n / ephemeral draft / IPC projections       │
└───────────────┬──────────────────────────┬───────────────────────────────┘
                │                          │ isolated persistent partitions
        ┌───────▼────────┐       ┌─────────▼──────────────────────────────┐
        │ RSB webview     │       │ Ghost visible webview + hidden logic  │
        │ browser guest   │       │ BrowserWindow sandbox per plugin      │
        └────────────────┘       └────────────────────────────────────────┘
```

安全原则由 `docs/dev-rules/electron-security-and-process-boundaries.md` 定义：renderer 不应直接持有 secrets、Node 或任意系统能力；preload 只做窄桥；main 校验 payload、来源、路径与 webview 生命周期。插件逻辑页在独立 partition 的隐藏沙箱，浏览器 guest 也受 main 的 webview 安全策略控制。

## 3. Renderer 启动与 Provider 树

`renderer/index.tsx` 先解析 query，再动态 import：

```text
?resourceUsageWindow=1 ──> resource-usage-entry
?sidebarWindow=1       ──> sidebar-window-entry
?ghostPanelWindow=id   ──> ghost-panel-window-entry
otherwise              ──> main-entry -> App
```

主 `App` 的逻辑层次：

```text
ThemeProvider
└─ FontSettingsProvider
   └─ LocaleProvider
      └─ ConfirmDialogProvider
         └─ EnvCheckProvider / EnvCheckGuard
            └─ AuthProvider
               └─ WorktreeProvider / PrRefsProvider
                  └─ LoginHandoffProvider
                     ├─ Splash / Login brand gates
                     ├─ MakerBootstrap（全局 IPC/projection 同步）
                     ├─ OwnerScopedRouter
                     └─ 全局 hosts
                        ├─ ToastContainer / FindInPageBar
                        ├─ LegacyMigrationDialog
                        ├─ GhostConfirmDialogHost
                        └─ PluginMarketPermissionReviewHost
```

关键点：

- 主监听器只注册一次，session view 不拥有全局 IPC 订阅生命周期；
- 登录 owner 切换会用 key 重建 Router，并重新预热 provider/capability snapshot；
- renderer localStorage 中的纯偏好镜像给 main，但认证票据仍留在 main；
- 辅助窗口使用更小模块图，避免主 App 副作用和 bundle 成本。

## 4. 路由架构

```text
GuestRoute
└─ /login -> LoginPage

ProtectedRoute
└─ LocalDbGate
   ├─ /sidebar-window -> SidebarWindowLayout
   ├─ /ghost-panel-window -> GhostPanelWindowLayout
   └─ MainLayout
      └─ section error boundary
         ├─ / -> /cc-agent
         ├─ /cc-agent -> CCAgentFeatureLayout
         │  ├─ index -> CCAgentIndexRedirect
         │  ├─ boot -> SecondaryWindowBootGate
         │  ├─ new -> NewMakerDraftRoute
         │  ├─ scheduled -> SchedulerPage
         │  ├─ files/:sessionId -> WorkdirBrowseRoute
         │  ├─ orca/:sessionId -> OrcaWorkflowRoute
         │  └─ :sessionId -> CCAgentSessionView
         ├─ /issues -> IssueTrackerFeatureLayout
         ├─ /skillhub -> SkillhubFeatureLayout
         │  ├─ local -> Home / global detail / project detail
         │  └─ market -> MarketList（详情/管理使用浮窗）
         ├─ /settings -> SettingsView
         ├─ /plugins -> GhostPluginPage
         ├─ /billing -> settings?tab=billing
         └─ /maker-experimental -> MakerExperimentalView
```

路由顺序有业务意义：`boot/new/scheduled/files/orca` 必须在 `:sessionId` 之前，否则静态段会被当成 session id。旧 deep link 用 redirect 保持兼容。

## 5. 布局系统

`shared/layoutTree.ts` 是 main 和 renderer 共同的纯函数真源：main 验证/持久化，renderer 渲染/编辑，避免 schema 漂移。

```text
Layout v1
├─ sidebar（树外，全高，edge=left）
│  └─ pane(session-list)
├─ content（递归 LayoutNode）
│  └─ split(row)
│     ├─ 0.5 pane(chat-main, minWidth=400)
│     └─ 0.5 pane(right-tabs)
└─ float[]（v1 保留字段，恒空）
```

约束：

- `chat-main` 恰好一个、必须可见、不可关闭/折叠；
- 最大深度 8，单子树最小 fraction 0.05；
- panelKind 是开放命名空间，插件使用 `ghost:<id>`；
- 未知 panelKind 是合法布局：渲染时隐藏，不因版本差异/卸载重置整树；
- reset 恢复内置默认但保留 ghost slots；
- 内置折叠记忆分别是 global / none / per-session。

`MainLayout` 仍持有大型页面节点与状态，`builtinPanels.tsx` 只通过 `BuiltinPanelBridge` 把 ReactNode 交给 `LayoutRoot`，避免布局引擎反向拥有业务组件。

## 6. 会话 UI 状态流

```text
NewMakerDraftRoute（renderer ephemeral prefs）
  ├─ agent/provider/model/effort/fast/permission
  ├─ workdir/local-remote/worktree/collaboration
  └─ 首次 send
       │ create via typed IPC
       ▼
main session repository + agent runtime
       │ push AgentEvent / projection / permission / usage
       ▼
makerChatStore（全局、按 session 归档）
       ├─ CCAgentSessionView
       │  ├─ MessageStream
       │  ├─ ChatInput / PendingQueue
       │  └─ StatusBar
       ├─ Session sidebar badges
       └─ RSB tabs
          ├─ review/subagents/background-tasks/orca-workers
          └─ file/browser/terminal/simulator
```

UI 侧重要状态所有权：

- new maker 草稿与 model preference 在 renderer 持久化，main 只持有用于远程/worker spawn 的镜像；
- live session 是 main/DB 权威，renderer store 是 event projection；
- RSB tab state 按 session 保存，插件自行 hydrate/serialize；
- terminal 的 PTY 不跨 main 生命周期，hydrate 必须把 `created` 归 false；
- web browser 的 URL/title/favicon 属于 tab state，cookie/history/sessionStorage 属于独立 Chromium partition；
- right sidebar collapse 是 per-session，主 layout tree 是 global。

## 7. 右侧栏扩展架构

```text
plugins/index.ts
  import side effects
       │
       ▼
registerTabKind(plugin)
  ├─ kind + menu metadata
  ├─ TabPillTitle / TabPillIcon
  ├─ lazy TabBody
  ├─ defaultState
  ├─ hydrateState / serializeState
  └─ onBeforeClose（可选）
       │
       ▼
registry -> RightSidebarShell
             ├─ TabBar / AddTabDropdown
             ├─ session-scoped store
             ├─ TabBodyErrorBoundary
             └─ detach -> SidebarWindowLayout
```

内置 kind 顺序：file-browser(10)、review(15)、subagents(16)、background-tasks(17)、orca-workers(18 hidden)、resource-usage(18 hidden compat)、web-browser(20)、ios-simulator(25)、terminal(30)。

插件关闭语义不统一是刻意设计：普通 tab 直接关；terminal 先 dispose 资源；Orca tab 关闭等于结束协作，必须确认且无 interceptor 时拒绝。

## 8. Ghost 插件可视架构

```text
Installed Ghost manifest / protocol contracts
          │
main cindy-brain runtime
  ├─ hidden ElectronSandboxAdapter (one partition/window per ghost)
  ├─ capability / IPC validation
  └─ visible panel/webview lifecycle
          │
renderer cindy-brain
  ├─ GhostPluginPage（管理）
  ├─ ghostPanels / ghostPanelBody（嵌入）
  ├─ GhostPanelWindowLayout（分离）
  ├─ GhostPanelBubbleLayer
  ├─ GhostConfirmDialogHost
  └─ GhostMediaLightboxHost
```

开放 `panelKind` 与独立窗口共同保证插件既能停靠进递归布局，也能作为独立窗口运行。插件市场的安装/更新/权限审查与运行面板分离，避免 UI 直接授予能力。

## 9. 设置与能力门

`SettingsView` 是单路由、query-tab 的控制台；它不是每页独立 route。显示由多种 gate 组合：

- platform：Agent Island 仅 macOS；Windows close behavior 仅 Windows；Linux 隐藏语音全局快捷键；
- account/capability：Billing 有能力才显示；
- mode：本地模式与登录模式共享大量本机设置；
- role/feature registry：Experimental 可 admin-only 或完全返回 null；
- owner：billing/provider/memory 等用 `dataOwnerId` key 避免跨账号复用实例。

## 10. Theme 架构

```text
registerColor(id, defaults/aliases)
          │
          ▼
ColorRegistry（semantic token graph）
          │
Theme.colors overrides + resolveThemeValue
          │
ThemeService.applyTheme
  ├─ inject #theme-vars :root CSS
  ├─ toggle .dark
  ├─ set data-theme
  └─ set color-scheme
          │
components use var(--semantic-token)
```

主题不是简单 dark class：每个主题是完整/部分语义覆盖，alias 通过 registry 求值；本地主题和 VSCode/Obsidian 调色板导入也进入同一管线。设计系统要求任何新增 UI 同时可用于 light/dark，禁止组件直接写单模式颜色。

## 11. 窗口状态与平台装配

- 主/副窗：最小 800×600；主窗位置尺寸持久化；副窗默认继承主窗。
- detached RSB：独立状态文件；ghost window 按 ghost id 独立状态文件。
- macOS：原生 menu/vibrancy/traffic lights；Agent Island 子进程；Cmd+W 与原生 close 语义协调。
- Windows：自绘标题栏、background material、tray close policy、独立 Tauri updater。
- Linux：无 application menu；全局语音快捷键与系统音频 mute 明确不可用；其他 UI 仍通过通用 Electron renderer。

## 12. 错误隔离

```text
TopLevelErrorBoundary / root RouteErrorFallback
             │
MainLayout section RouteErrorFallback（导航仍可用）
             │
Feature/page local states
             │
RightSidebar TabBodyErrorBoundary（单个工具 tab）
             │
webview / ghost sandbox process boundary
```

这是一套从 React 树到多进程的分层隔离：页面错误不应摧毁 chrome，tab 错误不应摧毁会话，guest/plugin 错误不应取得 renderer/main 权限。

## 13. 相关包的依赖方向

```text
maker-shared / auth-client / model-providers / plugin-protocol
file-browser-core / voice-input-core / device-link / orca-workflow
                         ▲
                         │ imports pure contracts/logic
apps/desktop renderer ───┼── typed preload IPC ──> apps/desktop main
                         │                         │
                         └─────────────────────────┘
                         main may also consume packages
```

约束是单向的：共享 package 不得 import Electron renderer/main。`maker-core` 负责无平台 agent/session 编排，桌面只负责平台装配与交互；这与产品原则“Cindy 连接现有智能，不在 UI 层重造 runtime”一致。

## 14. 评估

优势：

- 路由、布局树、右栏插件、窗口模块图的边界清晰，适合继续扩展；
- renderer/main 信任边界写入仓库规则，不只是口头约定；
- delayed-create、局部 error boundary、未知 panel 保留等规则减少用户状态损坏；
- 主题由语义 registry 驱动，能够承受多内置/本地主题。

风险与债务：

- 主进程 `bootstrap-electron.ts` 与若干 domain 文件非常大，虽然模块在拆，但仍是变更热点；
- 右栏与 Ghost 是两套扩展面，需要持续防止生命周期/权限语义漂移；
- 大量 UI 由账号、平台、实验 gate 决定，纯静态审计无法证明所有组合均可达；
- 设计史很长且旧结论保留在文档中，审计/实现必须遵守“源码优先 + 新决策覆盖旧记录”；
- experimental diagnostics 的裸色值是明确 backlog。

