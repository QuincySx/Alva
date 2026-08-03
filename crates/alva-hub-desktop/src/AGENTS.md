# alva-hub-desktop/src
> Hub Desktop 的 Rust 产品控制器与 Tauri 装配。

## 地位

位于 L7 Desktop 产品边界。业务执行只依赖 Hub/engine contract，具体 engine
装配和 Tauri IPC 保持在产品入口。

## 逻辑

1. `alva` 按用户选择为 workspace 构造 Alva、Claude light 或 Codex adapter。
2. `store` 在私有 SQLite 中原子保存 HubSession、global/session FallbackPolicy snapshot、多 EngineBranch、watermark、switch、handoff、timeline、Turn 和 Attempt。
3. `run` 创建或恢复 active branch 的 product/native session 映射；parked return 注入 delta，native resume 失效时原子升级完整 handoff 再 fresh 执行。
4. SessionStarted/ToolEnd 增量落盘，Completed 原子提交 Attempt usage 与 Turn 终态。
5. 运行中 switch 持久化为 queued；终态或启动 crash repair 后再原子应用。
6. runtime events 与 switch facts 被投影为稳定 timeline states。
7. `main` 从 Tauri app-data 初始化 store，并暴露运行、切换、列表和详情 IPC。
8. `main` 暴露全局与单 session fallback policy IPC；自动使用必须来自当前 session snapshot 的明确授权。
9. `readiness` 用只读 `EngineProbe` facts（PATH 发现、`--version`、凭证存在性、attempt 历史）分类出结构化 EngineReadiness；`main` 以 `SystemProbe` 暴露 readiness IPC。
10. `run` 以 engine factory + health veto 装配执行；`failure` 矩阵分类 terminal 失败，仅 operational continuity class 触发 `fallback_failed_attempt`：同一 Turn 原子转移到下一授权 target（parked source + 预持久化 full handoff + 串行 Attempt + `engine_fallback` fact），target 同步启动失败时全部状态可恢复。
11. fallback 候选按 policy 顺序、三元组去重、一 cycle 一次；health veto 或 required_capabilities 不满足 → skip + `engine_fallback_skipped` fact；链路耗尽 → `fallback_exhausted` fact 汇总全 cycle 终态。
12. stall watchdog 仅在无 open tool、无 pending permission 时计时，任意事件重置 180s 窗口；确认 stall = `FailureClass::Stall` continuity failure + Failed TimelineEvent + best-effort cancel，走既有有界 fallback。
13. `main` 维护 active engine registry（session→live adapter）；`stop_hub_attempt` 校验 running + native session 后落 `attempt_cancelled` fact 再 cancel 引擎，attempt 终态来自 cancelled stream 自然收敛，重启不重放；用户 Stop 不触发 fallback（Cancelled 归类 Other）。
14. `permissions` 定义 session 级 PermissionProfile（interactive/workspace_write/unrestricted，随 hub_sessions 持久化、branch 切换不变）与三 engine adapter 映射契约：等价或更严格才可执行（alva=交互回调、claude=acceptEdits/bypassPermissions、codex=sandbox 档位），只能弱化的组合 ineligible 并带结构化原因；interactive 保证的是"raised request 由用户裁决"的路由（risk classifier 决定哪些调用 raise），不是每个副作用都询问。`alva::build_engine` 按映射装配并 fail-closed；fallback 候选先过资格闸门再过 health veto，skip 落 `engine_fallback_skipped` fact；手动 switch 同受此闸门（`request_engine_switch` 拒绝 weaken-only target，同 target 重选仍是 no-op）。
15. PermissionRequest 事件携带 request_id + emitting EngineSessionId 投影为 `permission_required` TimelineEvent；`main` 的 `PermissionRouter` 把用户决定路由回发起请求的那个 engine session（单次使用，attempt 终态/fallback/用户 Stop 后清空），决定不会泄漏到后续 fallback engine。SessionStarted 之前到达的请求无路由身份：card 不带控件、watchdog 保持 armed，死等以有界 stall 收敛；attempt 已带 terminal error 时 watchdog 恒 armed，pending approval 不能让 drain 无限阻塞。

16. `redact` 是所有跨 engine handoff 的唯一过滤闸门（`build_handoff_package` 持久化前执行）：credentials/token/敏感 `NAME=value`/PEM 全字段清洗，账号与账单标签、team_snapshot 等 adapter-private 元数据不出境；hidden reasoning 与 approval token 结构性不入库。full transcript 按 session 开关（schema v7,默认关闭）附加且过同一过滤。

17. `repair` 把 readiness failure 映射为结构化 RepairAction：npm 官方分发的 claude/codex 提供一键 install/upgrade（argv 由服务端按当前 readiness 重新生成，客户端只报 engine+kind）；认证/配置修复是最短官方指引不代跑；执行后重新探测恢复 eligibility；模块不接触 HubStore，pinned model/account/profile/binary 结构性不被改动。

18. team walking skeleton（schema v8）：TeamRun 恰有一个 lead session；worker 是 Hub-owned durable hub_session（继承 lead workspace/profile、快照全局 policy、自选 EngineTarget），非 engine-private subagent；WorkerTask 记录 scope/status/result/changed files（来自 side-effect 证据 path，ToolEnd 合并保留 ToolStart 的 path）/verification；worker 终态经 `finish_worker_task` 落 lead timeline `worker_outcome` fact + live Worker card；`main::execute_session_run` 是用户 prompt 与 worker dispatch 共用的执行通道。

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
| Engine assembly | `alva.rs` | Alva provider/catalog、Claude light 与 Codex safe-default 装配 |
| Run controller | `run.rs` | 动态 Hub request、delta/full handoff、native-resume recovery、自动 fallback 循环、事件投影与结果 |
| Engine readiness | `readiness.rs` | 只读 probe seam、EngineReadiness 分类与 SystemProbe 实现 |
| Permission contract | `permissions.rs` | PermissionProfile、adapter 映射资格契约、eligibility DTO 与 PermissionRouter |
| Failure matrix | `failure.rs` | terminal 失败 FailureClass 稳定分类与 fallback 资格规则 |
| Handoff filter | `redact.rs` | 跨 engine 敏感内容过滤与 adapter-private 元数据剥离 |
| Engine repair | `repair.rs` | readiness→RepairAction 映射、一键安装 allowlist 执行器 |
| SQLite store | `store.rs` | identity、FallbackPolicy snapshot、branch watermark/switch/handoff/timeline、Turn/Attempt 与崩溃修复 |
| Desktop entry | `main.rs` | Tauri plugin、运行/切换/policy/readiness command 和 event bridge |
