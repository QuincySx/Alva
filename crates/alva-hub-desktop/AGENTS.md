# alva-hub-desktop
> ALVA Hub 的 Desktop-first L7 产品入口；独立于原生 Alva Desktop。

## 地位

本 crate 位于 `alva-hub-core` 与 engine adapters 之上。它拥有 Hub 产品交互和
Desktop IPC，不向 kernel、agent-core、`alva-app-cli` 或 `alva-app-tauri`
反向注入多引擎依赖。

## 逻辑

1. 产品控制器通过 `EngineRuntime` seam 驱动 `HubRuntime`。
2. `HubStore` 以 Desktop 私有 SQLite 持久化 product session、session-scoped FallbackPolicy snapshot、多 EngineBranch、switch、handoff、timeline、Turn 和 EngineAttempt。
3. Attempt 运行中增量记录 native session 与工具开始/完成证据（含 adapter 保留的真实 duration），终态记录 usage/失败原因。
4. 用户显式选择 ALVA/Claude Code/Codex/Pi；源 branch parked 并保留 native session，目标 branch 创建或恢复。Claude/Codex 可用自己的订阅，也可消费协议兼容的第三方 API；ALVA/Pi 只消费第三方 API。模型 route 按 provider × model × engine 精确声明 reasoning 档位与 Fast 能力，未知模型不猜测；选择随 EngineBranch 持久化并投影到实际 adapter，Fast 仅进入声明支持它的 Codex route。
5. 首次进入目标 branch 注入完整 HandoffPackage；返回 parked branch 以 watermark 生成 delta 并先 native resume，失效时原子升级 full handoff 后 fresh 执行。
6. 运行中切换排队到安全终态；启动修复 running Attempt 后自动应用可执行的 pending switch。
7. Tauri command 把用户运行/切换请求转换为 Hub 产品操作；所有 engine 的产品 TimelineEvent 在创建点取得同一产品时钟的 `created_at` 后再发给 React，raw adapter clock 不出产品边界。
8. React/Vite/TypeScript 前端的新会话中央入口优先并推荐“项目”，显式选择现有 Workspace；“独立会话”作为第二选项在 app-data 获得 owner-only 专属目录。Sidebar 不重复放 Workspace 入口。两种模式的 HubSession 历史、分支和运行状态都独立，项目模式仅共享项目文件。
9. 用户保存全局或 session fallback chain 前可看到 target 的 model/provider、账号、账单路径和自动使用授权；手动切换不授予自动 fallback 权限。
10. Engines dashboard 自动发现三个 engine 并展示版本兼容性、capabilities、auth 与 stream health；不可用原因是结构化 reason，可从 composer 与详情到达；刷新只读。

11. terminal 失败先经稳定 FailureClass 矩阵分类（missing/incompatible/auth/model/rate-quota-billing/context/crash/transport/other）；仅分类到 operational continuity failure 才在同一 Turn 自动切换到 fallback chain 下一个授权 target：源 branch parked、handoff 先持久化再执行、串行新 Attempt、非阻塞 fallback timeline card。
12. fallback chain 按 policy 顺序有界执行：每个 EngineTarget（engine/model/provider 三元组）每 attempt cycle 最多考虑一次、不回环；不健康或能力不兼容 target 被跳过并落 `engine_fallback_skipped` fact；链路耗尽落 `fallback_exhausted` card，列出每次 attempt/skip 的终态与原因。
13. stall watchdog：无 engine/tool/permission/stream 活动的 180s 可证明窗口后确认 stall 并分类为 continuity failure 进入有界 fallback；open tool、pending 审批与恢复中的输出暂停/重置计时。
14. fallback card 提供可逆控制：View handoff 展示实际发送的结构化 HandoffPackage（delivered/pending 状态）；Switch back 走手动 switch 恢复 parked source（后续 run 遵守 delta/full resume 规则）；Stop 取消当前活动 Attempt 并落 `attempt_cancelled` fact，重启不重放。
15. HubSession 持有稳定 PermissionProfile（interactive/workspace_write/unrestricted），branch 切换与 fallback 不改变；engine 只有能等价或更严格执行该 profile 才有资格（映射契约在 `permissions.rs`），只能弱化的 target 标记 ineligible 并显示原因，fallback、手动 switch 与 engine 装配三处都 fail-closed；PermissionRequest/Decision 按 request_id 路由到发起请求的那个 EngineSessionId，决定单次使用且 attempt 终态/Stop 后失效，无路由身份或已 doomed 的等待以有界 stall 收敛。

