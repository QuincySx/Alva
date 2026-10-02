# alva-hub-desktop/src
> Hub Desktop 的 Rust 产品控制器与 Tauri 装配。

## 地位

位于 L7 Desktop 产品边界。业务执行只依赖 Hub/engine contract，具体 engine
装配和 Tauri IPC 保持在产品入口。

## 逻辑

ALVA 能力/MCP 配置是 `store/tool_settings` 的 schema-22 全局 snapshot，每轮由 Desktop/server 经共同 blueprint 装配注入，其他引擎忽略；安全 substrate 不可关闭，不热改当前 Agent、不加载外部 MCP/AEP。新增/编辑 MCP 重新禁用，启用需确认宿主访问；`mcp_diagnostics` 仅确认后绑定已存 session cwd 做一次真实连接/目录测试并回收，不调用模型。

1. `alva` 按用户选择为 workspace 构造 ALVA、Claude Code、Codex 或 Pi adapter；API-backed route 从统一 ProviderConfig 投影各 adapter 的协议配置，Claude/Codex 原生订阅仍只归各自 CLI 使用；`provider_plane` 先按版本化模型声明与纯 RoutePlanner 精确求出 provider × model × engine 能力，再投影到四种 adapter 各自入口，未知模型不猜测能力，Fast 只投影到声明支持它的 Codex route `service_tier`。
2. `store` 在私有 SQLite 中原子保存 HubSession（含 project/session workspace kind）、global/session FallbackPolicy snapshot、多 EngineBranch（含 reasoning effort/Fast）、watermark、switch、handoff、timeline、Turn 和 Attempt；同 route 调参更新原 branch，不制造伪切换。
2.1. `session_workspace` 只在用户明确选择“独立会话”后，于 Desktop app-data 创建 owner-only 专属目录和受管标记；项目模式继续使用用户选择的 Workspace，受管目录不隐式清理。
2.2. `workspace_skills` 只列出 session 已存 cwd 内的技能文件，复用协议 parser 与 capability-relative no-follow。生态源只读；新建/编辑仅 `.alva/skills/user`，不覆盖创建目标，保存需匹配源 revision；macOS/Linux 归档/恢复用原子 no-replace 移动，保留资源。已有 enable-list 的全局名字及未知字段保留，损坏/超限配置拒绝创建，不热改运行中 Agent。
2.3. `store/automation` 持久化暂停优先的间隔配置与 revision；启用只能确认已展示的 branch/EngineTarget，路由变化需编辑后再授权。`run_persisted_with_automation` 与 resumed Turn 同事务认领，自动化不走 fallback、TeamRun 或独立自动审批。入口每 10 秒查询 due、最多 4 个并行；busy/missed tick 延后、成功后等完整间隔、失败/Stop 暂停。启动重置未执行的钟，中断执行暂停，不补跑；隔离 QA 可保存暂停配置但不能启用，pump 关闭。
2.4. schema 23 Turn 附件保存原始 BLOB 与 session cwd 中的新副本，每文件 20 MiB/每 Turn 40 MiB/8 文件。`run_persisted_with_input_attachments` 仅给模型文件引用/处理清单，user_message 保留原始指令；保存失败使 Turn 明确失败。no-follow/create-new 不覆盖项目文件，上传只做签名探测、不自动压缩/转码/抽帧/分析；Tauri/HTTP 预览以 session JOIN 校验归属。共同 `engine_wiring` 为 ALVA factory 接同一个 native repository：通用大结果/工具图片/MCP 调用结果投影引用、受管 MCP 单 gateway、固定 native session 检索；递归 child 通过 app-level policy 重绑自有 reader/assembly/middleware，不读父历史、不恢复 root host 工具。UI/Inspector 原记录不变。
2.5. 认领后的 HubStore/native-root 配方提供 hub_context，检索当前产品会话已存跨 branch/engine 可见记录/失败原因，并显式读取文字附件；不扫全局/其他会话/Worker/父 scope。hub_media 使用固定安装 decoder、原始 BLOB 的只读 no-follow FD 快照、限额/取消与既有 HITL，schema 24 独立保存派生图/指定或均匀视频帧/短静音视频与处理清单。只有声明 image 能力的 route，显式选中的 1–2 派生 JPEG 才附于一次压缩后的模型请求副本；receipt 保存 actual model/backend/来源 hash/真实帧点和尝试/响应/未知接受状态，原件及 conversation 引用不变、不自动重放。非 OS 沙箱、无子 Agent 产品媒体授权，外部 engine 循环/派生预览 UI/真实 provider 验收未覆盖，详见 docs/ALVA-HUB-CONTEXT-RETRIEVAL.md。
3. `run` 创建或恢复 active branch 的 product/native session 映射；parked return 注入 delta，native resume 失效时原子升级完整 handoff 再 fresh 执行。
4. SessionStarted/ToolEnd 增量落盘，Completed 原子提交 Attempt usage 与 Turn 终态；Alva adapter 的真实 ToolExecution duration 穿透到 side-effect evidence。
5. 运行中 switch 持久化为 queued；终态或启动 crash repair 后再原子应用。
6. runtime events 与 switch facts 被投影为稳定 timeline states；每个 TimelineEvent 在产品创建点取得统一 `created_at`，所有 engine 共用一个产品时间轴，UI 不读取 adapter-native clock。
7. `main` 从 Tauri app-data 初始化 store，并暴露运行、切换、列表和详情 IPC。debug-only `--isolated-ui-data <absolute path>` 使用独立数据与临时主 WebView，关闭共享 Provider/账号探测、worker 恢复及 engine/账号操作；专用于无真实账号的 UI 验证，正常启动路径不变。
7.1. `inspection` 持有与 Alva adapter 共享的 `SessionRepository`，按 HubSession/EngineBranch 校验 native session 必须是根或可达子孙，再把 `EngineInspector` snapshot 投影为稳定 `HubInspectionSnapshot`；`main::get_hub_inspection` 只暴露该 DTO，非 Alva engine truthful unsupported。
8. `main` 暴露全局与单 session fallback policy IPC；自动使用必须来自当前 session snapshot 的明确授权。
9. `readiness` 用只读 `EngineProbe` facts（PATH 发现、`--version`、凭证存在性、attempt 历史）分类 ALVA/Claude Code/Codex/Pi 的结构化 EngineReadiness；target-aware fast veto 对 API-backed CLI route 仍要求 binary，但不再错误要求该 CLI 的私有订阅登录。
9.1. `provider_diagnostics` 只消费后端解析出的 ProviderConfig：按协议发现远端模型，连接测试走 canonical provider factory 的最小真实 inference；跨 IPC 只返回模型标识、耗时和脱敏分类，不返回 URL、响应正文、样例、密钥或 header。
10. `run` 以 engine factory + health veto 装配执行；`failure` 矩阵分类 terminal 失败，仅 operational continuity class 触发 `fallback_failed_attempt`：同一 Turn 原子转移到下一授权 target（parked source + 预持久化 full handoff + 串行 Attempt + `engine_fallback` fact），target 同步启动失败时全部状态可恢复。
11. fallback 候选按 policy 顺序、三元组去重、一 cycle 一次；health veto 或 required_capabilities 不满足 → skip + `engine_fallback_skipped` fact；链路耗尽 → `fallback_exhausted` fact 汇总全 cycle 终态。
12. stall watchdog 仅在无 open tool、无 pending permission 时计时，任意事件重置 180s 窗口；确认 stall = `FailureClass::Stall` continuity failure + Failed TimelineEvent + best-effort cancel，走既有有界 fallback。
13. `main` 维护 active Hub registry（session→live HubRuntime）；`stop_hub_attempt` 校验 running + 本实例 ownership，await 前固定 attempt，cancel 成功后仅对仍 running 的同 attempt 写 `attempt_cancelled` fact，再清审批。启动前无 native id 时 Hub abandon 会关闭 adapter stream；ALVA 随关闭中断 factory/MCP 组装。run seam 的无 Completed EOF 必须失败而非成功，且产品停止归 Other 不 fallback；正常终态/迟到 ack 不被取消事实覆盖，重启不重放。
14. `permissions` 定义 session 级 PermissionProfile（interactive/workspace_write/unrestricted，随 hub_sessions 持久化、branch 切换不变）与四 engine adapter 映射契约：等价或更严格才可执行（alva=交互回调、claude=acceptEdits/bypassPermissions、codex=sandbox 档位、pi=read-only/full tool policy），只能弱化的组合 ineligible 并带结构化原因；interactive 保证的是"raised request 由用户裁决"的路由（risk classifier 决定哪些调用 raise），不是每个副作用都询问。`alva::build_engine` 按映射装配并 fail-closed；fallback 候选先过资格闸门再过 health veto，skip 落 `engine_fallback_skipped` fact；手动 switch 同受此闸门（`request_engine_switch` 拒绝 weaken-only target，同 target 重选仍是 no-op）。
15. PermissionRequest 事件携带 request_id + emitting EngineSessionId 投影为 `permission_required` TimelineEvent；`main` 的 `PermissionRouter` 把用户决定路由回发起请求的那个 engine session（单次使用，attempt 终态/fallback/用户 Stop 后清空），决定不会泄漏到后续 fallback engine。SessionStarted 之前到达的请求无路由身份：card 不带控件、watchdog 保持 armed，死等以有界 stall 收敛；attempt 已带 terminal error 时 watchdog 恒 armed，pending approval 不能让 drain 无限阻塞。

