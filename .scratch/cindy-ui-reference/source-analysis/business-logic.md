# Cindy 业务逻辑考古（源码审计）

> 审计对象：`makecindy/cindy` @ `240247e52e938710f409c921a7a393097dcef52b`，主要覆盖 `packages/*` 与 `apps/desktop/src/main`/preload/服务编排。
> 
> 确认级别：**[静态确认]** = 实现代码可直接证明；**[动态待验]** = 实现存在，但需真实服务/系统环境验证；**[推断]** = 从多处静态证据得出，已显式标注。未标注者默认为静态确认。

## 1. 产品边界与核心对象

Cindy 桌面端是 Electron 宿主。Renderer 只通过 preload 白名单 IPC 访问能力；账号凭据、数据库、Agent 实例、定时器、远程连接、终端和插件子进程都在 main 进程持有。产品中的“任务”对应持久化 `sessions` 记录；一次模型运行是 turn；Agent 运行时会话和产品任务的 active/archived/deleted 状态是两层独立生命周期。

- 任务数据源：`apps/desktop/src/main/localDb/schema.ts` 的 `sessions`/`messages`。
- 运行时管理：`packages/maker-core/src/maker.ts` 与 `packages/maker-core/src/session.ts`。
- 统一事件面：`packages/maker-core/src/types/events.ts`。

## 2. 账号、本地模式与数据边界

### 2.1 启动与进入应用

- **[静态确认]** 应用会话只有 `signed-out | local | cloud`。云模式启动时先按 signed-out 处理，直到服务端 membership 验证成功；只有 local/cloud 能进入主应用。证据：`apps/desktop/src/main/appSessionState.ts`、`authStartupGate.ts`。
- **[静态确认]** 本地模式使用保留 owner ID，数据根为 owner 哈希目录；无 owner 时使用临时命名空间，不落到共享根目录。边界每次换 mode/owner 都递增 generation，异步写入必须校验 scope key，防止旧账号结果穿透。证据：`appSessionState.ts`。
- **[静态确认]** 进入 local 会取消浏览器登录、清理 provider secret cache 与认证状态，再提交 local 边界并广播；退出 local 回 signed-out，本地数据保留。证据：`apps/desktop/src/main/authManager.ts`。
- **[静态确认]** 启动恢复优先级是 local mode → 已缓存云账号快路径 → 被动注销 tombstone/更新后重登标记/旧数据迁移 → 冷启 refresh。冷启 refresh 去重且不会被 UI 超时取消；UI 可先以 signed-out 结束 splash，晚到成功仍会广播。auth epoch 丢弃登录/退出前的过时结果。证据：`authManager.ts`、`authStartupGate.ts`。
- **[静态确认]** 明确失效凭据才会清除；网络错误、429/短暂故障保留 token，但当次从 signed-out 开始。Renderer 不取得 token，token 由 main 进程 safeStorage/内存持有。证据：`authManager.ts`、`apps/desktop/src/shared/authIpc.ts`。

### 2.2 登录流与账号删除

- **[静态确认]** auth-client reducer 的可见步骤为 identifier、realm-confirmation、method-choice、verification-code、browser-redirect、account-selection、sso-verification、binding、completed 或 error；error 指定 `recoverTo`。证据：`packages/auth-client/src/types.ts`。
- **[静态确认]** 删号 challenge/status/receipt 均由 main 持有；确认删除后立即本地退出，未登录态继续用 receipt 跟踪服务端结果。证据：`authManager.ts`。
- **[静态确认]** owner 级集成只在账号 DB 准备完成后启动，回调再次校验当前 owner，防止切换账号时旧启动穿透。证据：`accountIntegrationStartup.ts`。

## 3. 项目、工作区与 Git Worktree

