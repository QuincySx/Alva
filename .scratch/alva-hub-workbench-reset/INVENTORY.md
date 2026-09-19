# H01 · Hub 现状总账（INVENTORY）

- 日期：2026-09-20；基线 commit：`764864da`；分支：`refactor/kernel-boundary`
- 状态词只使用：未开始 / 部分实现 / 待验收 / 已验收 / 阻塞。
- 本文件只登记**当前 commit 下可重跑证据**的结论；"旧文档写已完成"一律不作为证据（见 LEGACY-MAP.md 旧总表纠错）。
- 路径缩写：`web/` = `crates/alva-hub-desktop/web`；`desktop/` = `crates/alva-hub-desktop/src`；`hub-core/` = `crates/alva-hub-core/src`。

## 1. 用户入口（前端）

| # | 项 | 状态 | 证据（文件:行，基线 764864da） | 未验证项 |
|---|---|---|---|---|
| E1 | 启动入口区分"对话模式/项目模式" | 未开始 | 当前唯一 gate 是 WorkspaceGate（`web/src/App.tsx:1753-1774`、`web/src/components/workspace-gate.tsx:15-66`）；所有会话强制绑定 workspace，无 workspace 时 `submit` 直接拒绝（App.tsx:1504-1507） | — |
| E2 | 页面切换 | 部分实现 | 无路由库；`activePage` 纯 state（App.tsx:611），HubPage 定义 `web/src/components/product-pages.tsx:55`；URL `?page=` 仅初始化读一次（App.tsx:226-229） | 深链刷新保持 |
| E3 | 中央消息阅读流 ChatTranscript | 部分实现 | 只渲染 user/assistant（`web/src/components/chat-transcript.tsx:24-33`）；全量挂载 DOM（:235-269）无虚拟化；手写受限 Markdown（:45-146）+ 代码复制（:148-182）；滚动跟随（:188-229） | 真实浏览器长会话行为未测（H19） |
| E4 | 执行/控制流 HubTimeline | 部分实现 | 工具活动/审批/fallback/handoff（`web/src/components/hub-timeline.tsx:90-363`）；key 用数组索引（:71-79、97），App.tsx:978-980 自认按索引重填会误展开；仅运行中/待审批/未落定失败时挂载（App.tsx:1788-1806） | — |
| E5 | 工具事件位置 | 部分实现 | R42：工具活动被滤出主聊天列（App.tsx:852-855），归 Workbench→Inspector 的 execution overview（`web/src/components/workbench-tools.tsx:272-301`）。与 H02 中央完整展示目标冲突，待迁移 | — |
| E6 | Workbench Files | 部分实现 | 仅列 side_effects 出现过的路径（workbench-tools.tsx:32-45），非真实文件树 | — |
| E7 | Workbench Git/Review | 未开始（占位） | GitTool/ReviewTool 只是已记录事件罗列（workbench-tools.tsx:72-109，R54 注释），非真实 git status/diff | — |
| E8 | Workbench Browser | 未开始（诚实禁用） | 注释"R53: nothing loads here"（workbench-tools.tsx:541-543），地址输入与打开按钮 disabled（:548-549） | — |
| E9 | Workbench Terminal | 未开始（诚实禁用） | 注释"R53: the native terminal adapter is not wired in"（workbench-tools.tsx:557-559），输入 disabled（:564） | — |
| E10 | Text 预览 | 未开始（诚实禁用） | workbench-tools.tsx:64-66 | — |
| E11 | Agent/Team 工具 | 部分实现 | TeamRun worker/task 树来自真实 IPC（workbench-tools.tsx:111-182） | 真实 Tauri 全流程待验收 |
| E12 | Inspector | 部分实现 | `web/src/components/engine-inspector.tsx` 默认 loader 为真实 `getHubInspection`（engine-inspector.tsx:223）；仅 alva 引擎有 native 记录；R79 定位缺陷未修（见 LEGACY-MAP） | — |
| E13 | 设置页 | 部分实现 | 大量诚实禁用项（product-pages.tsx:85-92、221-230、371-398、413-451、471-495、500-610） | — |
| E14 | Composer 发送 | 部分实现 | Enter 发送/Shift+Enter 换行（`web/src/components/hub-composer.tsx:46-51`） | — |
| E15 | i18n | 部分实现 | `web/src/i18n.tsx`（1551 行）类型安全双语；残留 inline locale 三元（App.tsx:151/239、hub-timeline.tsx:153/173）与未本地化 Inspector 英文段（workbench-tools.tsx:454-493） | — |
| E16 | 键盘可达性 | 未开始 | 无全局快捷键；⌘K 等仅展示（product-pages.tsx:546-556）；局部 Esc/箭头键（App.tsx:595-608、`web/src/ui/layout.tsx:83-86`） | — |
| E17 | reduced-motion | 部分实现 | 仅 CSS 三处（`web/src/styles/primitives.css:392,461`、`app.css:1281`）；设置开关为禁用占位（product-pages.tsx:397-398） | — |
| E18 | 状态管理 | 部分实现（缺位） | 无 store；zustand 声明未用（package.json:23，src 零引用）；~50 个 useState 堆在 2006 行 App.tsx（:547-627） | — |
| E19 | 事件订阅 | 部分实现 | 单一 mount 期长驻订阅（App.tsx:667-726），`activeSessionRef` 过滤；切会话不重建订阅，后台会话事件被丢弃不缓存（App.tsx:681-684） | — |
| E20 | UI Lab / demo | 已验收（dev-only） | `web/src/ui-lab.tsx`（?ui-lab=1）、`web/src/demo-data.ts`（?demo=1）；Playwright `web/tests/visual.spec.ts`+`behavior.spec.ts`；vitest 8 个单测 | Lab 不含页面级/时间线状态 story |

