# Hub 引擎 seam —— 工单索引

> 背景与核心判断见 [README.md](./README.md)。工单正文在 [issues/](./issues/)。
> 每张工单含：级别 / 影响 / 状态 + 证据（file:line，均已回代码验证）/ 复现 / 工作 / 验收。

## P0 —— 今天就在骗用户或花冤枉钱

| # | 标题 | 一句话影响 |
|---|---|---|
| [01](./issues/01-codex-pi-fatal-no-break.md) ✅ | codex / pi 在 fatal Error 后不停止读取循环 | ~~时间线已标「失败」，助手还在继续追加正文和工具活动~~ **DONE 2026-09-01** |
| [02](./issues/02-alva-adapter-silent-missing-completed.md) ✅ | alva adapter 两条早退路径漏发 Completed | ~~本地 panic 被伪装成引擎故障，自动换引擎重跑~~ **DONE 2026-09-01** |
| [03](./issues/03-permission-result-discarded-audit-lies.md) 🔶 | 审批结果被丢弃 | ~~引擎早已拒绝，审计却说「已允许」~~ **alva 侧 DONE 2026-09-01；claude 侧见 18** |
| [04](./issues/04-gpui-port-regressions.md) ✅ | GPUI 移植走样 | ~~Interrupt 按钮保证无效 + Pi 加不进降级链~~ **DONE 2026-09-01** |
| [05](./issues/05-hub-server-engine-handle-leak.md) ✅ | hub-server 错误路径泄漏引擎句柄 | ~~之后点「停止」返回成功、实际什么都没停~~ **DONE 2026-09-01**（顺带修了 gpui 完全无清理） |
| [06](./issues/06-required-capabilities-vocabulary-collision.md) ✅ | required_capabilities 词表对撞 | ~~任何模型能力词进入该字段 → 降级目标被永久静默否决~~ **DONE 2026-09-02**（路线 B：白名单 + veto 只看引擎词） |
| [17](./issues/17-claude-adapter-fatal-no-break.md) ✅ | claude 两个 adapter 疑似同一 R2 漏网（**验证：不成立**） | 同 01；但 claude 每个 fatal 都与 Completed 成对产出、读取循环成对 break——**CLOSED 2026-09-06**（验证结论 + 防御性 pin 测试） |
| [18](./issues/18-claude-bridge-permission-ack.md) ✅ | claude 全模式审批 ack 只证明「写进了 stdin」 | ~~同 03 的另一半；需跨语言协议往返设计，三步不能拆开做~~ **DONE 2026-09-06**（accepted/rejected 语义帧 + pending oneshot + 超时 + 60s 下沉 Rust） |

> 06 是**埋着的雷不是正在爆的雷**：出厂 UI 路径今天一律写 `capabilities: []`。修在填充数据之前是几行的事，填完之后是线上事故。

> **P0 全部完成**（01–06）。02/03 各有一半拆到工单 18；03 的 claude 侧与 06 的字段分离分别归工单 18 与 15。

## P1 —— 结构性但局部

> 多数今天被「旗舰产品每轮重建 `HubRuntime`」掩盖，只有 hub-cli 持长生命周期实例。
> `docs/specs/hub-headless-service.md` 路线 A 的常驻 hub-server 一旦落地，07/08/09 当天变成线上并发 bug。
> **07/08/09/12 已于 2026-09-06 随 `46a2xxx`（claim_seq + abandon + 终端态收窄 + ToolEnd 合成）一起落地**；剩余 10/11 是各自独立的工程面改动。