- `workspaceKind` 区分 project/dialogue，`workingDir` 与产品项目身份解耦。dialogue 任务默认工作目录是 `<owner>/dialogues/YYYY-MM-DD/<sessionId>`。证据：`localDb/schema.ts`、`dialogueWorkspace.ts`。
- 对旧 dialogue 目录采用惰性自愈与启动扫描：只重写严格匹配的老根；原路径存在时先复制内容，再更新记录；复制失败保留原值；远程记录排除。证据：`dialogueWorkdirSelfHeal.ts`。
- 项目稳定 key 按 local path / remote host+path / device+path 生成；Windows 本地路径比较 case-fold。证据：`apps/desktop/src/shared/projectKeys.ts`。
- `.cindy/project-knowledge` 是项目知识数据源，Markdown/frontmatter 由 commit 驱动刷新；新建运行时会话时注入快照。证据：`packages/project-context/*`、`apps/desktop/src/main/maker-ipc/projectContextInject.ts`。
- Worktree 源数据是 owner 级 `worktrees.json`，`sessions.worktree_path` 是反规范化索引。产品任务 archived/deleted 才触发回收，单纯运行时 close 不回收。脏 worktree 删除前自动 stash，并校验 live ref/安全边界。启动回收只处理 deleted/已丢失，不回收 archived。证据：`apps/desktop/src/main/worktree/WorktreeManager.ts`、`worktreeStore.ts`、`sessionRemovalRecycle.ts`。
- 同一 base repo 的创建串行；远程/移动端 recovery-key 按 session 串行；Scheduler 可使用短命工作树池。证据：同上及 scheduler host worktree 适配。

## 4. 任务、消息、输入队列与恢复

### 4.1 Agent 会话

- Maker 按 AgentKind 创建/恢复 Claude Code、Codex、Pi 实现，并以 CAS 保存 vendor session ID。runtime close 从内存 map 删除，不等于数据库 archive。证据：`packages/maker-core/src/maker.ts`。
- Session 状态为 active/aborting/closed/error。`send()` 必须取得 host turn lease，每次仅有一个 reservation；会话忙则拒绝。发送前取消返回 `accepted:false`，只有 agent handle 确认后才算越过 dispatch 边界。证据：`packages/maker-core/src/session.ts`。
- 统一事件包含 text/thinking/tool use/result/full result/subagent update/image/account usage/turn diff/interaction/plan/status/compact/session id/done/error。错误终态优先使用 `isTerminal`，否则 `!willRetry`，旧未知错误按终态处理。证据：`packages/maker-core/src/types/events.ts`。
- 没有 interaction listener 时 fail closed：权限拒绝、问答返空、计划拒绝。无事件 45 分钟触发 turn stall watchdog，等待人机交互或存在后台任务时不计时；provider 调用有 30 分钟看门狗。abort 后 grace 内 vendor turn 仍未结束则关闭 session，下次 send 惰性重建。证据：`session.ts`。

### 4.2 可持久输入队列

- 队列要求“先入队、只排头、接受后持久化”；只有 `handle.send` 返回 accepted 才标记已派发。steer marker 阻塞 drain，Maker 接受 steer 后才持久化，失败时保留/回滚。证据：`apps/desktop/src/main/maker-ipc/agent-input-coordinator.ts`。
- Stop 保留待处理项并暂停；close 取消 steer marker，在锁释放后继续 drain。崩溃后从 SQLite 快照惰性恢复为 paused，需明确继续；损坏/不合法快照丢弃。证据：同上与 `localDb/schema.ts` 的 `agent_input_queue_snapshots`。
- Retry 不盲目重发：未产生输出时克隆已接受 turn；已有部分输出时使用 continuation prompt。自动恢复有标记、不重复计费，用户接管会撤销。大快照会清理并移除 inline base64。证据：同上及相关 local DB DAO。

## 5. Harness/模型/路由切换

- **[静态确认]** 跨引擎切换是 deferred intent：当前 turn 不被打断，下一次 send 前一次性应用。流程为验证目标、寻找已停放 vendor session ID、生成确定性 handoff、关旧运行时、以 DB 事务提交 agent/model/provider/effort/fast/sdkSessionId、写 agent_switch 边界，再恢复/创建目标。DB 是 commit point；边界写失败仍保留内存 handoff，创建失败下次惰性重试。证据：`sessionAgentSwitchHandler.ts`、`agentHandoff.ts`。
- 切回原引擎时用最新 boundary 的 `fromSdkSessionId` 与增量 handoff；无法恢复则原子清除 vendor ID 并用 full handoff。远程任务和 Orca 不支持此流程。intent 仅内存保存，重启后丢失是显式可接受行为。
- 运行时模型切换先更新 route，`setModel` 失败则回滚；忙时的 credential-family 跨路由更换延迟到 turn boundary。Codex route 不在 turn 中切换。`provider_id` 是权威路由，null 才做旧数据推断。证据：`runtimeSetModel.ts`。
- effort 切换超时 3.5s，随后 1s 内终止运行时，下次 send 重建。证据：`runtimeSetEffort.ts`。

## 6. Pi Harness