16. `redact` 是所有跨 engine handoff 的唯一过滤闸门（`build_handoff_package` 持久化前执行）：credentials/token/敏感 `NAME=value`/PEM 全字段清洗，账号与账单标签等 adapter-private 元数据不出境（team_snapshot 例外，见 23：它是产品协作状态，过滤后随 handoff 出境）；hidden reasoning 与 approval token 结构性不入库。full transcript 按 session 开关（schema v7,默认关闭）附加且过同一过滤。

17. `repair` 把 readiness failure 映射为结构化 RepairAction：npm 官方分发的 claude/codex/pi 提供一键 install/upgrade（argv 由服务端按当前 readiness 重新生成，客户端只报 engine+kind）；认证/配置修复是最短官方指引不代跑；执行后重新探测恢复 eligibility；模块不接触 HubStore，pinned model/account/profile/binary 结构性不被改动。

18. team walking skeleton（schema v8）：TeamRun 恰有一个 lead session；worker 是 Hub-owned durable hub_session（继承 lead workspace/profile、快照全局 policy、自选 EngineTarget），非 engine-private subagent；WorkerTask 记录 scope/status/result/changed files（来自 side-effect 证据 path，ToolEnd 合并保留 ToolStart 的 path）/verification；worker 终态经 `finish_worker_task` 落 lead timeline `worker_outcome` fact + live Worker card；`main::execute_session_run` 是用户 prompt 与 worker dispatch 共用的执行通道。

