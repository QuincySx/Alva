# Cindy Desktop UI 审计证据索引

> 仓库：`https://github.com/makecindy/cindy`
> 本地证据根：`/Users/smallraw/.cache/cindy-deep-dive/source/cindy`
> 审计 commit：`240247e52e938710f409c921a7a393097dcef52b`
> 日期：2026-08-13（Asia/Taipei）

## 1. 证据口径

- 远程仓库元数据只通过 `gh repo view` 读取；源码分析只读共享浅克隆。
- 没有启动 Cindy、没有修改源码、没有读取 `.env`、凭据、token、钥匙串、登录态、本地数据库或任何用户数据。
- “当前行为”优先级：实现源码 > `docs/design-rules/DESIGN.md` 现行规范 > 新日期 decision log > Figma 历史读取记录。
- 组件/窗口“存在”表示该 commit 有可达实现；账号、区域、平台、feature/admin gate 可能让运行时不可见。

## 2. 仓库与产品元数据

| 结论 | 证据 |
|---|---|
| repo、描述、homepage、topics、stars/forks、license、更新时间 | `gh repo view makecindy/cindy --json ...`；原始回显于本次审计命令记录 |
| 产品定位、安装入口、支持 Agent/平台 | `README.zh-CN.md`、`README.md` |
| Desktop 为 Electron + React + TS | `apps/desktop/README.md`、`apps/desktop/package.json` |
| Client monorepo 与目录边界 | `AGENTS.md`、`docs/dev-rules/repo-map.md` |

## 3. UI 总入口与路由

| 证据主题 | 文件 | 关键导出/位置 |
|---|---|---|
| 四模块图 query 分发 | `apps/desktop/src/renderer/index.tsx` | lines 1–28 |
| App provider/全局 host/owner router | `apps/desktop/src/renderer/App.tsx` | `App`、`MakerBootstrap`、`OwnerScopedRouter` |
| 完整 route tree 与 error boundary | `apps/desktop/src/renderer/router.tsx` | `router` lines 43–170 |
| 认证门 | `apps/desktop/src/renderer/components/auth/GuestRoute.tsx`、`ProtectedRoute.tsx`、`LocalDbGate.tsx` | 对应组件 |
| 登录流程 | `apps/desktop/src/renderer/components/login/LoginPage.tsx` | `LoginPage` |
| 登录品牌/首次启动 | `LoginBrandStage.tsx`、`LoginFirstLaunchLightGateBridge.tsx`、`loginDesignTokens.ts` | 对应导出 |

## 4. 主窗口、布局与 Panel

| 结论 | 文件 | 关键证据 |
|---|---|---|
| MainLayout 组合 sidebar/content/RSB/overlay/shortcut | `apps/desktop/src/renderer/components/layout/MainLayout.tsx` | `MainLayout` |
| Layout v1、默认 0.5/0.5、chat minWidth 400、开放 panelKind、深度/份额 | `apps/desktop/src/shared/layoutTree.ts` | `Layout`、`createDefaultLayout`、validation/ops |
| 三个内置 panel 与折叠记忆 | `apps/desktop/src/renderer/panels/builtinPanels.tsx` | `SessionListPanel`、`ChatMainPanel`、`RightTabsPanel`、`registerBuiltinPanels` |
| Panel runtime registry | `apps/desktop/src/renderer/panels/registry.ts` | `PanelDefinition`、`registerPanelKind` |
| 递归渲染与拖动 | `apps/desktop/src/renderer/layout/LayoutRoot.tsx`、`PanelDragController.tsx` | 对应导出 |
| 架构不变量 | `docs/dev-rules/architecture-invariants.md` | main layout tree 章节 |

## 5. Agent 页面与业务流程

| 结论 | 文件/导出 |
|---|---|
| Feature 壳与最近路由记忆 | `features/cc-agent/CCAgentFeatureLayout.tsx` → `CCAgentFeatureLayout` |
| index 恢复/回退 | `features/cc-agent/CCAgentIndexRedirect.tsx` → `CCAgentIndexRedirect` |
| delayed-create 新建页 | `features/cc-agent/NewMakerDraftRoute.tsx` → `NewMakerDraftRoute` |
| 会话结构 | `features/cc-agent/CCAgentSessionView.tsx` → `CCAgentSessionView` |
| 副窗 boot route | `features/cc-agent/SecondaryWindowBootGate.tsx` → `SecondaryWindowBootGate` |
| Orca | `features/cc-agent/OrcaWorkflowRoute.tsx`、`OrcaWorkerPanel.tsx`、worker projection hooks |
| 文件页 | `features/cc-agent/workdir-browse/WorkdirBrowseRoute.tsx` 及其 tree/search/preview 子目录 |
| 递归分屏会话（页面内） | `features/cc-agent/splitGroupStore.ts` → `MAX_SPLIT_PANES=8` 等 |
| 新建偏好与跨窗镜像 | `state/newMakerDraft.ts`、`state/providerModelMemory.ts`、`App.tsx` sync effects |