## 2. 后端入口（Tauri command / HTTP）

| # | 项 | 状态 | 证据 | 未验证项 |
|---|---|---|---|---|
| B1 | 运行/停止/切换 | 待验收 | `desktop/main.rs:166/1222/1288`（`run_hub_prompt`/`stop_hub_attempt`/`switch_hub_engine`） | R77/R78/R84/R86/R87/R91 缺陷未修（LEGACY-MAP） |
| B2 | 会话 CRUD/详情 | 待验收 | `list/get_hub_sessions`、`get_hub_inspection`、memory、transcript、workspace（main.rs:1211-1461） | R88/R89 串台缺陷未修 |
| B3 | 审批 | 待验收 | `respond_hub_permission`（main.rs:414，stale→Expired :465-470）、分组审批（:489-556） | — |
| B4 | Provider/路由 | 待验收 | providers/routes/default-route/discover/test（main.rs:1310-1390）；`desktop/providers.rs:144` Catalog、`route_planner.rs:44` | 四引擎兼容矩阵未成文（H06） |
| B5 | readiness/repair | 待验收 | main.rs:1168-1185、:1487 | — |
| B6 | Team/worker | 待验收 | main.rs:561-1050；受管 worktree `desktop/worktree.rs:85-247` | — |
| B7 | 文件浏览服务 | 未开始 | 无对应 Tauri command（main.rs:1545-1587 注册表核查） | — |
| B8 | Git 服务 | 未开始 | 无 | — |
| B9 | 终端 PTY | 未开始 | 无 | — |
| B10 | 浏览器宿主 | 未开始 | 无 | — |
| B11 | HTTP server | 部分实现 | `alva-hub-server/src/main.rs:1161-1192` 路由表；`/api/events` 全局 SSE 仅实时无回放（:1044-1059）；无鉴权；team/worktree/repair 端点缺失 | — |
| B12 | 引擎注册表 | 部分实现 | `desktop/engine_registry.rs:42`、接线 `engine_wiring.rs:255-545`；alva 进程内可用，claude/codex/pi 为 CLI 模式，Interactive 全拒绝（:379-403、:447-468、:511-516） | — |

## 3. 存储

| # | 项 | 状态 | 证据 |
|---|---|---|---|
| S1 | HubStore SQLite schema 17 | 待验收 | `desktop/store/mod.rs:46,55`；建表/迁移 `desktop/store/db.rs:12-383`；崩溃修复 db.rs:385-434 |
| S2 | project 实体 | 未开始 | 无 project 表；仅 `hub_sessions.workspace` 路径 + `workspace_kind`（`desktop/session_workspace.rs:15-19`，受管目录 marker :46,:82） |
| S3 | 会话详情全量读取 | 待验收（无分页） | `get_session`（`desktop/store/turn.rs:166-221`）、`list_sessions`（:139-164）无 limit/cursor |
| S4 | 事件持久化语义 | 部分实现 | 权威记录 = turns/attempts/side_effects（run.rs:935-1005、`store/turn.rs:47-96`）；`timeline_events` 表只存引擎切换（uuid，`store/mod.rs:393-409`）；普通对话事件不落事件表 |
| S5 | RuntimeEvent | 待验收 | 8 变体（`alva-engine-runtime/src/event.rs:28-97`）；Hub 包装 `hub-core/runtime.rs:238-242` |
| S6 | TimelineEvent DTO | 部分实现 | 12 状态（`desktop/run.rs:52-65`）+ 前端 15 状态（bridge.ts:10-25）；**无 event seq**，item_id 按情形携带 tool call id/审批 id/`{turn_id}:user` 等（run.rs:651-1044）；线上事件无 turn_id/attempt_id 字段 |
| S7 | 历史与实时不同构 | 部分实现 | 历史 = `HubSessionDetail{turns,...}`（store/turn.rs:110-122）；实时 = `hub_timeline` 事件即发即弃（main.rs:210-215、:382-390），assistant delta 逐条 AssistantMessage（run.rs:897-913） |
| S8 | worktree 发现 | 未开始 | 无 git worktree enumeration；受管 worker worktree 仅用于 team（worktree.rs:85） |

## 4. 当前证据基线（命令）

```bash
# 单测（当前全绿基线，截至 764864da）
cargo test -p alva-hub-desktop -p alva-hub-core -p alva-engine-adapter-alva
cd crates/alva-hub-desktop/web && npm run test        # vitest
cd crates/alva-hub-desktop/web && npx playwright test  # 需 demo 构建
```

性能基线 = H19 交付（PERFORMANCE.md）；本文件不预填任何性能数字。

## 5. 结论速览

- 两模式入口（E1）、project 实体（S2）、worktree 发现（S8）、文件/Git/终端/浏览器服务（B7-B10）**全部未开始**。
- 中央完整展示被 R42 迁移策略阻挡（E5），历史读取无分页（S3），事件无稳定序号（S6/S7）——H20 必须先建立统一投影。
- 前端无虚拟化、无会话级订阅生命周期（E3/E4/E19）——H19 基线 + H21 是硬前置。
- R45/R77-R91/R96/R97 无任何修复 commit（LEGACY-MAP.md），H15/H16 沿原卡执行。