19. 并行 worker 队列：每个 worker 独立持久 FIFO（status queued→running→终态/cancelled，`claim_next_worker_task` 原子领取并写 started_at，schema v9；证据窗口按 claim 时间而非入队时间，前一任务的 attempt 不会算进后一任务）；queued 任务可查看/编辑/取消，claim 后不可变；`add_team_worker` 支持多 engine 批次；`main` 每 worker 单 pump 消费（pumps set 防双泵，退出前 `has_queued_task` 复查防竞态遗漏），不同 worker 真并行、单 worker 串行；启动时为仍有 queued 任务的 worker 恢复 pump（重启不丢队列）；claim 时发 Worker 事件让 UI 及时看到 queued→running；interrupt = 对 worker session 复用 stop 机制（不清空队列，下一任务继续）。

20. `claims` 是共享工作区协调的冲突代数（path 含子树覆盖、read/read 不冲突、facility/exclusive_tool 全有全无、异类不争用）；mutating WorkerTask 入队时声明 ResourceClaims（schema v10 claims_json），`claim_next_worker_task` 在**任何 engine 启动之前**按同一 workspace 内 running 任务的持有集裁决：兼容并行、冲突则队首等待不重排（确定性顺序），blocked 时回报资源与持有者并落 `resource_wait` fact；in-product Alva worker 与外部 engine 走同一条闸门（是产品级调度，不是 kernel tool lock 本身——生产环境没有 provision `ToolLockRegistry`）。公平性：同 workspace 内更早的队首等待者会预留其资源，短任务流不能饿死它（holder 从不等待，故预留不会死锁）；claim 入队即校验（拒绝空/绝对路径/越界），路径归一用 kernel `normalize_path_string`。**已知边界：claim 只覆盖 dispatch 的 WorkerTask；用户在 lead 或 focus 到 worker session 后直接发 prompt 的那条路径不声明也不受闸门约束。**