16. HandoffPackage 跨 engine 前经统一过滤（credentials/env 值清洗、账号账单标签与 team snapshot 剥离），过滤在持久化前完成故 View handoff 即 adapter 实收内容；full transcript 每 session 显式开启（默认关），启用后仍过同一过滤。

17. readiness failure 提供修复入口：npm 分发 engine 一键 install/upgrade，其余最短官方指引；unauthenticated 有目标明确的认证修复指引；完成后重新探测恢复 eligibility；修复失败或取消不动任何 Hub 状态。

18. TeamRun：lead session 可建团队并向不同 engine 的 Hub-owned durable WorkerSession 派发 WorkerTask（scope/status/result/changed files/验证证据），worker 终态经执行通道回 lead timeline 并在 UI 可见；全部关联在数据库重开后保持。

19. Lead 可一批向多个不同 engine 的 worker 派发独立任务并行执行；每个 worker 有独立持久有序 queue，pending 任务消费前可查看/编辑/取消；用户可 focus/message/interrupt 指定 worker；outcome 保持 task/engine/验证归属。

20. 共享工作区 ResourceClaims：mutating WorkerTask 启动前声明 path/facility/exclusive-tool claim，兼容的并行、冲突的确定性排队；in-product Alva worker 与外部 engine 受同一产品级调度（产品级排程，非 kernel tool lock）；更早的等待者预留资源防饿死；timeline 展示等待的资源及其持有者。claim 只约束 dispatch 的 WorkerTask，直接对 session 发 prompt 的路径不在闸门内。

21. 高冲突/探索性/高风险任务可用 managed isolated worktree：worker 在独立工作树与分支上跑，UI 明确标注隔离状态与 worktree 身份；完成不自动改共享工作区，必须经展示变更/冲突/验证的显式 adoption 才进入；取消或丢弃隔离 worker 只删工作树，不碰共享数据。

22. WorkerSession 的 durable identity、UI 可见性与 live runtime 三者独立：可 hibernate（释放 runtime，保留 identity/history/queue）、以新任务 resume、archive（移出活跃协作面但不删记录）；archive/hibernate/terminal/delete 在领域状态中互不相同；TeamRun 结束默认归档而非删除；重启恢复 Lead/Worker 关联、queue、branch、timeline 与 lifecycle。

23. Lead 或某个 Worker 遇到 continuity failure 时只有该 session 进入 fallback：TeamRun identity、其他 session 与 queue 全部保留，兄弟 Worker 不被取消（并发测试证明一个 Worker fallback 时另一个继续推进到完成）；fallback TimelineEvent 明确归属发生 fallback 的那个 session；lead/worker 的 handoff 各自携带 TeamRun / WorkerTask+queue+证据快照。

24. Lead 与 Worker 默认继承 HubSession PermissionProfile，建 Worker 不隐式放宽权限，每个 Worker 的 adapter mapping 都必须满足该 profile；autonomous TeamRun 需要独立、可见、持久化的用户选择；等价并发 PermissionRequest 合并为一张审批卡但逐条展示 Worker/操作/资源范围，一次 allow/deny 精确路由到全部被选请求并留审计记录。

25. 两栏信息架构收口：左栏 HubSession 创建/选择/搜索，composer 集中 engine、model、PermissionProfile 与 FallbackPolicy，timeline 只用产品 TimelineEvent，并按 shared clock 投影 Input/Agent/Tools 三泳道；可选 Details 抽屉完整检视 branches、attempts、handoff 与 engine diagnostics；collaboration surface 管理 Lead/Workers/queues/lifecycle；fallback、permission 与 Worker 卡片在重启后要么可操作要么明确终态。

25.1. Workbench Inspector 与 timeline 分层：timeline 继续只读产品 TimelineEvent；Inspector 经 `HubInspectorService` 使用与 Alva adapter 共享的 `SessionRepository`，把权威 typed records 投影为稳定 IPC DTO。Tauri command 与 headless HTTP endpoint 同构；前端可钻取 LLM/Tool/Middleware/raw durable record 和递归子 Agent，并按详情→当前 Agent→父 Agent 逐页回退。非 Alva engine 明确显示 unsupported，不伪造内部事件。

