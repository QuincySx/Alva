# Cindy 状态机与端到端时序

> 基线：`240247e52e938710f409c921a7a393097dcef52b`。表中除“待验”外均为源码静态确认。“任务”是产品 session；“运行时会话”是 Maker/Agent handle。

## 1. 应用账号边界

```mermaid
stateDiagram-v2
  [*] --> signed_out: cloud startup / no verified member
  [*] --> local: persisted local mode
  signed_out --> cloud: login or late refresh success
  signed_out --> local: enter local
  local --> signed_out: exit local
  cloud --> signed_out: logout / definitive invalid / delete confirmed
  cloud --> local: enter local (clear auth + provider secret cache)
  cloud --> cloud: refresh rotate token
```

| 转移 | 前置/副作用 | 失败语义 | 证据 |
|---|---|---|---|
| signed-out → cloud | membership 已验证 | 短暂错误保留 token 但不进 app | `authManager.ts`, `authStartupGate.ts` |
| any → local | 取消 browser auth，清 provider secret cache/auth，再 commit boundary | 旧异步结果由 auth epoch/owner generation 丢弃 | `authManager.ts`, `appSessionState.ts` |
| local → signed-out | 保留 local owner 数据 | 不隐式转 cloud | 同上 |
| cloud → signed-out | logout/凭据确定失效/删号确认 | 网络/429 不清 token | `authManager.ts` |

冷启认证有两个时钟：UI splash 可超时返回 signed-out，但后台 refresh 不取消；晚到成功后广播 cloud。这是显式“界面 gate 超时 ≠ 认证请求取消”。

## 2. 登录流 reducer

```mermaid
flowchart LR
  I[identifier] --> R[realm-confirmation]
  R --> M[method-choice]
  M --> V[verification-code]
  M --> B[browser-redirect]
  B --> A[account-selection]
  A --> S[sso-verification]
  V --> X[binding]
  S --> X
  X --> C[completed]
  I & R & M & V & B & A & S & X --> E[error + recoverTo]
  E --> I
  E --> R
  E --> M
  E --> V
  E --> B
  E --> A
  E --> S
  E --> X
```

证据：`packages/auth-client/src/types.ts` 的 `AuthFlowState` 与 reducer。具体 realm/SSO 网络回包需真服务动态待验。

## 3. Maker Session / Turn

```mermaid
stateDiagram-v2
  [*] --> active: create/resume
  active --> active: send accepted -> events -> done
  active --> aborting: user stop / stall watchdog
  aborting --> active: vendor turn settles
  aborting --> closed: grace expires, force close
  active --> error: terminal runtime error
  active --> closed: requested / agent-switch / unexpected close
  error --> closed: close
  closed --> active: next send lazy rebuild (host level)
```

| 阶段 | 不变式 |
|---|---|
| reserve | host turn lease + 单 reservation；busy 拒绝 |
| pre-dispatch | 取消返回 `accepted:false`，不计已执行 |
| dispatch | 安装 origin，agent handle 接受后才越边界 |
| running | 事件流持续重置 stall watchdog |
| waiting interaction | 暂停 stall 计时；无 listener 默认拒绝/空答 |
| aborting | 不再计 stall；grace 内不 settle 则 close |
| terminal | `done` 或终态 `error`；旧 unknown error 按终态 |

证据：`packages/maker-core/src/session.ts`、`types/events.ts`、`maker.ts`。

## 4. 持久化输入队列

```mermaid
flowchart TD
  U[用户 send] --> Q[入队]
  Q --> H{是队头且未暂停?}
  H -- no --> W[等待]
  H -- yes --> S[handle.send]
  S -->|accepted false| Q
  S -->|accepted true| P[持久化 accepted/active turn]
  P --> R[运行]
  R --> T[终态]
  T --> D[出队并 drain 下一条]
  R --> X[Stop]
  X --> K[保留队列 + paused]
  K --> C[明确 continue/retry]
  C --> H
```

Steer 是一个标记而不是普通 send：它阻塞 drain，仅 Maker 接受后持久化；失败则保留/回滚。崩溃后快照以 paused 恢复，不自动重放不确定输入。Retry 根据是否已有部分输出选择 clone 或 continuation。

证据：`apps/desktop/src/main/maker-ipc/agent-input-coordinator.ts`、`localDb/schema.ts` 的 `agent_input_queue_snapshots`。

## 5. Harness/Agent 切换

```mermaid
flowchart TD
  A[用户请求切换] --> V{远程或 Orca?}
  V -- yes --> N[拒绝: unsupported]
  V -- no --> I[记录内存 deferred intent]
  I --> B[等待 turn boundary]
  B --> P[下次 send 前预检]
  P --> H[生成 deterministic handoff]
  H --> C[关闭旧 runtime]
  C --> DB[DB 事务 commit 目标 route/vendor id]
  DB --> E[写 agent_switch boundary + 注入 handoff]
  E --> R[恢复/创建目标 runtime]
  R --> S[发送原输入]
```