21. `worktree` 管理隔离 worker：`git worktree add` 在 Hub 私有 root（不在 shared workspace 内）建独立分支工作树，worker session 的 workspace 指向它，故其 claims 与共享 workspace 天然不争用（schema v11 存 worktree_path/branch）。隔离要求 workspace 正是 repo 根（worktree 覆盖整个 repo，而 `git apply` 从子目录会**静默丢弃**目录外的改动）。worker 完成不自动合并；`adoption_preview` 只读列出变更/与共享工作区并发编辑的冲突/patch 是否仍能干净应用/验证证据，`adopt_into_workspace` 是唯一写共享工作区的入口且先 `--check` 再 apply；`discard_worktree` 三重守卫（canonical 路径必须在 Hub worktree root 内、分支必须 `hub/worker-*`、不得等于 workspace）——`git worktree remove --force` 会连同未提交改动删除**任意**同 repo 的 linked worktree，包括用户自己的；丢弃后把 session workspace 指回共享 workspace，durable session 与历史保留；两种决定都落 lead timeline fact。**adoption 是共享 workspace 写入且不持 claim：因此要求 worker 空闲（无 running/queued 任务）才允许，patch 生成用一次性 GIT_INDEX_FILE 不动 worker 自己的 index，adopted 文件名取自 patch 而非 `git status`。**

22. worker lifecycle（schema v12 `team_workers.lifecycle`）与 runtime 活性、任务终态、删除四者彼此独立：hibernate 释放 live runtime（cancel 在跑的 attempt + 摘 engine 注册 + 清 permission router）但保留 identity/history/queue，`claim_next_worker_task` 对非 active worker 返回 `TaskClaim::Dormant`（**不是 `Empty`**——pump 退出后按队列深度决定是否重启，若二者不可区分会对 hibernate 的 worker 无限重启形成热循环）；新任务自动唤醒 hibernated worker，archived 必须显式 restore；`complete_team_run` 结束时**归档**全部 worker 决不删除；`delete_worker` 是独立操作且只允许对 archived 执行，故"归档"永不被误当"删除"（注意它只删 worker_tasks/team_workers，session 与历史仍在，故未接入 IPC）。已结束的 run 拒绝再加 worker、拒绝重复 complete（否则会把用户 restore 过的 worker 又归档）。释放 runtime 时区分"本来就没在跑"（debug）与"cancel 真失败"（warn 且保留 engine 句柄，否则活着的进程再也停不掉）。重启只为 active 且有 queued 任务的 worker 恢复 pump。

