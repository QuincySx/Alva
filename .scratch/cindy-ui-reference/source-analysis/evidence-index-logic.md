# Cindy 业务逻辑证据索引

> 源码基线 `240247e52e938710f409c921a7a393097dcef52b`。路径相对于 `/Users/smallraw/.cache/cindy-deep-dive/source/cindy`。本索引只列源码/测试/schema；设计文档只用来定位，不单独作为“已实现”证据。

## 确认级别

- **S（静态确认）**：产品实现代码/schema 直接证明，通常有测试二次印证。
- **D（动态待验）**：客户端实现存在，但结论依赖真实服务、三方 CLI、OS 或网络。
- **I（推断）**：由多个 S 证据组合得出，非单一代码分支的显式承诺。

## 1. 账号/本地模式

| 证据 | 位置 | 支持的结论 | 级别 |
|---|---|---|---|
| AppSessionMode 与 owner boundary | `apps/desktop/src/main/appSessionState.ts:17` | signed-out/local/cloud，local owner，generation/scope fence | S |
| Auth 主状态机 | `apps/desktop/src/main/authManager.ts` | 启动优先级、refresh、local 进出、logout/删号、token 不出 main | S |
| Local 进/出 | `apps/desktop/src/main/authManager.ts:1674`, `:1687` | 进 local 清运行认证，退出保留本地数据 | S |
| 冷启 splash gate | `apps/desktop/src/main/authStartupGate.ts` | UI 超时不取消 refresh，late success 可广播 | S |
| Owner 集成启动 | `apps/desktop/src/main/accountIntegrationStartup.ts` | DB ready 后启动，回调复验 owner | S |
| Renderer IPC 合约 | `apps/desktop/src/shared/authIpc.ts` | 登录/本地模式/删号动作与输入校验 | S |
| Auth reducer | `packages/auth-client/src/types.ts:216` | 登录界面全步骤与 `recoverTo` | S |
| 竞态回归测试 | `apps/desktop/src/main/__tests__/authStartupGate.test.ts`, `accountIntegrationStartup.test.ts` | timeout/late completion/owner switch 语义 | S |
| 真实 realm/SSO/API | 外部 auth 服务 | 服务状态码、重试和删号最终结果 | D |

## 2. 项目/工作区/Worktree

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `apps/desktop/src/main/localDb/schema.ts:40` | session 项目、workingDir、workspaceKind、worktree/remote/device 字段 | S |
| `apps/desktop/src/main/localDb/dialogueWorkspace.ts` | owner/date/session 组成 dialogue 工作目录 | S |
| `apps/desktop/src/main/localDb/dialogueWorkdirSelfHeal.ts` | 旧 dialogue 路径严格匹配、copy-before-rewrite、失败保留 | S |
| `apps/desktop/src/shared/projectKeys.ts` | local/remote/device 稳定 key，Windows case-fold | S |
| `packages/project-context/src/*` | `.cindy/project-knowledge`、Markdown/frontmatter、commit 语义 | S |
| `apps/desktop/src/main/maker-ipc/projectContextInject.ts` | session create 时注入项目知识快照 | S |
| `apps/desktop/src/main/worktree/WorktreeManager.ts` | 创建/删除/stash/安全校验/并发串行 | S |
| `apps/desktop/src/main/worktree/worktreeStore.ts` | owner-scoped `worktrees.json` 真值 | S |
| `apps/desktop/src/main/worktree/sessionRemovalRecycle.ts` | archive/delete 的回收，runtime close 不回收 | S |

## 3. Maker/Turn/队列

| 证据 | 位置 | 支持的结论 | 级别 |
|---|---|---|---|
| SessionStatus | `packages/maker-core/src/session.ts:54` | active/aborting/closed/error | S |
| Stall 默认 | `packages/maker-core/src/session.ts:80` | 45 分钟，可 env 覆盖/0 关闭 | S |
| Send/abort/interaction/watchdog | `packages/maker-core/src/session.ts` | accepted 边界、单 turn lease、无 listener fail closed、abort grace | S |
| Maker registry | `packages/maker-core/src/maker.ts:223` | AgentKind 创建/恢复，runtime map，vendor ID CAS | S |
| AgentEvent union | `packages/maker-core/src/types/events.ts:16` | 文本/工具/交互/计划/用量/终态的统一面 | S |
| 输入协调器 | `apps/desktop/src/main/maker-ipc/agent-input-coordinator.ts` | enqueue/head drain/steer/stop/retry/崩溃快照 | S |
| 队列持久化 schema | `apps/desktop/src/main/localDb/schema.ts:1455` | 每 session 一份 queue snapshot | S |
| Session 回归测试 | `packages/maker-core/src/session.turn-stall.test.ts`, `session.close.test.ts`, `session-send-outcome.test.ts` | watchdog/close/accepted 边界 | S |
| Queue 回归测试 | `apps/desktop/src/main/maker-ipc/__tests__/agent-input-coordinator.test.ts` | stop/steer/recovery 不变式 | S |
| 三家供应商真实长连 | `packages/maker-core/src/agents/{claude,codex,pi}` + 外部 CLI/API | 事件顺序、异常终态、账号配额 | D |

