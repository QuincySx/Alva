# alva-hub-desktop/web/src
> Hub Desktop React session list、workspace、composer 与产品 timeline。

## 地位

Tauri webview 的纯表现层。只消费稳定 IPC DTO 和 `hub_timeline` 产品事件，
不理解 BaseAgent 或 adapter-native stream。

## 逻辑

1. `bridge` 选择 workspace、提交/恢复 prompt、请求手动 engine switch、global/session fallback policy 与 engine readiness，并读取持久化 session。
2. `App` 管理 session list、EngineBranch 选择、显式授权的有序 fallback chain、switch queue、历史 Turn/Attempt、interrupted 提示和用户输入。
3. `App` 的 Engines dashboard 展示每个 engine 的版本/能力/auth/stream health 与结构化不可用原因；composer 与 engine selector 同步显示不健康状态。
4. 自动 fallback 以非阻塞 `fallback` timeline card 呈现 source→target、原因与 handoff 摘要；skip 复用 fallback 投影，链路耗尽以 `exhausted` card 展示全 cycle 终态（live event 与持久化 fact 同一投影）。
5. fallback card 携带 Stop / Switch back / View handoff 控件：Stop 仅在运行中可用，Switch back 在已回源时禁用，View handoff 展开结构化 HandoffPackage；`attempt_cancelled` fact 投影为 cancelled card。
6. 新 session 在 composer 顶栏选择 PermissionProfile（默认 workspace_write），session 打开后只读；engine selector 按 `get_permission_eligibility` 把只能弱化 profile 的 engine 标记 ineligible（禁用 + 原因），可执行映射的等价/更严格摘要就地展示。
7. `permission_required` timeline card 携带 Allow/Deny 控件，经 `respond_hub_permission` 按 request_id 路由决定；已回答的卡片本地锁定不可重发。
8. Team panel：为 active session 创建 TeamRun、Add worker（多 engine 并行批次）、按 worker 队列派发/Message；queued 任务就地 Edit/Cancel；每 worker Focus（打开其 durable session）与 Interrupt；`worker_outcome` fact 投影为 `worker` card，live worker 事件触发队列刷新（teamLeadRef 防 stale closure）。
9. 派发任务可声明 ResourceClaims（`src/lib.rs` / `read:docs` / `facility:x` / `tool:git`），队列项展示各自 claim；`resource_wait` fact 投影为 worker card 显示等待资源与持有者。
10. Add worker 可勾选 Isolated worktree；隔离 worker 卡片带 isolated 徽标与 worktree 分支，提供 Review changes / Adopt / Discard worktree，预览展示变更、冲突与验证证据后才可 Adopt。
11. worker 卡片按 lifecycle 提供 Hibernate/Archive（active 时）或 Restore（否则），非 active 带状态徽标；archived worker 默认不出现在活跃面，"Show archived" 可显示；"End run" 归档全部 worker。
12. 等价并发审批合并为一张 approval-group 卡（逐条列出 requester/工具/资源），一次 Allow all / Deny all；Team panel 有独立的 Autonomous run 开关。
13. `index.css` 提供两栏 Desktop walking-skeleton 布局。

## 约束

- 不直接渲染 raw adapter event。
- browser-only 模式必须给出清楚错误，不产生未捕获异常。
- 提供 Alva/Claude/Codex EngineTarget；session list 不泄露 engine-native session ID。
- 运行中选择目标 engine 只显示 queued；用户选回 active engine 会撤销 pending switch。
- 保存 fallback target 前必须展示 model/provider、账号来源、账单路径与自动使用授权；手动 switch 不改变授权。
- interrupted 会话保持可选择、可继续，不能伪装成 running 或普通 completed。
- readiness 原因渲染结构化 reason code，不透传 stderr 文本；Refresh 只读，不改任何配置。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| React entry | `main.tsx` | 挂载应用 |
| Product UI | `App.tsx` | workspace、engine selector、permission profile 与 eligibility、readiness dashboard、fallback authorization、switch queue、composer、timeline 与 permission card |
| IPC bridge | `bridge.ts` | run/switch/policy/readiness/session/permission 的 Tauri invoke/listen/dialog 封装 |
| Styles | `index.css` | Desktop 多引擎状态视觉与响应式布局 |