- DB commit 是业务真值切换点。boundary 写失败时用内存 handoff 降级继续；spawn 失败由下次 send 重建。
- 切回原 Agent 优先用最新 boundary + 停放 vendor ID 增量恢复；失败原子清 vendor ID，用 full handoff。
- deferred intent 不持久化，app 重启后丢失。

证据：`sessionAgentSwitchHandler.ts`、`agentHandoff.ts`。

## 6. Runtime 模型/路由切换

| 情形 | 处理 |
|---|---|
| 同 route 且 runtime 支持 | 直接 `setModel` |
| route 先更新后 `setModel` 失败 | 回滚 route |
| turn 忙且 credential family 不兼容 | 延迟到 boundary |
| Codex turn 运行中 route 更换 | 不中途切，边界应用 |
| 运行时 effort 更改超过 3.5s | 终止 runtime，1s 收敛，下次 send 重建 |

证据：`runtimeSetModel.ts`、`runtimeSetEffort.ts`。

## 7. 人机交互

```mermaid
stateDiagram-v2
  [*] --> waiting: interaction_request
  waiting --> resolved: desktop/channel/headless decision
  waiting --> cancelled: turn aborted/session closed/dismissed
  waiting --> resolved: timeout => deny/empty
  resolved --> [*]
  cancelled --> [*]
```

交互类型：permission / ask_user_question / plan_review。无可用界面或 resolver 时 fail closed。plan-mode 与 permission-mode 是正交状态，不可由一个字段替代另一个。

证据：`packages/maker-core/src/session.ts`、`apps/desktop/src/main/maker-ipc/interactionRouter.ts`、`localDb/schema.ts`。

## 8. Orca Team/Worker

### Team

```mermaid
stateDiagram-v2
  [*] --> active: start_team
  active --> completed: end_team success
  active --> cancelled: explicit cancel/end
  active --> failed: lifecycle failure
  completed --> [*]
  cancelled --> [*]
  failed --> [*]
```

### Worker

```mermaid
stateDiagram-v2
  [*] --> idle: create worker/session
  idle --> running: queued message accepted by Maker
  running --> idle: dispatch failed rollback
  running --> done: turn terminal success
  running --> error: terminal error
  done --> idle: explicit idle/reuse
  error --> idle: explicit recovery/reuse
  idle --> [*]: archive
  done --> [*]: archive
  error --> [*]: archive
```

不变式：

- DB 部分唯一：每 lead 最多一个 active team，每 team 最多一个 focused worker。
- 创建 reservation 是短租约；label 唯一；软/硬上限分开。
- Worker 初始不继承 Lead permission。远程继承 remoteHostId，但 Pi remote 拒绝。
- 从 idle 到 running 以 Maker accepted 为边界；拒绝/失败回滚。
- done/error 会向 Lead bridge；idle watcher 可关运行时，但不删逻辑 Worker。
- 只有透明 runtime/thread provenance 能做控制身份，业务 sessionId 和 `mcp-session-id` 不足以授权。

证据：`apps/desktop/src/main/localDb/schema.ts`、`apps/desktop/src/main/orca/*`、`packages/orca-workflow/src/orca-bridge-mcp.ts`。

## 9. Scheduler Attempt

```mermaid
stateDiagram-v2
  [*] --> claiming
  [*] --> loading
  claiming --> persisting
  loading --> persisting
  persisting --> running
  running --> queued: target busy / no runtime resource retained
  queued --> running: retry admitted
  running --> cancelling: abort requested
  queued --> cancelling: cancel queued
  running --> finalizing: success/failure/skip
  cancelling --> finalizing: runner settles
  finalizing --> [*]
```

运行时 phase 不落 DB；持久 run 只有 running/success/failed/aborted/interrupted/skipped。`queued` 不持有 Agent/MCP/token，不占并发槽。转移只能由 `attemptLifecycle.ts` 执行，非法边抛错。

其他关键时序：

- due 时以 `nextFireAt` CAS claim，避免重复触发。
- 忙 target 可“无 run row”延迟，与已创建 run 后 queued 不同。
- pre-run hook: 0 运行，2 skipped，其他/超时/不存在 failed。
- 运行中 heartbeat；启动 stale sweep 将崩溃留下 running 转 interrupted。
- silent success 不通知，除非 Agent 显式 notify；失败强制通知。

证据：`packages/maker-scheduler/src/types.ts`、`packages/maker-scheduler/src/engine/attemptLifecycle.ts`、`packages/maker-scheduler/src/engine/scheduler.ts`、`apps/desktop/src/main/scheduler-host/index.ts`、`apps/desktop/src/main/scheduler-host/storage.ts`。