26. V1 验收矩阵在 `ACCEPTANCE.md`：18 条 scenario 逐条映射到可执行断言或明确标注为待打包运行时验证；外部链接经 `tauri-plugin-opener`（wry webview 里裸 `<a target="_blank">` 不生效）；worktree 隔离测试在缺 git 时**响亮失败**而非静默跳过（它们是"未 adopt 的工作不进共享 workspace"的唯一证据）。

## 约束
- **`EngineTarget.required_capabilities` 承载两套词表**，定义在 `readiness.rs`：`ENGINE_CAPABILITIES`（5 词，`capability_labels` 的穷尽产出，降级 veto 只比对这些）与 `MODEL_CAPABILITIES`（`images`/`vision`/`tools`，provider 组装读回用）。新增词汇改那两个常量，并保持 `capability_labels` 与 `ENGINE_CAPABILITIES` 同步（有测试钉死）。两表之外的词由 `validate_engine_target` 拒绝——引擎永远无法"提供"模型词，把它算进 missing 会让降级目标被永久静默否决。

- 自动 fallback 只消费当前 session policy snapshot 中显式授权的 target；无授权则 Turn 直接失败。
- canary 测试钉死敏感字段不达另一 EngineRuntime。
- 未分类（Other）失败与成功但质量不佳的结果都不触发自动 fallback。
- 不复用 `alva-app-tauri` 的 Rust backend 或 session database。
- raw adapter stream 不直接泄露给 UI；产品运行态先投影为 timeline vocabulary，调试态只通过稳定 Hub Inspector DTO 暴露 durable typed records。
- SQLite 路径来自 Tauri app-data；目录/文件私有、拒绝符号链接、写入走事务，并以实例锁保护启动修复。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Product seam | `src/run.rs` | 动态 engine Hub 运行、完整 handoff 注入与 shared-clock timeline 投影 |
| Engine assembly | `src/alva.rs` | 为 workspace 构造 ALVA、Claude Code、Codex 或 Pi adapter，并投影统一第三方 ProviderConfig |
| Inspector bridge | `src/inspection.rs` | 共享 Alva SessionRepository、校验 product/native session 归属并投影稳定递归 Inspector DTO |
| Engine readiness | `src/readiness.rs` | 只读 probe、结构化 readiness 分类与 dashboard DTO |
| SDK installation | `src/sdk_installation.rs` | Hub 受管 SDK 入口校验，与 task workspace 隔离，由 assembly/probe/veto 共同消费 |
| Permission contract | `src/permissions.rs` | session PermissionProfile、adapter 映射资格与 PermissionRouter |
| Provider plane | `src/provider_plane/` | 版本化模型身份、显式 binding 校验、per-engine override 与无 I/O 路由规划 |
| Provider catalog | `src/providers.rs` | Provider 连接配置、backend-validated model binding、私密凭证与 route DTO 投影 |
| Session workspace | `src/session_workspace.rs` | owner-only 独立会话目录创建与 project/session workspace kind |
| Workspace files | `src/workspace_files.rs` | 会话 cwd 绑定的真实只读目录/文本预览，敏感路径及符号链接隔离、有界响应，Tauri/HTTP 同构 |
| Workspace Git | `src/workspace_git.rs` | 真实工作树只读 status/diff，绝不以 EngineBranch 或工具结果推断 Git 状态 |
| Session store | `src/store/` | 私有 SQLite（`db.rs` storage kernel + `mod.rs` 共享数据层 + `branch.rs`/`handoff.rs`/`team.rs`/`turn.rs`/`tests.rs` 持久化簇）covering workspace kind、policy snapshot、branch/switch/handoff/timeline/Turn/Attempt 持久化与恢复 |
| Message recovery | `src/store/message_journal.rs` | schema 20 可见 assistant 全文与有界游标补页，复用消息位置协议并对齐崩溃修复终态 |
| Tauri entry | `src/main.rs` | workspace 创建、运行/切换/Inspector IPC、event emission 与 Desktop runtime |
| Crate facade | `src/lib.rs` | 对 Tauri 层和测试暴露产品类型 |
| Web frontend | `web/` | React/Vite/TypeScript workspace、composer、三泳道 timeline 与递归 Inspector 页栈 |
| Desktop config | `tauri.conf.json` / `capabilities/` | window、build 与最小权限配置 |
| Presentation assets | `icons/` | walking-skeleton 暂用的 Alva 图标 |
| Acceptance matrix | `ACCEPTANCE.md` | 18 条 V1 scenario → 证据映射与剩余缺口 |
