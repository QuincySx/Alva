# 历史 Core 工单复核（01–19、R01–R13）

审查基准：`5c64af1640ab00b6a64aa3474112b62fe4106036`。逐卡读取原始需求及当前源码，不将 archived/done 当作验收。静态认可表示已核对主要实现，**不等于全量测试通过**；历史中间提交的全绿与“断言零改动”未重新执行。与第五轮 R38–R48 重叠的问题只引用，不重复开卡。

## 主要风险与新发现

1. **P2 / 原 01：公开表面快照存在确定性漏检。** `scripts/public-surface.py:329` 的名称匹配不接受 `async fn` 前缀；`:342` 对 struct/enum 只记录名称，不记录字段/变体，trait 分支亦不记录 supertrait 和 associated type。新增公开 async 函数、修改 DTO 字段类型或增加枚举变体均可能不改变快照，不能兑现“任何公开符号增删改都被记录”。真实 `alva-kernel-abi/src/runtime.rs:52` 的公开 `timeout` 也不进快照。已通过直接调用 scanner 的动态探针确认，见下方结果。应覆盖签名、公开成员、trait 约束及异步函数，避免仅检查普通 `pub fn`。
2. **P2 / 原 04：工具自述未进入审批提示。** `request_escalation.rs:134` 确实生成原句，但 `agent-security/src/guard.rs:334` 只将 `effect.summary` 写 debug；`middleware/security.rs:319` 的 `ApprovalRequest` 仅传 request_id/name/arguments。真实 UI 通道没有 summary，`requires_action` 也只消费 tool_call。原卡“审批提示语与改动前逐字一致”未实现；已有测试只直接调用描述方法检查内容，不能证明提示接线。需把自述传入真实审批 DTO 与显示路径，并用正式中间件+notifier 验证。
3. **验收缺口 / 原 17、R07：第五引擎仍由测试复刻装配。** `tests/readiness_tests.rs:733` 的 `test_build` 自己找 blueprint、协商并直接调用 `entry.build`，未走公开 `build_engine_for_target`；生产装配当前静态上已查注册表，但测试不能证明正式入口可构造第五引擎。R07 明确要求不重复实现装配逻辑。
4. **验收缺口 / R10、R11：组合矩阵缺失。** R10 已动态解析当前 capability，但未找到 A→B→C→B 下真实 CompactionMiddleware 执行的回归；现有 swap 测试主要验证共享身份、并发/flush。R11 有父绝对路径→子相对路径与 registry 归一化测试，未见其要求的反向、两子 agent、不同资源和读读共享全套生产路径矩阵。不能仅凭单测名字或共享 cell 推导组合全部通过。
5. **P2 / 原 07：两条工具错误补齐回归是空测试。** `tests/tool_batch_coordinator.rs:607`、`:616` 调用 async helper `assert_no_dangling_calls_with` 却未 `.await`，before/wrap 普通错误路径的执行与全部断言均未发生；本轮 cargo 编译输出 `unused implementer of Future` 明确证实。须 await 真正场景再判断生产实现是否正确，不能以现有测试绿证明验收。
6. **原 16 数值指标未达。** 当前 `src/**/*.rs` 总行数 kernel-abi=3765、kernel-core=5455，原 README 指标约2500/3500。尚无新的计量口径或获接受的目标调整，归入未验收；不建议为了凑数盲删必要契约。

## 逐卡状态矩阵

路径省略 `crates/` 前缀；“现有测试”表示读取过测试，不自动表示本轮已运行。