## 4. Harness/模型切换

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `apps/desktop/src/main/maker-ipc/sessionAgentSwitchHandler.ts` | deferred intent，turn boundary，DB commit point，远程/Orca 拒绝 | S |
| `apps/desktop/src/main/maker-ipc/agentHandoff.ts` | full/incremental deterministic handoff，boundary vendor ID | S |
| `apps/desktop/src/main/maker-ipc/runtimeSetModel.ts` | route-first + rollback，busy family defer，provider_id authoritative | S |
| `apps/desktop/src/main/maker-ipc/runtimeSetEffort.ts` | 3.5s 超时、1s 终止收敛、下次重建 | S |
| 同目录 `__tests__/{sessionAgentSwitchHandler,agentHandoff,runtimeSetModel,runtimeSetEffort}.test.ts` | 切换成功/回滚/降级行为 | S |

## 5. Pi Harness

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `packages/maker-core/src/agents/pi/index.ts` | `pi --mode rpc`、JSONL、会话 config/resume/compaction/model switch/MCP bridge | S |
| `docs/dev-rules/pi-harness.md` | 与代码对照的安全/运行时不变式 | S（仅与实现交叉处） |
| `packages/maker-core/src/agents/pi/__tests__/*` | MCP URL 边界、父子 watchdog、集成协议 | S/D |
| 真 Pi 二进制/供应商 | RPC 版本兼容、模型路由、网络故障 | D |

## 6. 权限/交互

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `packages/maker-core/src/session.ts` | 无 resolver 拒绝/空答，交互期暂停 stall | S |
| `apps/desktop/src/main/maker-ipc/interactionRouter.ts` | waiting/resolved/cancelled，desktop/channel/headless | S |
| `apps/desktop/src/main/maker-host/auto-permission-reviewer.ts` | auto 审查和 fail-closed | S |
| `packages/maker-core/src/types/events.ts` | permission/ask/plan 统一 AgentEvent | S |
| `apps/desktop/src/main/maker-ipc/__tests__/interactionRouter.test.ts` | 解决/取消/超时语义 | S |

## 7. Orca

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `apps/desktop/src/main/localDb/schema.ts:247` | team 状态、lead 唯一 active | S |
| `apps/desktop/src/main/localDb/schema.ts:269` | worker 状态、focused 部分唯一 | S |
| `apps/desktop/src/main/localDb/schema.ts:308` | 创建 reservation 租约 | S |
| `apps/desktop/src/main/orca/*` | lifecycle/creation/team/inter-agent/focus/idle/archive/reconcile | S |
| `packages/orca-workflow/src/orca-bridge-mcp.ts` | `cindy_orca` 工具面、远程 preflight，角色校验 | S |
| `packages/orca-workflow/src/__tests__/orca-bridge-mcp.test.ts` | 不可伪造调用者、remote 边界 | S |
| `docs/dev-rules/orca-team-architecture.md` | 用于检查代码是否符合 Lead/Worker 约束；未实现 side activity 不入结论 | S（仅交叉处） |
| 多引擎真实批量运行 | 资源上限、部分创建、断网 bridge 重放 | D |

## 8. Scheduler/自动化

| 证据 | 位置/含义 | 级别 |
|---|---|---|
| `packages/maker-scheduler/src/types.ts:3` | schedule active/paused/expired，run 终态、触发/路由/通知字段 | S |
| `packages/maker-scheduler/src/engine/attemptLifecycle.ts` | runtime phase 权威转移 | S |
| `packages/maker-scheduler/src/engine/scheduler.ts` | 入场/并发/stall/cancel/finalize/replan | S |
| `apps/desktop/src/main/scheduler-host/index.ts` | host 启停、dev passive、runNow、项目自动化对齐 | S |
| `apps/desktop/src/main/scheduler-host/runner.ts` | prompt/script/pre-run hook/target session/worktree | S |
| `apps/desktop/src/main/scheduler-host/storage.ts` | due CAS claim、heartbeat、stale interrupted 清理 | S |
| `apps/desktop/src/main/localDb/schema.ts:783`, `:1007` | schedules/schedule_runs 持久化 | S |
| `packages/maker-scheduler/src/__tests__/*`, `apps/desktop/src/main/scheduler-host/__tests__/*` | 转移、defer、stale、pre-hook、通知 | S |
| 真 cron 跨休眠/时区与 IM 通知 | OS/上游服务结合行为 | D |