## 6. 其他顶层功能

| 功能 | 证据 |
|---|---|
| Scheduler | `features/scheduler/SchedulerPage.tsx`、`ScheduleFormDialog.tsx`、`hooks/useRuns.ts` |
| Scheduler script mode/权限协议 | `src/main/scheduler-host/python-client/README.md` |
| SkillHub local/market | `features/skillhub/SkillhubFeatureLayout.tsx`、`SkillhubHomeView.tsx`、`SkillhubDetailView.tsx`、`SkillhubMarketListView.tsx` |
| SkillHub 发布/审核/可见性 | `PublishDialog.tsx`、`ScanResultDialog.tsx`、`ReviewVerdictDialog.tsx`、`components/VisibilityEditorDialog.tsx` |
| Ghost/plugin 管理 | `features/plugin/GhostPluginPage.tsx` 与 marketplace dialogs |
| 已迁移 Issue Tracker 引导 | `features/issue-tracker/IssueTrackerFeatureLayout.tsx`、router 注释 |
| 诊断页设计债 | `features/maker-experimental/MakerExperimentalView.tsx`、`docs/design-rules/design-decision-log.md` Backlog |

## 7. 右侧栏证据

| kind | 注册文件 | Body/关键状态 |
|---|---|---|
| file-browser | `features/right-sidebar/plugins/file-browser/index.tsx` | `FileBrowserBody`、selected/reveal/external path |
| review | `.../review/index.tsx` | `ReviewTabBody`、diff mode/tree/wrap/word diff/whitespace/Markdown/base ref |
| subagents | `.../subagents/index.tsx` | `SubagentsBody` |
| background-tasks | `.../background-tasks/index.tsx` | `BackgroundTasksBody`、focusTaskId |
| orca-workers | `.../orca-workers/index.tsx` | hidden singleton、`onBeforeClose` fail-closed |
| resource-usage | `.../resource-usage/index.tsx` | hidden legacy compat 注释 |
| web-browser | `.../web-browser/index.tsx` | `WebBrowserState`、persistent URL/title/favicon/audible |
| ios-simulator | `.../ios-simulator/index.tsx` | `IOSSimulatorTabBody`、instanceId |
| terminal | `.../terminal/index.tsx` | `TerminalState`、hydrate reset、dispose on close |
| 汇总注册 | `features/right-sidebar/plugins/index.ts` | import-only registration list |
| Shell/error/empty | `RightSidebarShell.tsx`、`TabBodyErrorBoundary.tsx`、`EmptyState.tsx` | 对应导出 |

## 8. 设置证据

| 主题 | 证据 |
|---|---|
| tab ID/label/legacy aliases | `apps/desktop/src/renderer/lib/tabLabels.ts` |
| tab gating 与 section 映射 | `components/settings/SettingsView.tsx` lines 61–520 |
| General 平台行为 | `WindowBehaviorSection.tsx` |
| 快捷键改绑 | `KeyboardShortcutsSection.tsx` + `shared/appShortcuts.ts` |
| 语音平台差异 | `VoiceInputSection.tsx`（global shortcut、mute、dictionary gates） |
| Agent Island mac-only | `SettingsView.tsx` + `AgentIslandSection.tsx` |
| tools/MCP 同页 | `SettingsView.tsx` builtin-tools branch |
| Help drawer | `HelpSection` + `HelpAssistantPanel` |

## 9. 窗口证据

| Surface | 文件 | 实测配置证据 |
|---|---|---|
| 主窗口 | `src/main/bootstrap-electron.ts` | `new BrowserWindow` around line 2728；min 800×600；state default 1280×800 |
| 会话副窗 | `src/main/secondary-windows.ts` | around line 122；inherit/default 1280×800；min 800×600 |
| detached RSB | `src/main/right-sidebar-window/window.ts` | around line 44；state default 520×860；min 360×480 |
| detached Ghost | `src/main/ghost-panel-window/window.ts` | around line 45；state default 480×720；min 320×400 |
| Resource Usage | `src/main/resource-usage-window/window.ts` | line 30；580×520；min 380×320 |
| Session drag preview | `src/main/session-drag-preview.ts` | line 17；320×68、transparent |
| Permission guide pair | `src/main/computer-permission-guide/window.ts` | lines 909/978；transparent always-on-top backdrop + guide |
| Voice overlay/toast | `src/main/voice-input/global.ts` | lines 2079/2233；transparent auxiliary windows |
| Ghost hidden logic page | `src/main/cindy-brain/runtime/electronSandboxAdapter.ts` | lines 476–483；show false、480×320、sandbox/contextIsolation |
| Agent Island | `native/agent-island/AGENTS.md`、`src/main/agent-island/MacAgentIslandNativeHost.ts` | SwiftUI + JSONL + states/skins |
| Windows updater | `cindy-updater/README.md`、`src-tauri/tauri.conf.json`、`ui/index.html`、`ui/app.js` | Tauri 480×220 + phases/errors/log/quit |