23. fallback 天然局部于单个 session：`fallback_failed_attempt` 只动该 session 的 attempt/branch，TeamRun identity、其他 session 与全部 queue 不受影响，`engine_fallback` fact 落在真正 fallback 的那个 session（lead 或 worker）上。handoff 的 `team_snapshot` 携带协作上下文：lead 侧写"领导 TeamRun X、N 个 worker（各自 engine/lifecycle/队列深度）、worker 仍在独立运行"，worker 侧写"TeamRun X 的 worker、当前任务、剩余队列、已触碰文件、lead 与兄弟 worker 仍在运行"。故 `redact` **不再丢弃** team_snapshot（它是产品协作状态而非 adapter-private 元数据），改为与其他字段一样过滤。

24. TeamRun 权限：worker 继承 lead 的 PermissionProfile，且 `create_team_run`/`add_team_worker` 都过 `enforcement_for` 资格闸门——建 worker 永远不是绕开 profile 的路径。autonomous 是独立、可见、持久化的显式选择（schema v13 `team_runs.autonomous`，默认关）：**真正接线**——该 scope 的 PermissionRequest 到达即自动 Allow 并落审计 fact；它只决定 raised request 是否仍需人裁决，**不改任何人的 PermissionProfile**。`PermissionRouter` 的等价键是 (scope, tool, resource)：跨 team run/session 不合并；**resource 未知的请求各自成组**（`rm -rf` 绝不能搭 `ls` 的顺风车），resource 取 path 或 command 等字段。聚合卡保留每个 member 的 requester/action/resource，UI 按渲染时的 request_id 列表作答（防止点击瞬间混入没见过的请求）；`respond_many` 返回 answered/failures，部分失败向用户报错而非静默，逐个 member 落 `permission_decision` 审计 fact，member 单次使用不可重放。

## 约束