- **[静态确认]** Pi 以 `pi --mode rpc` 子进程运行，stdio JSONL。每会话有 `models.json`；密钥从 env 注入，不写 config。宿主 prompt 以 append 方式保留 Pi 默认 prompt。证据：`packages/maker-core/src/agents/pi/index.ts`、`docs/dev-rules/pi-harness.md`。
- 注入 `cindy-bridge` 拦截工具调用，并动态读权限 JSON。permission 模式顺序 ask/auto/bypass，auto 评审异常时 fail closed。“Full Access”不等于 OS 沙箱。
- 内置 MCP 通过 localhost HTTP；自定义 MCP 仅允许 HTTPS 或 loopback。会话创建时快照，不在运行中动态换工具。plan extension 将状态回镜给宿主。
- Pi 支持运行时模型更换，但 route/credential family 不相容时关闭会话，下次重建；支持 resume 文件/CAS 与自动 compact。

## 7. 权限、人机交互与计划

- 权限审批、问题和计划评审共用 `interaction_request` 通道。Router 状态是 waiting/resolved/cancelled，呈现面可为 desktop/channel-card/headless。无 resolver 拒绝，不自动放行。证据：`packages/maker-core/src/session.ts`、`apps/desktop/src/main/maker-ipc/interactionRouter.ts`。
- plan 是与 permission mode 正交的布尔态；`plan_mode_changed` 会持久化。auto 权限由 `maker-host/auto-permission-reviewer.ts` 与共享策略审查，异常 fail closed。
- 插件 Full Access 豁免是 runtime/session-instance/live-local 级别，不按可伪造的业务 sessionId 盲信；条件不足拒绝。

## 8. Orca 多 Agent 协作

- 一个 Lead 对应一个完整 Maker session，每个 Worker 也是完整 session。本地支持 Claude/Codex/Pi；SSH 只支持 Claude/Codex，Pi 远程明确不支持；device-link 数据真值在被控端。证据：`packages/orca-workflow/*`、`apps/desktop/src/main/orca/*`。
- DB 部分唯一索引保证每 Lead 仅一个 active team，每 team 仅一个 focused worker。Worker 状态 idle/running/done/error，Team 状态 active/completed/cancelled/failed。证据：`localDb/schema.ts`。
- `cindy_orca` MCP 提供启动/结束 team，创建单个/批量 worker，列表/聚焦，发信/编辑/取消排队消息，idle/archive，模型/工作区查询，状态和读取。Worker→Lead 使用独立 bridge。工具可全局可见，但 handler 会用角色/team 二次授权。Codex 依赖可证明来源的 `_meta.threadId`；未知调用者或 batch 丢失元数据均 fail closed，`mcp-session-id` 不作身份。证据：`packages/orca-workflow/src/orca-bridge-mcp.ts`、`apps/desktop/src/main/maker-host/orca*`。
- Worker 不继承 Lead 权限默认。用户偏好在 renderer localStorage，main 保持镜像；初始是 Full Access，明确从 auto 改 bypass 需用户确认，已保存 bypass 不重复弹窗。
- 创建有硬/软上限、label 唯一、短租约 reservation，route 必须精确且失败闭合；远程 Worker 继承 remoteHostId，批量是顺序创建。创建后 idle，首次接受派发转 running，派发失败回滚，终态 done/error 并自动 bridge Lead。idle 会释放运行时但保留逻辑 Worker；archive 关闭 session；end team 终止并 archive。
- **[静态确认]** 当前 Worker 不在 turn 中切模型，effort 可调；“side activity”是未实现未来能力，当前 side-chat 只是普通 fork，本报告不把它当成已有功能。

## 9. Scheduler/自动化

- Schedule 状态 active/paused/expired；run 状态 running/success/failed/aborted/interrupted/skipped。运行时 phase 是 loading/claiming/persisting/running/queued/cancelling/finalizing，不持久化；`attemptLifecycle.ts` 是唯一转移权威，非法转移抛错。证据：`packages/maker-scheduler/src/types.ts`、`engine/attemptLifecycle.ts`。
- queued 表示尚未持有 Agent/MCP/token，不占并发额度。Schedule 支持 cron、手动 run-only、interval/recurrence、Agent 路由/模型/effort/Fast、project/dialogue 工作区、worktree、持久/临时 target session、silent idle、agent 或 script runner、pre-run hook、桌面/Feishu/WeCom 通知与过期。证据：`types.ts`、`apps/desktop/src/main/scheduler-host/index.ts`。
- pre-run hook 退出 0 运行，2 为 skipped 且不读 token，其他退出/超时/丢失都 fail closed。script capability 默认拒绝，当前显式能力是 jira.read/jira.comment/sessions.dispatch/feishu.read。silent 成功不通知，除非 Agent 调 `notify_current_run`；失败仍通知。
- due trigger 用 `nextFireAt` CAS claim，run 有 heartbeat；启动清理将旧 running 转 interrupted，且排除当前进程在跑 ID。同 schedule 用锁串行。被动 dev 模式关闭自动 fire，但 CRUD/runNow 仍可用。证据：`apps/desktop/src/main/scheduler-host/storage.ts`、`apps/desktop/src/main/scheduler-host/index.ts`。
- 目标任务忙可 defer，不创建 run row，稍后短间隔重试。持久 target archived 时清绑并重建，非持久 target 暂停 schedule。review session 不允许做 target。崩溃依靠 heartbeat/stale sweep 收敛为 interrupted。