## 10. Ghost Runtime

```mermaid
stateDiagram-v2
  [*] --> off
  off --> starting: enable/start
  starting --> running: startup success
  starting --> crashed: startup/process failure
  running --> stopping: stop/disable/update/uninstall
  stopping --> off: cleanup complete
  running --> crashed: child exit
  crashed --> starting: retry below fuse threshold
  crashed --> fused: >=3 crashes within 60s
  fused --> off: explicit re-enable resets fuse
```

Stop 幂等。Node broker 对每 plugin/entry 管理子进程，有 pending/bytes/timeout 上限、启动重试和空闲超时。

证据：`apps/desktop/src/main/cindy-brain/runtime/GhostRuntime.ts`、`apps/desktop/src/main/cindy-brain/nodeRuntimeBroker.ts`。

## 11. 插件安装/升级

```mermaid
flowchart LR
  A[not-installed/update-available/conflict] --> L[获取 per-plugin lock]
  L --> D[下载 staging]
  D --> V[验证包/签名/provenance]
  V --> P{权限扩展?}
  P -- yes --> R[用户 review]
  P -- no --> C[预提交重验 owner/source/bytes]
  R -->|allow| C
  R -->|deny| X[取消]
  C --> M[原子 commit package]
  M --> G[更新 owner ledger]
```

卸载先停/清 runtime，再最佳努力更新 ledger。显式 opt-out 是防默认插件被再安装的持久化意图。

证据：`apps/desktop/src/main/plugin-market/service.ts`、`install.ts`、`packages/plugin-protocol/src/delivery.ts`。

## 12. Hook Transport

```mermaid
stateDiagram-v2
  [*] --> stopped
  stopped --> connecting: enabled + owner active
  connecting --> connected: WS open + hello/welcome
  connecting --> error: auth/network/protocol failure
  connected --> connecting: idle watchdog/missed pong/reconnect
  connected --> standby: duplicate-device close
  standby --> connecting: low-frequency probe
  error --> connecting: backoff retry
  connecting --> stopped: disabled/logout/generation changed
  connected --> stopped: disabled/logout
  standby --> stopped: disabled/logout
```

401 只做一次 forced refresh；连接退避 1–30s 带 jitter；65s 无活动 watchdog。

证据：`apps/desktop/src/main/hook-control/transport.ts`。

## 13. Hook Task Dispatch

```mermaid
flowchart TD
  R[远程 task request] --> D{见过 connectionId+requestId?}
  D -- yes --> RA[重放原 ack，不重跑]
  D -- no --> V[验证启用/路径 alias/session/live cwd]
  V -->|invalid| X[rejected + reason]
  V -->|new/idle| A[accepted + dispatch]
  V -->|busy| Q[queued + FIFO]
  A --> P[最新 progress 快照]
  P --> E[turn.end 终态]
  E --> N[离线缓存/持久并重放]
  E --> QD[drain next queued]
```

Ack reason 只使用定义枚举：unknown_workspace/workspace_not_allowed/session_not_found/disabled/invalid。复用 session 每条都查 live cwd；路径映射变化必须丢弃旧绑定。

证据：`apps/desktop/src/main/hook-control/dispatcher.ts`、`apps/desktop/src/main/hook-control/requestLedger.ts`、`apps/desktop/src/main/hook-control/session-runner.ts`、`apps/desktop/src/main/hook-control/outbound.ts`。

## 14. 个人微信连接与任务

### 连接

```mermaid
stateDiagram-v2
  [*] --> disconnected
  disconnected --> authorizing
  authorizing --> waiting_confirmation: QR scanned
  waiting_confirmation --> connected: confirmed
  connected --> reconnecting: transient disconnect
  reconnecting --> connected
  reconnecting --> needs_reauth: credentials invalid
  authorizing --> disabled_by_policy: policy deny
  connected --> disabled_by_policy
  authorizing --> error
  reconnecting --> error
```

### Inbox task

```mermaid
stateDiagram-v2
  [*] --> pending
  pending --> dispatching: lease claim (60s)
  dispatching --> pending: target busy / lease release
  dispatching --> accepted_running: Maker accepted
  accepted_running --> waiting_desktop: interaction needs desktop
  waiting_desktop --> accepted_running: resumed
  accepted_running --> delivery_pending: turn terminal + outbox created
  delivery_pending --> completed: all outbox delivered
  pending --> expired: age > 30min
  dispatching --> expired: age > 30min
  pending --> cancelled: stop/binding close
  dispatching --> cancelled: stop/binding close
  accepted_running --> interrupted: stop/binding close
  pending --> rejected_overload: non-control overload
  delivery_pending --> failed_terminal: terminal delivery failure
```