| 工单 | 当前状态 | 源码/测试核对证据与限制 |
|---|---|---|
| 01 快照闸 | **确认缺陷** | CI workflow 调用 public-surface.sh，checked-in 快照存在；scanner 漏检已动态确认，见风险1。Registrar 规则由快照+review，而非额外语义 lint。 |
| 02 删除死表面 | 静态认可 | kernel-abi `tool/types.rs` 只剩7方法；宏不再生成 is_destructive；旧多模态/白名单出口未见；全 workspace、task/team 与 wasm 未重跑。 |
| 03 Scheduling | 静态认可 | `tool/types.rs:152` 单入口；tool_batch:303、tool_timeout:73、tool_lock:935 读取描述符；宏生成 Scheduling。现有并行/冲突/取消测试已读取。 |
| 04 自述/权限 | **确认缺陷** | EffectDescription 无 deny，SecurityGuard 拥有决策；自述文本只 debug，未传审批，见风险2。 |
| 05 契约 expand | 静态认可 | ConversationLog、SessionStore 独立，AgentSession 是组合；最终形态正确。原 expand 提交的“零调用修改/全绿”不以最终源码代证。 |
| 06 最小日志/广播 | **确认缺陷（沿 R38/R39）** | state.rs:28 为 dyn ConversationLog，读写可失败且 run 最终/周期 flush；广播遗留沿既有 R38/R39，不重复报告。 |
| 07 工具错误补齐 | **确认缺陷（验收测试）** | 生产有错误补齐逻辑；before/wrap两测试未await async helper，完全没有执行场景/断言，见风险5；after/取消测试需独立验证。 |
| 08 锁搬迁 | 静态认可 | 表/取锁在 agent-core/tool_lock.rs；kernel 仅 Scheduling 波次；子栈接同 bus 与 workspace。R11组合未全验收。 |
| 09 内存会话搬迁 | 静态认可 | 两实现位于 agent-core/agent_session.rs；AgentBuilder 默认 InMemoryAgentSession；内核无默认存储实现。 |
| 10 压缩保日志 | 静态认可 | context_apply 在 agent-context；middleware:401 当前 store 追加 marker + compact_messages，不 clear；有压缩前 LLM/tool 因果保留测试。真实切换组合见R10。 |
| 11 seam/真流式 | 静态认可 | loop_impl.rs 单 drive(AgentEventSender)；Agent::run 以 run_step 交替驱动，native run_bounded 独立生产驱动；两个运行中提前产事件测试。保留 unbounded 是已写 ADR 的兼容口径。 |
| 12 agent-abi expand | 静态认可 | agent-abi 持 Task/Scope/Spawn/Phase/Analytics/Provider；session_record/context 留 kernel 是最终接受边界；不以最终代码重证过渡提交。 |
| 13 SDK 迁移 | 静态认可 | agent-core/context/builtin 消费 agent-abi 产品概念，kernel 路径不再转出这些类型；完整 all-features 未跑。 |
| 14 产品迁移 | 静态认可 | app/host/adapter 取 agent-abi；kernel session_record 是有意保留的机制词汇；完整 workspace+wasm 未跑。 |
| 15 Analytics | 静态认可 | analytics.rs:65 注册单一 raw middleware，:245 wrap LLM，从 response usage 取token/cache；错误不发埋点，cost保持0；生产接线测试存在。 |
| 16 contract边界 | **未验收** | CI Rule19 查询传递依赖，旧产品re-export删除；行数指标未达；原01快照不完整亦影响收口证据。 |
| 17 第五引擎 | **未验收** | alva.rs:89 经 blueprint 构造；permissions/readiness 生产路径查自述；测试复制生产入口，见风险3。 |
| 18 编排拆分 | 静态认可 | store/orchestration.rs::decide_claim 纯数据决定，team.rs 查库/应用决策；无新trait，TaskClaims中性共享。历史“零行为变化”未全量比对运行。 |
| 19 ADR/文档 | **确认缺陷（沿 R46）** | ADR0008/0009/0010及0007粗粒度修订齐全；根CONTEXT已存在；局部旧模块/职责仍不一致，沿R46处理。 |
| R01 子锁 | 静态认可 | agent_spawn:654 显式挂 ToolLockMiddleware，同一 registry；review_wiring_regressions 有真实子调用 SerialGlobal 锁测试。 |
| R02 生产埋点 | 静态认可 | AnalyticsPlugin 仅一次 r.middleware；正式插件产 LlmCall 测试、逐字段测试存在，未见重复 tool phase 注册。 |
| R03 广播顺序 | **确认缺陷（沿 R38/R39）** | delivery_lock 串行实际投递；非Tokio回调重入与driver退出既有返工尚未完成。 |
| R04 启动失败收尾 | 静态认可 | run/mod.rs:192 收集启动错误，跳过body，仍on_agent_end/dispose/run_end/flush/AgentEnd且保存根因；真实fail首append测试存在。 |
| R05 最小内核 | 静态认可 | minimal_log_consumer 只实现ConversationLog，state/ChildAgentParams以此接入；工具完整store显式capability，不强制最小日志。 |
| R06 分形文档 | **确认缺陷（沿 R46）** | 迁入目录地图部分已同步；security/src/AGENTS仍写Tool::check_permissions，guard文件头仍提ToolPermissionResult；kernel-core顶层仍登记迁走的context_apply/tool_lock。 |
| R07 测试helper | **未验收** | 生产export已删除，但readiness测试把同一装配复制到私有test_build，违背不重复装配逻辑的验收项。 |
| R08 TaskClaims | 静态认可 | orchestration.rs:40共享中性TaskClaims，holders/older_waiters保留角色区分，未新增抽象。 |
| R09 持久子会话 | 静态认可（组合见R48） | builder→Arc<dyn PersistentSessions>→tool_batch同typed key→spawn专属child桥→repo；真实递归测试已新增，swap+Inspector组合沿R48。 |
| R10 切换压缩 | **未验收** | middleware动态store、swap同锁共享cell静态正确；未找到真实压缩A/B/C组合测试，不能宣称完整验收。 |
| R11 子workspace锁 | **未验收** | child明确with_workspace；现有child_lock_workspace_tests两条仅覆盖部分矩阵，见风险4。 |
| R12 广播交接 | **确认缺陷（沿 R38/R39）** | listener游标解决历史重复；native独立driver追加解耦；非Tokio回调/弃置driver未完，沿现卡。 |
| R13 文档残留 | **确认缺陷（沿 R46）** | app-core Analytics raw middleware描述已改，但kernel-core目录仍把已迁出文件登记为业务行，顶层职责亦陈旧。 |