- 产品测试从注入 `EngineRuntime` 的最高层 seam 进入。
- `EngineSessionId` 不作为用户会话 ID。
- terminal error 不得再投影为成功状态。
- restart resume 只从持久化 workspace 和 native session 映射恢复，不信任 UI 覆盖。
- 同一 MainlineSession 的 running EngineAttempt 由 SQLite partial unique index 强制唯一。
- readiness probe 只读：不执行 engine run，不写 binary/model/account/权限配置；原因是结构化 reason code 而非 stderr 文本；capabilities 来自 adapter `RuntimeCapabilities`，不手写。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| Crate facade | `lib.rs` | 导出 run seam 与 DTO |
| Engine assembly | `alva.rs` | 统一 ProviderConfig 到 ALVA、Claude Code、Codex、Pi 的 route-aware 安全装配 |
| Product context Plugin | `hub_context.rs` | 已认领 HubSession/native-root 双绑定的跨 engine 可见历史与上传文本 Tool，真实 catalog 配方回归；子 Agent 不继承父产品数据授权 |
| Selected media | `hub_media.rs` | 显式审批的 native 解码/有界 JPEG、指定或均匀抽帧、短 silent 视频转码；核验产物后只向一次模型请求附选中图与 provenance，receipt/error/取消/不重放回归 |
| Inspector bridge | `inspection.rs` | 共享 native repository、session 树归属校验、RunRecord/raw durable record 稳定 DTO 投影；真实 SQLite flush/reopen 的递归与跨 Hub 隔离回归 |
| Run controller | `run.rs` | 动态 Hub request、delta/full handoff、native-resume recovery、自动 fallback 循环、带产品时钟的事件投影与结果 |
| Engine readiness | `readiness.rs` | 只读 probe seam、EngineReadiness 分类与 SystemProbe 实现 |
| Managed SDK installation | `sdk_installation.rs` | 产品运行时目录的包身份/版本/入口校验，拒绝符号链接、Unix 异主及组/其他用户可写路径、工作区交叠；不执行包代码 |
| Permission contract | `permissions.rs` | PermissionProfile、adapter 映射资格契约、eligibility DTO 与 PermissionRouter |
| Provider plane | `provider_plane/` | 版本化模型身份、显式 binding 校验、per-engine override 与纯路由规划 |
| Provider catalog | `providers.rs` | Hub 多供应商连接配置、backend-validated model binding、私密凭证与 route DTO 投影 |
| Provider diagnostics | `provider_diagnostics.rs` | API 模型发现、非权威 binding preview、canonical 最小推理测试与脱敏 DTO |
| Session workspace | `session_workspace.rs` | 独立会话专属目录创建、私有权限与 project/session 分类 |
| Workspace files | `workspace_files.rs` | 从已存 session cwd 读取真实目录与只读文本预览；capability-relative 逐层 no-follow，复用敏感路径规则，目录 2000 项/预览 256 KiB 上限，HTTP/Tauri 共用 |
| Workspace Git | `workspace_git.rs` | 当前 worktree 的只读 status/diff；NUL 路径解析、staged/unstaged/untracked 分离，敏感 rename 双端过滤；禁外部 diff/textconv/fsmonitor，10 秒/1 MiB status/2000 路径/256 KiB diff 限额 |
| Workspace Skills | `workspace_skills.rs` | 有界项目技能文件目录/源文档，真实创建/编辑/可恢复归档，保留启用状态/资源；现有 FsSkillRepository 发现回归，无全局扫描/下载/脚本执行 |
| User terminal | `terminal.rs` | macOS/Linux 原生 PTY，显式用户启动；session+随机 handle 所有权，最多 4 个实例，256 KiB 输出；关闭回收 owned shell group/thread，先清进程组再 reap 防 PID 复用误杀；不属于 Agent 审批/命令日志 |
| User browser | `user_browser.rs` | 独立临时原生窗口，session+随机 handle 绑定、最多 4 个；HTTP(S) 导航，拒绝特权来源/凭证 URL/弹窗/下载；普通应用 IPC 由 main 全局窗口身份守卫拒绝，core/plugin capability 仍仅 main |
| Worktree discovery | `worktree_discovery.rs` | 已有工作树只读发现及数组/结构化 state 的 untagged IPC DTO；不得把嵌套 Result 的 Ok/Err 包装传给前端 |
| Failure matrix | `failure.rs` | terminal 失败 FailureClass 稳定分类与 fallback 资格规则 |
| Handoff filter | `redact.rs` | 跨 engine 敏感内容过滤与 adapter-private 元数据剥离 |
| Resource claims | `claims.rs` | 共享工作区 ResourceClaim 冲突代数 |
| Worktree isolation | `worktree.rs` | 受管 git worktree、adoption 预览与丢弃 |
| Engine repair | `repair.rs` | readiness→RepairAction 映射、一键安装 allowlist 执行器 |
| SQLite store | `store/` | `db.rs` storage kernel；`mod.rs` 共享数据访问层与 `pub(crate)` 常量/helper；`branch.rs`/`handoff.rs`/`team.rs`/`turn.rs` 各持久化簇；`tests.rs` 全量测试。covering workspace kind、identity、模型选项、FallbackPolicy snapshot、branch watermark/switch/handoff/timeline、Turn/Attempt 与崩溃修复 |
| Tool content | `store/tool_content.rs` | 原子保存脱敏全文与 evidence 引用，UTF-8 分块、会话归属读取及回归 |
| Input attachments | `store/input_attachments.rs` | schema 23 原字节/限额、私有 BLOB/会话文件副本、处理清单、仅引用 prompt 与归属预览；不执行媒体处理 |
| Message journal | `store/message_journal.rs` | schema 20 耐久 assistant 文本，发送前保存、游标分页、attempt 终态对齐；旧 Turn 摘要继续作为无日志记录的兼容路径 |
| Product history / media authority | `store/context_history.rs` / `store/media.rs` | 复用权威日志跨片段/块搜索与来源 SHA，schema 24 派生字节/文件、选择快照和私有投递 receipt，源文件不改 |
| Local automations | `store/automation.rs` | schema 21 真实 CRUD、限额/乐观冲突、授权固定路由、原子认领和终态/restart 恢复；10 项无账号回归 |
| ALVA tool settings | `store/tool_settings.rs` | schema 22 真实能力开关与 stdio MCP 暂停优先 CRUD、授权/revision 及运行 snapshot |
| MCP diagnostics | `mcp_diagnostics.rs` | 最多 2 并发、已存 session cwd、revision/确认核对的单次握手/目录测试与回收，无模型调用 |
| Desktop entry | `main.rs` | Tauri plugin、workspace 创建、运行/切换/policy/readiness/Inspector/工具正文 command 和 timestamp-preserving event bridge |