## 9. Plugin/Skill/MCP

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `packages/plugin-protocol/src/manifest.ts` | ghost.json v2、ID/路径规则、slots、launch/panel/node/secret/skill/workspace | S |
| `packages/plugin-protocol/src/delivery.ts` | 市场发布/默认安装与可用性合约 | S |
| `apps/desktop/src/main/cindy-brain/runtime/GhostRuntime.ts:19` | off/starting/running/stopping/crashed/fused | S |
| `apps/desktop/src/main/cindy-brain/nodeRuntimeBroker.ts` | 子进程、JSON-RPC/MCP、限额、超时、secret binding | S |
| `apps/desktop/src/main/plugin-market/{service,install,ledger,download}.ts` | owner ledger、staging 验证、权限 review、commit/卸载 | S |
| `apps/desktop/src/main/skillhub/scanner.ts` | global/project + cc/codex/pi 扫描，realpath 去重 | S |
| `apps/desktop/src/main/skillhub/autoSyncService.ts` | 登录后一次同步、白名单、ignore/冲突保护 | S |
| `apps/desktop/src/main/maker-host/custom-mcp-store.ts` | DB 配置/safeStorage token/校验/CRUD 副作用 | S |
| `apps/desktop/src/main/mcp-integrations/custom-mcp-{registry,provider}.ts` | session/runtime context、实际 provider 桥接 | S |
| `packages/maker-core/src/interfaces/mcp-provider.ts:29` | provenance enforcement 显式能力 | S |
| 真市场/CDN/签名 | 发布源可用性与签名链 | D |

## 10. IM/Hook/WeChat

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `apps/desktop/src/main/im/host.ts` | Feishu/Discord/WeChat/Telegram/DingTalk/WeCom host，owner credential/media | S |
| `apps/desktop/src/main/im/connectionLifecycle.ts` | 开停/work 串行，generation 作废旧队列 | S |
| `apps/desktop/src/main/im/binding.ts` | DB-first binding，复合 identity，旧重复修复 | S |
| `apps/desktop/src/main/hook-control/transport.ts` | WS hello/welcome/ping/pong/watchdog/backoff/401/standby | S |
| `apps/desktop/src/main/hook-control/dispatcher.ts` | ack accepted/queued/rejected，路径/session 校验，busy FIFO | S |
| `apps/desktop/src/main/hook-control/requestLedger.ts` | `connectionId+requestId` 去重与 ack 重放 | S |
| `apps/desktop/src/main/hook-control/{session-runner,turnObserver,outbound}.ts` | Agent 派发、progress/终态、离线交付 | S |
| `packages/slack-hook-protocol/src/types.ts`, `build.ts`, `parse.ts` | Hook 协议状态/reason/interaction 字段 | S |
| `apps/desktop/src/main/im/wechat/WechatIM.ts:60` | WeChat bot phase | S |
| `apps/desktop/src/main/localDb/worker/opHandlers/wechatTx.ts` | inbox/outbox 事务转移，租约/TTL/过载 | S |
| `apps/desktop/src/main/im/wechat/{taskStore,contextCrypto,permissionPolicy}.ts` | task 调度、上下文加密、权限策略 | S |
| `apps/desktop/src/main/localDb/schema.ts:595`, `:631`, `:648`, `:709`, `:741` | binding/sync/inbox/outbox/attachment schema | S |
| 真实上游 bot/hook 服务 | 认证、断网、媒体限制、服务端重放 | D |

## 11. Device Link

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `packages/device-link/src/client.ts:313` | stopped/connecting/online，hello/ack/ping/reconnect/reliable stream/invoke | S |
| `packages/device-link/src/allowlist.ts` | 远程 invoke/push 严格白名单与 timeout override | S |
| `apps/desktop/src/main/device-link/ownership.ts` | DB CAS 单 owner、5s heartbeat/15s stale、self-demote | S |
| `apps/desktop/src/main/device-link/*` | generation fence、push/outbox/重连/响应性保护 | S |
| `apps/desktop/src/main/localDb/schema.ts:1475` | `device_link_ownership` 持久化 | S |
| 真 relay/多设备长时运行 | handshake 兼容、两端故障、不确定 in-flight | D |