## 10. 插件、Skill 与 MCP

### 10.1 Ghost 插件

- `ghost.json` schema v2，ID 严格且路径安全。能力 slot 包括 subscribe/tool/card/panel/cindy/agent/node/network/notify/badge/confirm/fs/session-context/pick/preview/skill/workspace/ios-simulator，launch 可 on-demand/resident，panel 可 left/right/tab。证据：`packages/plugin-protocol/src/manifest.ts`。
- Agent 工具是创建 session 时的声明式快照，不中途注册。Node worker 只能启动 package 内已声明 JS entry，无 command/shell/env 注入；它以当前用户权限运行，不是 OS 沙箱。与宿主用 JSON-RPC/MCP stdio，secret binding 按 method 声明且由 safeStorage 注入。证据：`packages/plugin-protocol/src/manifest.ts`、`apps/desktop/src/main/cindy-brain/nodeRuntimeBroker.ts`。
- Runtime 状态 off/starting/running/stopping/crashed/fused；60 秒内 3 次 crash 进 fused，重新启用显式 reset fuse，stop 幂等。证据：`apps/desktop/src/main/cindy-brain/runtime/GhostRuntime.ts`。
- 安装 ledger 按 owner 隔离，同插件安装串行；下载、包和 provenance 都验证，新增权限必须显式 review，commit 前重验字节/来源/owner。默认插件会对齐安装/升级，显式 opt-out 防止再装；市场源删除不删本地包。证据：`apps/desktop/src/main/plugin-market/service.ts`、`install.ts`。

### 10.2 Skill/MCP

- bundled skill 链接到 `~/.agents/skills/<id>--<name>`，对所有会话生效，不在沙箱；禁用/卸载才移除。SkillHub 扫描 global/project scope 及 cc/codex/pi 引擎，按 realpath 去重。自动同步只在登录后、每用户一次，且仅明确服务端白名单；当前代码默认白名单为空。用户 ignore 受尊重，不覆盖用户 global/冲突 Claude skill；取消清理会持久并重试。证据：`apps/desktop/src/main/skillhub/*`。
- 自定义 MCP 配置放 DB，token 放 safeStorage；远程仅 http/sse 且 URL 必须 HTTPS/loopback，ID 路径安全并保留内置 ID，header 值限字符串。CRUD 刷新 provider、广播 MCP_CHANGED、尽力失效 Codex cache/app-server；Claude 在下次 session 生效，Pi 用 session-create 快照。证据：`apps/desktop/src/main/maker-host/custom-mcp-store.ts`、`apps/desktop/src/main/mcp-integrations/custom-mcp-registry.ts`、`apps/desktop/src/main/mcp-integrations/custom-mcp-provider.ts`、`apps/desktop/src/main/localDb/schema.ts`。
- MCP 调用上下文区分业务 `sessionId`、不透明运行时 `sessionInstanceId` 与可证明 caller provenance。控制工具不应相信工具参数里的 session 自报。证据：`packages/maker-core/src/interfaces/mcp-provider.ts`及宿主适配。

## 11. IM、Hook 与个人微信