## 引擎注册表与编排策略（R-tickets 17/18）

### 2026-09-22 产品接线修正（覆盖上文旧 Claude print 描述）

- Desktop Claude 使用 `ClaudeAdapter` Full SDK，interactive 与 workspace_write 都走 Default 权限回调（后者更严格），仅 unrestricted 使用 BypassPermissions；resume 保留持久化 SDK session，默认不加载工作区 setting sources。
- Claude binary/version 检查针对 Node.js 20+；SDK 固定为 `$HOME/.local/share/alva-hub/runtimes/claude/node_modules/@anthropic-ai/claude-agent-sdk`，用户显式安装已审查版本后使用。assembly、readiness、fallback veto 共用只读解析并传绝对入口；从 package.json 的 exports/import 或 main 读取入口，不从项目、cwd 或 npm global 搜索。缺失/非法 SDK 返回 `dependency_unavailable` + Missing；不以 Node 存在冒充 SDK 就绪。此校验只保证安装边界与入口结构，SDK 真实账号/版本行为仍待端到端验收。
- `TimelineActivity` 的 `tool_input`/`tool_output` 是 2000 字符摘要，对应 `_ref` 指向 schema 19 耐久全文。blocking worker 内整体脱敏后，正文块与 evidence 原子提交，再发送 live DTO。每块 ≤16 KiB UTF-8、单正文 ≤64 MiB，超限/失败明确报错；Tauri/HTTP 复用按 session 归属校验的读取 API。旧摘要无引用，不补造丢失内容。
- 工具 live DTO 与 side-effect evidence 共用首次观测的 `first_seen_at`/attempt-local `sequence`；完成时不可覆盖，live DTO 携带 attempt_id 隔离重复 tool ID。旧 evidence JSON 默认缺省，新字段无需改 SQL schema；无法追溯旧记录未保存的精确时间。
- Assistant 片段在 execute_turn 的统一 emit 边界附加 `MessagePosition`：stream_id 是 attempt ID（非持久 seam 使用随机 ID），offset 是每个 native message 的 UTF-16 长度。重复文本不同 offset 不合并，终态保留同一 message scope；Tauri/SSE 原样传递。schema 20 在同一边界发送前保存可见 assistant 文本，Tauri/HTTP 提供固定高水位分页；失败显式使 attempt 失败。工具/审批/协作事实仍使用各自现有耐久表，不宣称所有 raw adapter 事件均入日志。

- `engine_registry.rs` + `engine_wiring.rs`：引擎注册表是权限门、就绪分类与装配的唯一查询点。每个引擎一份 `EngineBlueprint`（descriptor、readiness spec、probe/veto/enforcement/build 闭包）；内建四引擎的名字只出现在 `engine_wiring.rs` 一个装配点。注册新引擎 = 注册新 blueprint，三模块零改动（R17）。
- `store/orchestration.rs`：团队队列领取**策略**（生命周期门、冲突检测、FIFO 公平预留）为纯函数 `decide_claim`；`store/team.rs::claim_next_worker_task` 是记账层——读库、组装 `ClaimInputs`、按 `ClaimDecision` 落库（R18）。
- `readiness.rs` / `permissions.rs` / `alva.rs`（装配）的引擎名字面量为零；permissions 的 `#[cfg(test)]` 断言内建映射时按名点名。