Outbox：`pending → sending → delivered`；可从 sending 回 pending 重试，或进 failed_terminal。`/stop` 与 `/stop all` 是控制消息，不走 overload 拒绝。binding epoch 防止关闭/重绑后旧写入落库。

证据：`apps/desktop/src/main/localDb/worker/opHandlers/wechatTx.ts`、`apps/desktop/src/main/im/wechat/WechatIM.ts`、`apps/desktop/src/main/localDb/schema.ts`。

## 15. Device Link 连接/单主

### Client

```mermaid
stateDiagram-v2
  [*] --> stopped
  stopped --> connecting: start/token available
  connecting --> online: relay connect + hello/ack
  connecting --> connecting: timeout/backoff
  online --> connecting: missed pong/transient close
  connecting --> stopped: permanent close/stop/revoked
  online --> stopped: permanent close/stop/revoked
```

### Local owner lease

```mermaid
stateDiagram-v2
  [*] --> standby
  standby --> owner: CAS acquire absent/stale lease
  owner --> owner: heartbeat every 5s
  owner --> standby: DB/store unavailable self-demotion
  owner --> standby: explicit release
  standby --> standby: live owner heartbeat (<15s)
```

网络断开不等于让 owner。离线 push 仅内存，不是业务真值。`transport-timeout` 是可能恢复 close，永久原因（user/toggle-off/shutdown/revoked）不自动重开。

证据：`packages/device-link/src/client.ts`、`ownership.ts`、protocol types，`apps/desktop/src/main/device-link/*`。

## 16. Terminal

```mermaid
stateDiagram-v2
  [*] --> running: create/attach
  running --> exited: PTY exit
  exited --> running: restart same id
  running --> disposed: explicit dispose / no fallback owner
  exited --> disposed: explicit dispose
  disposed --> [*]
```

PTY exit 不删记录；dispose 才 kill/移除。窗口销毁时优先转 owner，无 fallback 才 dispose。证据：`apps/desktop/src/main/terminal/ptyManager.ts`。

## 17. 更新

```mermaid
stateDiagram-v2
  [*] --> idle
  idle --> checking: timer/manual
  checking --> downloading: newer manifest
  checking --> idle: no update
  checking --> error: check failure
  downloading --> ready: verified patch saved
  ready --> superseding: newer version begins download
  superseding --> ready: newer succeeds and replaces old
  superseding --> ready: newer fails, old ready preserved
  ready --> idle: matching version after relaunch
  ready --> error: apply attempts >= 3 / invalid patch
  error --> checking: later retry/manual
```

Auto relaunch 是另一个 gate：enabled + packaged + ready + 无活跃任务 + quiet/resume 窗口 + 锁屏/空闲条件。任一忙查询错误均 fail closed，不自动重启。

证据：`apps/desktop/src/main/updateService.ts`。真实 patch 签名与系统替换为动态待验。

## 18. Local DB 打开/恢复

```mermaid
flowchart TD
  O[选定 owner DB] --> L[获取 reader/writer lease]
  L -->|incompatible/downgrade/damaged lock| F[fail closed]
  L --> B[迁移前 online backup + 磁盘预算]
  B --> M[schema migrations]
  M --> V[vec capability/migrations]
  V --> D[drift detection]
  D -->|dev repair allowed| RB[备份后 repair]
  D -->|release drift| W[警告/提示]
  D --> S[startup repairs + optimize]
  S --> X[worker smoke]
  X -->|pass| T[DB worker takeover]
  X -->|fail| I[in-process fallback]
```

`SQLITE_CORRUPT` 独立恢复链：`.bak.clean` → 最新 `.bak.<ISO>` → 无备份致命。Worker 运行后错误边界自动重启一次。

证据：`apps/desktop/src/main/localDb/index.ts`、`schema.ts`、`migrations/*`。

## 19. 需要动态故障注入验证的边

1. auth splash 超时后 late success 与同时 logout/local switch 的 epoch 竞态。
2. Agent accepted boundary 前/后 crash，输入快照是否严格不重复执行。
3. Harness 切换 DB commit 后 boundary/spawn 失败的恢复文本是否完整。
4. Scheduler claim/heartbeat/finalize 各 await 点 crash，stale sweep 是否都收敛为单一终态。
5. Orca reservation 过期、批量部分成功、Worker bridge 丢线重放的可观测性。
6. Hook/WeChat 在 accepted-running 、outbox sending 和 binding close 瞬间崩溃的去重。
7. Device Link 两个本地进程与 DB 短暂不可用时的单主保证。
8. Updater ready→superseding→回退旧 patch 及 apply-attempt=3 的真实文件系统结果。