- IM host 接入 Feishu、Discord、个人微信、Telegram、钉钉、企微。凭据 owner-scoped，IPC 校验 trusted renderer；媒体使用内容寻址缓存和 owner pinning。证据：`apps/desktop/src/main/im/host.ts`。
- 连接启停与 work queue 串行化，generation 使登出前队列任务失效；start 失败不会阻断后续 stop。Binding 先落 DB 再更新 O(1) 内存索引，身份是 channel+bot+user+scope(thread)；启动会确定性修复旧的多重绑定。证据：`im/connectionLifecycle.ts`、`im/binding.ts`。
- Hook WebSocket 连接内部状态 connecting/connected/standby/error/stopped；必须 hello→welcome 才 connected，ping/pong + 65s idle watchdog，1–30s 指数抖动重连，401 只强制 refresh 一次。duplicate device 进 standby 低频探测。证据：`apps/desktop/src/main/hook-control/transport.ts`。
- Hook 任务 ack 是 accepted/queued/rejected，拒绝原因包括 unknown_workspace/workspace_not_allowed/session_not_found/disabled/invalid。路径 alias 是唯一目录白名单；复用 session 每条消息重验 live working dir，移动/删除映射会丢绑定并重建。忙任务 FIFO 且 ack queued，终态 drain。`connectionId+requestId` 去重会重放 ack 但不重跑。progress 只保最新快照，turn.end 离线持久/重放，重开 turn 例外。新 session 权限 override 校验，默认 bypass；复用 session 以 DB meta 为准。证据：`apps/desktop/src/main/hook-control/dispatcher.ts`、`apps/desktop/src/main/hook-control/requestLedger.ts`、`apps/desktop/src/main/hook-control/session-runner.ts`。
- 个人微信 phase 为 disconnected/authorizing/waiting_confirmation/connected/reconnecting/needs_reauth/disabled_by_policy/error。任务 TTL 30min，租约 60s；空闲可从 pending 进 dispatching/accepted_running/waiting_desktop/delivery_pending/completed，忙时释放回 pending，超时进 expired，过载非控制消息进 rejected_overload。outbox 是 pending→sending→delivered，可回 pending 重试或 failed_terminal；全部送达后任务 completed。证据：`apps/desktop/src/main/localDb/worker/opHandlers/wechatTx.ts`及 `apps/desktop/src/main/im/wechat/WechatIM.ts`。

## 12. Device Link

- Client 状态 stopped/connecting/online；relay 需 connect→hello/ack→online，20s ping，丢 pong 重连，1–30s 指数退避。可靠传输对每 peer 做 ack/replay/backpressure，invoke/link-open 有配对和超时，错误保留 `inFlight` 不确定性。证据：`packages/device-link/src/client.ts`。
- 一个 owner DB 行用 CAS + 5s heartbeat/15s stale 实现单主，其他进 standby。store 不可用时当前 owner 先自降级，再允许 takeover；单纯网络断开不让出 owner。证据：`apps/desktop/src/main/device-link/ownership.ts`、`apps/desktop/src/main/localDb/schema.ts`。
- 离线 push 队列只在内存，最多 128 项/512KiB/5min；显式 state channel 合并，event 保序，不是持久化真值。`transport-timeout` 只在 online owner、本地控制启用且有 outbound intent 时最多 5 次重开；永久 close 取消。证据：`packages/device-link/*`、`apps/desktop/src/main/device-link/*`。
- 远程 IPC 严格白名单，是本地 preload 能力的真子集；本地设置、secret 修改等不会透传。证据：`packages/device-link/src/allowlist.ts`。

## 13. 浏览器、终端和文件

- PTY 生命周期属于任务/tab/main；create/attach 幂等，exit 保留记录可用同 ID restart，dispose 才 kill。write 在 microtask 批处理，owner window 销毁时转给 fallback 窗口，无 fallback 才 dispose。子进程 env 剥离 terminal integration 和继承的 secret-like 变量。证据：`apps/desktop/src/main/terminal/ptyManager.ts`。
- File browser 支持 local/SSH/device，list/read(≤2MiB)/write/create/rename/delete/stat/watch。scanner 统一拒绝 traversal/symlink 逸出。文本搜索用 ripgrep fixed-string + 文本白名单，包含 hidden 但遵守 `.gitignore`，有匹配上限；取消 SIGTERM，200ms 后 SIGKILL。证据：`packages/file-browser-core/*`、`apps/desktop/src/main/file-browser/*`。
- SSH 大文件用 chunk cache，device 通过 export→OSS→download，deadline 30min 且短暂错误轮询；可回退 stale cache。SSH 聊天文件超出 working dir 拒绝，device 超出范围用 media fetch；exact stat 区分 file/directory/nonfile/unknown。
- Browser TabRegistry 在 main 是真值，renderer 持有 DOM/LRU。tab 在 dom-ready 后 report，崩溃自清理，stale release 用 webContentsId 防错杀，pin lease 使自动化 tab 免于回收。证据：`apps/desktop/src/main/rsb-browser-bridge/registry.ts`。
- 30s 资源 watchdog：后台内存≥1GiB 回收；后台 CPU≥50% 连续 4 次回收（audible 例外）；前台内存≥2GiB kill+通知；前台 CPU≥90% 连续 4 次告警且有 cooldown；pinned 免除。共享 PID 聚合 pin/foreground。