| # | 标题 | 一句话影响 |
|---|---|---|
| [07](./issues/07-session-state-clobber-claim-seq.md) ✅ | 旧轮回滚快照覆写新轮状态（引入 `claim_seq`） | ~~同一会话可能同时跑两条引擎流；致命失败的一轮被显示成「已完成」~~ **DONE 2026-09-06** |
| [08](./issues/08-abandon-verb-stuck-session.md) ✅ | 会话卡在 Starting/Running 无出口（补 `abandon`） | ~~消费者提前放弃某轮 → 该会话**永久**不可用，且此时连取消都调不动~~ **DONE 2026-09-06** |
| [09](./issues/09-restore-session-illegal-state.md) ✅ | `restore_session` 可直接种出非法状态 | ~~恢复时传错状态值 → 会话一落地就永远报 "already running"~~ **DONE 2026-09-06** |
| [10](./issues/10-cancel-semantics-three-tiers.md) 🔶 | cancel 是三档而非一档 | ~~显示「已停止」但子进程仍在计费；另有窗口期停不掉、180s 后自动换引擎重跑~~ **主体 DONE 2026-09-06**（契约文档 + 三档 adapter 的 cancel terminal 测试 + claude full grace→kill + bridge 对齐 + Hub 两级）；桌面 Stop 时序随工单 13 收口 |
| [11](./issues/11-resume-failure-self-heal-is-dead-code.md) ✅ | resume 失败的自愈路径生产永不触发 | ~~会话被永久焊死，每次都拿同一个失效 id 去 resume~~ **DONE 2026-09-06** |
| [12](./issues/12-unclosed-tool-synthesize-toolend.md) ✅ | 未闭合工具卡片永远停在 running | ~~已落盘，刷新和重开应用都恢复不了~~ **DONE 2026-09-06** |

## P2 —— 架构收口（修的是「明天不会更糟」）

| # | 标题 | 一句话影响 |
|---|---|---|
| [13](./issues/13-model-the-run-not-the-engine.md) 🔶 | 给「一次运行」建模，而不是给 engine 建模 | ~~Hub 内核没有能停下那一次运行的句柄；三份手抄影子表纪律已分叉~~ **hub-core RunHandle（批次 1–3）DONE 2026-09-06**：`RunHandle{abort, engine 强引用, engine_session_id}` + `runs` 表 + cancel/respond_permission 优先走 run 句柄 + abandon 收编 + unregister/`has_live_run`；**产品三份影子表收编（批次 4）为后续增量** |
| [14](./issues/14-decide-switch-handoff-authority.md) ✅ | 拍板 switch / handoff 的权威层 | ~~一个用户动作两套实现，旗舰走的是另一套；内核那套已与 ADR-0002 矛盾~~ **DONE 2026-09-06**（ADR-0002 Authority 节：store/branch 层权威；hub-core 定位为瞬时 dispatcher；D-2 无 resume 引擎退化为 fresh；D-1 记为有意范围） |
| [15](./issues/15-engine-descriptor-and-negotiation-uplift.md) | EngineDescriptor + 协商模块上移 | 第五个引擎注册成功后被四道 `match` 拒绝；每处判断被两个前端各抄一份 |
| [16](./issues/16-stream-contract-conformance-harness.md) ✅ | 流契约 conformance harness + 违约处置动词 | ~~改 pi / claude-print 的映射导致违约，`cargo test --workspace` 全绿~~ **DONE 2026-09-06**（alva-engine-runtime 导出五剧本 harness；pi / claude-print 零断言缺口补齐；codex / alva / claude-full 接入） |

## 依赖关系

```
08 (abandon 动词) ──────────► 13 (给「一次运行」建模)
                                 │
14 (拍板 switch 权威层) ─────────┤  两者都应先于 15，
                                 │  否则 descriptor 会被塞进一个待下线的状态机
                              15 (EngineDescriptor + 协商模块上移)
                                 ├─ 前置：04 / 05 / 06 先止血，避免上移时把已知 bug 一起搬走
                                 └─ 产出：订正 ADR-0007 Finding 2

16 (conformance harness) —— 独立，可随时开工；越早越能接住 01 / 02 的回归
```

## 建议节奏

1. **01–06 单独成 commit 各修各的**，都是当天量级，且 01/02/03 直接消除「系统说谎」类问题。
2. **16 紧随其后**——它是 01/02 的回归网，先有网再动 adapter 更安全。
3. **07/09 一起做**（同一文件的状态机守卫），**08 接着做**（它是 13 的最小前置）。
4. **14 需要你拍板**，不是纯技术工作：hub-core 的 switch/handoff 整块下线，还是把 store 那套上提。工单里写成了待决选项，附带两条路各自的代价。
5. **13 / 15 是真正的架构投资**，等 14 拍板后再开工。

## 验证约定（沿用项目惯例）

每张工单完成时：代码 + 测试 + 相关 `AGENTS.md` 更新 + 工单状态改 DONE + 单独 commit + 子 agent review。
涉及 `alva-engine-runtime` / `alva-hub-core` 契约面的改动（10 / 13 / 15 / 16）额外跑 `scripts/ci-check-deps.sh`。