## 12. Terminal/文件/浏览器

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `apps/desktop/src/main/terminal/ptyManager.ts` | PTY create/attach/write/resize/exit/restart/dispose/owner transfer/env 清理 | S |
| `packages/file-browser-core/src/*` | 路径/symlink 边界、基础 fs 操作、ignore | S |
| `apps/desktop/src/main/file-browser/index.ts` | local/remote/device 统一服务与 IPC | S |
| `apps/desktop/src/main/file-browser/{remote,remote-file-cache,ssh-media,device-op,chat-file}.ts` | SSH/device 路由、大文件缓存、范围校验 | S |
| `apps/desktop/src/main/file-browser/search/index.ts` | fixed-string ripgrep/白名单/匹配上限/取消 | S |
| `apps/desktop/src/main/rsb-browser-bridge/registry.ts` | main TabRegistry 真值、owner/webContents 防 stale | S |
| `apps/desktop/src/main/rsb-browser-bridge/resource-watchdog.ts` | 30s CPU/内存检测、前后台/pin/audible 策略 | S |
| `apps/desktop/src/main/rsb-browser-bridge/ipc.ts` | capture/kill 路由与 sender ownership | S |
| 真 OS PTY/SSH/device/Chromium | 信号语义、网络中断、PID 共享/内存采样 | D |

## 13. Usage

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `apps/desktop/src/main/usage/modelUsageDelta.ts` | 累计→turn delta，reset/clamp，Vertex 晚到 | S |
| `apps/desktop/src/main/usage/turnCostCalculator.ts` | route 权威、actual/estimated、gateway/BYOK/subscription/DeepSeek | S |
| `apps/desktop/src/main/localDb/schema.ts:1172`, `:1211` | daily spend/model usage 表 | S |
| `apps/desktop/src/main/usage/__tests__/*` | 路由/货币/累计重置边界 | S |
| 真 gateway quote/provider usage | 价格、折扣、结算延迟 | D |

## 14. 更新/遥测

| 证据 | 位置/含义 | 级别 |
|---|---|---|
| `apps/desktop/src/main/updateService.ts:68` | idle/checking/downloading/ready/superseding/error | S |
| `apps/desktop/src/main/updateService.ts` | patch-info、替换/回退、apply attempts、auto-relaunch gate、子系统关闭 | S |
| `apps/desktop/src/main/__tests__/updateService.test.ts` | 状态转移、进度、gate 边界 | S |
| `apps/desktop/src/main/analytics-settings-store.ts:33` | privacy consent 和 analytics enabled 独立真值 | S |
| `apps/desktop/src/main/analytics-settings-store.ts:275` | 有效上报基础判定 | S |
| `apps/desktop/src/main/analyticsSettingsService.ts` | local runtime opt-out，legacy consent migration gate | S |
| 真 CDN/patch helper/OS relaunch | 签名、文件替换、处于 translocation/锁定状态的结果 | D |

## 15. 数据库/恢复

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `apps/desktop/src/main/localDb/schema.ts` | 全表/索引/枚举/check/default/FTS/vec 结构 | S |
| `apps/desktop/src/main/localDb/index.ts` | owner DB 打开/关闭、WAL、corruption backup recovery、worker takeover | S |
| `apps/desktop/src/main/localDb/schemaMigrationLease.ts` | reader/writer 租约，stale PID，损坏 fail-closed | S |
| `apps/desktop/src/main/localDb/schemaDrift{Core,Compatibility,Detector,Repair}.ts` | drift 发现，dev repair/release 告警，精确兼容 | S |
| `apps/desktop/src/main/localDb/migrations/*` | 0000–0089 渐进 schema，FTS/vec/Orca/remote/queue/MCP/media/WeChat/hook/subagent | S |
| 真断电/磁盘满/多版本共存 | WAL/备份/锁在 OS 故障下的结果 | D |

## 16. IPC 面证据

| 证据 | 支持的结论 | 级别 |
|---|---|---|
| `apps/desktop/src/preload/preload.ts` | Renderer 可见 API 与 invoke/send/on 映射 | S |
| `apps/desktop/src/main/maker-ipc/channels.ts` | Maker invoke/push 合约、输入验证、push 事件列表 | S |
| `apps/desktop/src/shared/authIpc.ts` | Auth IPC 合约 | S |
| `packages/device-link/src/allowlist.ts` | 可远程转发的 invoke/push 真子集 | S |
| 静态脚本扫描 preload | 主 preload 检出 797 个 invoke 调用（其中 750 字面频道）、29 send（其中 28 字面）、11 on、29 sendSync；常量/fan-out 使字面频道计数仅下界 | S（下界） |

## 17. 跨域推断与缺口

| 结论 | 证据链 | 级别 |
|---|---|---|
| 账号切换是全局隔离边界 | owner generation + DB reopen + owner ledger + owner credentials + 连接 generation | I |
| accepted 是“可能已有外部副作用”的关键界 | Maker send outcome + input queue + Scheduler/Orca dispatch rollback | I |
| 持久化真值与运行时投影被刻意分开 | sessions status vs Maker status；worktree JSON vs DB path；queue snapshot vs runtime；device owner row vs socket | I |
| 跨端/插件控制一律需 provenance，不相信业务 ID 自报 | Orca MCP context + plugin Full Access scope + Device Link allowlist | I |

未读取任何 `.env`、token、钥匙串、认证库或用户私密正文。