## 14. Usage/计费

- Claude 累计 model usage 按 turn 做 delta；计数器回退视为 reset，负值 clamp。Vertex 的已知 output 可晚一 turn 结算，只抑制 timing 噪声，不抑制账务。证据：`apps/desktop/src/main/usage/modelUsageDelta.ts`。
- 计费先按权威 route 区分 xd gateway/provider-api/subscription/unknown。subscription 不计 actual money，可作 value estimate；gateway 用 quote/discount/currency；BYOK SDK USD 投影到 ledger currency；DeepSeek 用 token delta/参考价。数据明确区分 actual 和 estimated。证据：`turnCostCalculator.ts`。
- 日消费/模型用量落 `daily_spend`/`daily_model_usage`；session/run 归因标注 exact/direct/mixed/zero/unavailable/legacy，不把不可归因成本伪装成精确。证据：`localDb/schema.ts`及 usage DAO。

## 15. 更新与遥测

- Updater 状态 idle/checking/downloading/ready/superseding/error。下载更新版时保留旧 ready patch；新版成功替换，失败回退旧 ready。`patch-info` 保存 version/file/hash/relogin/applyAttempts；当前版本已匹配则删除，应用失败≥3 次放弃。证据：`apps/desktop/src/main/updateService.ts`。
- 首次检查约 10s，之后 30min；manifest 超时 8s。Linux 只手动，0.0.0 禁用，macOS translocation 拒绝。进度单调归一化。普通自动重启需 packaged+ready+无忙任务+60s 安静期+resume 后 60s+锁屏/空闲且 idle≥10min；DB/schedule 忙查询失败则不重启。启动应用 patch 的 gate 较轻，但 boundary 会复验。重启前停 ADB/iOS/Ghost node 并写 expected-exit marker。
- 遥测有持久 `privacyConsentAccepted` 与 `analyticsEnabled` 两个独立开关。只在 packaged（或严格 dev opt-in）+同意+开启+非 local 时上报。local 不改持久同意，只在运行期 opt-out。旧同意迁移只在冷启确定存在认证时执行；local/确定无认证封闭窗口，短暂/含糊凭据保留窗口，设置损坏 fail closed。证据：`apps/desktop/src/main/analytics-settings-store.ts`、`apps/desktop/src/main/analyticsSettingsService.ts`。

## 16. 持久化、迁移与错误恢复

- 账号 DB 路径按 `<prefix>-<user>.db`隔离，换账号会关旧库/开新库。SQLite WAL 提供崩溃恢复，退出不做全量备份。证据：`apps/desktop/src/main/localDb/index.ts`。
- `SQLITE_CORRUPT` 优先恢复旧 `.bak.clean`，再用最新迁移 `.bak.<ISO>` 并 toast；无备份为致命错误。迁移前用 online backup 且权限 0600，有磁盘预算；先 schema migration，后 vec 能力。
- 跨进程 reader/writer lease：被动 reader 不得写；packaged 可以只读打开精确兼容 schema，禁止 downgrade。stale lock 可按 PID 清理，lock 本身损坏则 fail closed。schema drift 检测；dev 可备份后 repair，release 只记录/提示。
- 数据库 worker 只在 smoke test 通过后 takeover，否则回退 in-process；worker error boundary 自动重启一次。启动还修复管理的 dialogue 目录和孤儿 Orca 记录，并做优化。

## 17. 静态审计无法确认的部分

- **[动态待验]** 真实 auth/realm/SSO/删号、云同步、插件市场签名链、各 LLM provider 事件完整性、IM/Hook/WeChat 上游协议行为、Device Link relay 端和自动更新签名/替换需连真实服务验证。
- **[动态待验]** macOS/Windows/Linux 的终端、资源 watchdog、safeStorage、锁文件和 patch relaunch 依赖 OS 语义。
- **[推断]** 从 owner generation、DB 重开、owner-scoped ledger/credential store 的共同设计可推断“账号切换是全局运行时隔离边界”；但要证明所有组件都无漏清理，仍需带故障注入的端到端测试。
- 未读取 `.env`、钥匙串、token/凭据存储、用户消息正文或真实账号 DB；本报告仅审查源码和 schema/migration 结构。