## 本轮验证证据

直接载入 `scripts/public-surface.py`，分别对两份合法 Rust 声明调用 `scan_scope`：

```text
before: pub struct Foo { pub value: u32 }; pub enum E { A }; pub trait T: Send { type Item; }
after:  pub struct Foo { pub value: String, pub added: bool }; pub enum E { A, B }; pub trait T: Sync { type Changed; }
两者输出完全相同：enum E / struct Foo / trait T
pub async fn new_public_api() {} → 空记录
pub fn new_public_api() {} → fn new_public_api fn new_public_api()
```

这验证的是扫描器行为，不依赖臆测或历史状态。定向 Rust 测试命令：`cargo test -p alva-kernel-core --test minimal_log_consumer --test tool_batch_coordinator --offline`，执行完成 exit 0：最小日志 1 passed / 0 failed；tool_batch 7 passed / 0 failed。但其中 before/wrap 两条未await，属于空绿，故只能确认其余5条实际执行测试通过；原07完整验收仍不成立。编译已确认两条未await测试警告。未运行全 workspace、wasm、真实CLI或付费provider。

## 第三轮 R14–R19 补充矩阵

已逐卡阅读第三轮需求，并对照同一 HEAD 的最终接线；不把旧卡记录的 655 tests passed 推导为当时真实产品验收。

| 工单 | 当前状态 | 当前证据及承接 |
|---|---|---|
| R14 递归父仓库 | **未验收（沿R48）** | 最终R24/R27已同typed key下发dyn能力，spawn用CellBackedSessions::wrapping(child_session)，真实root→child→grandchild仓库测试存在；根swap不重绑运行中child及重启后真实Inspector组合仍由R48补齐。 |
| R15 原子swap | **未验收（沿R48）** | BaseAgent::swap_session在state锁内flush、替换日志和cell；并发swap、flush拒绝与运行中turn gate测试存在。与正在压缩/派生子的联验未找到，不能用身份单测替代。 |
| R16 独立投递 | **确认缺陷（沿R38/R39）** | native driver目前接管追加，真实abort/live订阅测试已修；无Tokio回调重入及idle driver退出缺陷沿R38/R39。 |
| R17 reset游标 | 静态认可（文档沿R46） | clear使用generation，wants/advance_after拒绝旧代际推进新游标；现有clear中途/restore后/rollback后订阅测试可定位。restore/rollback并未如新文档所述递增代际，文档纠正沿R46；本轮不重复推断新的并发失败。 |
| R18 内核storage边界 | 静态认可 | R27最终删除kernel-core对StoreSessionCell识别及cell桥；tool_batch/runtime_context/run_child只消费Arc<dyn PersistentSessions>，桥实现在agent-core。 |
| R19 接口职责文档 | **确认缺陷（沿R46）** | kernel-abi与agent-core已登记窄能力桥；agent-core AGENTS仍虚称restore/rollback均锁内重置代际，且迁出模块地图不全，沿R46。 |