## 10. 菜单与快捷键证据

| 结论 | 证据 |
|---|---|
| mac application menu 完整模板 | `src/main/bootstrap-electron.ts` around lines 1800–2024 |
| non-mac application menu null | `bootstrap-electron.ts` platform branch |
| Windows tray Show/Quit | `bootstrap-electron.ts` around lines 2175–2269 |
| selection/editable context menu | `src/main/selection-context-menu.ts` → `buildSelectionContextMenuTemplate`、`buildEditableContextMenuTemplate` |
| App shortcut registry | `src/shared/appShortcuts.ts` → `APP_SHORTCUT_DEFINITIONS` |
| MainLayout focus dispatch | `components/layout/MainLayout.tsx` shortcut effects |
| Windows title controls | `components/title-bar/WindowControls.tsx` |

## 11. 主题与设计证据

| 结论 | 证据 |
|---|---|
| 颜色注册/496 call sites | `renderer/themes/colors.ts`；`rg -c 'registerColor\('` |
| Theme injection | `renderer/themes/theme-service.ts` → `ThemeService.applyTheme` |
| Builtin themes | `renderer/themes/registry.ts`、`themes/builtin/*` |
| Local/import themes | `renderer/themes/local-themes.ts`、`shared/theme-import/*` |
| CSS motion/reduced motion | `renderer/styles/globals.css` |
| Tailwind semantic mapping | `apps/desktop/tailwind.config.ts` |
| Current design system | `docs/design-rules/DESIGN.md` |
| 登录 Figma 读取记录 | `docs/design-rules/figma-component-spec.md` |
| token 历史/当前值判定 | `docs/design-rules/token-decision-table.md` |
| 决策覆盖与 backlog | `docs/design-rules/design-decision-log.md` |

## 12. 相关 package 证据

| 包 | 用途证据 |
|---|---|
| `@cindy/maker-shared` | package.json + src exports；纯 UI/协议模型、零 React/Electron |
| `@cindy/maker-core` | package.json + src exports；平台无关 agent/session 编排 |
| `@cindy/plugin-protocol` | manifest/delivery/member-upload contracts |
| `@cindy/file-browser-core` | scanner/ignore/ripgrep exports |
| `@cindy/voice-input-core` | dictation/refinement/dictionary exports |
| `@cindy/auth-client` | zod auth contracts |
| `@cindy/device-link` | injected WebSocket/cross-device protocol |
| `@cindy/model-providers` | provider/model routing pure logic |
| `@cindy/orca-workflow` | lead MCP/prompt |

## 13. 阅读过的仓库说明/规范

- `AGENTS.md`
- `README.md`、`README.zh-CN.md`
- `DESIGN.md`（根 redirect）
- `apps/desktop/README.md`
- `apps/desktop/native/agent-island/AGENTS.md`
- `apps/desktop/cindy-updater/README.md`
- `apps/desktop/src/main/scheduler-host/python-client/README.md`
- `apps/desktop/src/renderer/vendor/drawio/README.md`
- `apps/desktop/src/renderer/vendor/tapdb/README.md`
- `docs/README.md`
- `docs/dev-rules/repo-map.md`
- `docs/dev-rules/architecture-invariants.md`
- `docs/dev-rules/electron-security-and-process-boundaries.md`
- `docs/product-rules/core-product-principles.md`
- `docs/design-rules/README.md`
- `docs/design-rules/DESIGN.md`
- `docs/design-rules/figma-component-spec.md`
- `docs/design-rules/token-decision-table.md`
- `docs/design-rules/design-decision-log.md`
- `docs/desktop-help-knowledge/*`（桌面帮助主题索引与内容）

## 14. 命令记录（只读，路径已固定）

以下是审计使用的命令族；未执行修改、反编译、依赖安装或应用启动：

```text
git rev-parse HEAD
gh repo view makecindy/cindy --json ...
rg --files <scoped dirs>
find <scoped dirs> -name AGENTS.md -o -name README.md -o -name DESIGN.md
sed -n '<range>p' <known source/doc>
nl -ba <known source> | sed -n '<range>p'
rg -n '<symbol/pattern>' <scoped source dirs>
rg -c 'registerColor\(' apps/desktop/src/renderer/themes/colors.ts
find apps/desktop/... -type f ... | wc -l
```

所有分析产物写在 `/Users/smallraw/.cache/cindy-deep-dive/source-analysis/`；唯一另行更新的是用户要求的 `/Users/smallraw/docs/github-article/makecindy-cindy-blueprint.md`。

## 15. 限制

- 未做动态点击/截图，本索引不能证实 runtime feature flags、账号 entitlement、区域配置的每一种组合。
- 未对 2000+ renderer 文件逐行做语义证明；采用入口→路由→feature→dialog/error/registry 的渐进覆盖，并对用户可见关键面逐文件取证。
- Figma 文档含明确“作废/勘误”段；本索引不把历史节点参数当成当前 UI 事实。

