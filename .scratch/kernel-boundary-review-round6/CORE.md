# Round 6 Spec 核心复审

固定范围：`5c64af16..5290e033`。只读生产代码；新增 probe 全在 `evidence/core/`。未调用真实外部引擎、网络服务或用户数据。R51 由主审负责。

## 确认问题

### C1 / P1 / R38：线程局部重入标记错误地阻断其它会话与异步任务

位置：`crates/alva-agent-core/src/agent_session.rs:596–602`（监听 await 在 621；native driver 非空重试在 747–753）。

`DELIVERY_ACTIVE` 是整线程单一 bool，既没有 session identity，也没有 async task scope，却跨 `on_event().await` 保存。源码“回调都在单次 poll 内同步执行”的前提不成立。两条独立动态反例：

- 无 Tokio：A listener 向独立 B append，B append 返回成功，但 B listener 收到 **0** 次；A 外层循环只排 A，不会帮助 B 清队列。B 无后续写入则一直不投递。
- Tokio current_thread：A listener 停在 pending 后，B append 触发自身 driver，但被误认作重入，B listener 仍为 **0**。队列非空使 driver 反复 `continue`；Tokio 的协作预算仍能让 timer 前进，不能夸大成整个 runtime 完全饿死。

动态证据：`cross_session.rs/.log`、`tokio_delivery.rs/.log`；两个断言均 FAIL（预期 1，实际 0）。修复应限定到确切同会话的重入/投递所有权，不能把 OS 线程当 async task；保留原同会话 inline 重入测试，同时增加上述跨会话与 await listener 回归。

### C2 / P1 / R50+R58：旧轮 cancel 的迟到 Unsupported 会中止新轮

位置：`crates/alva-hub-core/src/runtime.rs:997–1001`，`abandon` 在 517–552 按 product id 删除当前句柄。

确定性 gate：旧轮 cancel 已进入引擎但尚未返回 → 旧轮发 Completed → 同 pid 新轮发 SessionStarted，状态 Running → 旧 cancel 返回 Unsupported → 新逻辑调用不带 generation 的 abandon。新轮变 **Failed**，`has_live_run=false`。

动态证据：`stale_cancel.rs/.log`，FAIL。修复要求所有异步控制面结果携带所针对的 claim/generation；旧结果只能释放旧轮，不得触碰后来认领者。不能只把 generation 加在尾部 remove。

### C3 / P1 / R58：已弃置的 Starting 轮迟到登记仍覆盖新轮控制面

位置：`crates/alva-hub-core/src/runtime.rs:946–959`；claim 与 `engine.execute` 在 570–661。

确定性 gate：旧轮已 claim，卡在同步 `engine.execute` 尚未登记 → abandon → 新轮 execute/Started/登记 → 旧 execute 返回并无条件 insert → 覆盖新轮；旧轮收到已取消 token 后尾清理只删除“自己的”代际，从而把表清空。最终新轮 **Running**，`has_live_run=false`。

动态证据：`late_registration.rs/.log`，FAIL。新增 oneshot 只保证自身清理不早于登记，不能保证该登记仍属于当前 claim。需要 claim/registration/abandon 的一致性检查，并验证旧同步启动失败的 rollback 也不能覆盖新轮。

### C4 / P2 / R48：swap × child 组合测试仍未验证声明的关键行为

位置：`crates/alva-app-core/tests/r48_swap_child_identity.rs:149–155,172–178,182–223`。

确实通过真实 SubAgentPlugin 启动了 child；但启动 swap_task 后立即释放 child，无握手证明 swap future 已开始等待。所谓“next root turn”只读取 session_handle，从未执行下一轮，也未查询 child 自身后续 records 与 parent id。因此坏实现让 swap 过早落地、第二轮仍写旧会话、child 自身串线，均不一定被本测试抓住。

保留真实 spawn 与 root ChildSessionLinked 断言。补 swap 尝试/阻塞 gate，执行下一轮并核对新旧日志；捕获/查询 child 身份和后续消息。生产 Inspector root→child→grandchild 重开查询新增部分有效，勿重做。

### C5 / P2 / R48 的 R11 补充：顺序执行不能冒称多 child 并发矩阵

位置：`crates/alva-app-core/tests/r11_tool_lock_matrix.rs:230–270,273–313`（253–255 明写 agent 工具 sequential）。

同资源测试让外部手工 hold 拒绝两个按顺序到来的 child（count=0）；不同资源测试只证明两个顺序调用均执行（count=2）。这证明 registry 路径接线，但没有两个运行中 child 相互竞争，也没有重叠执行窗口；把所有 child 全局串行化仍能全部通过。

保留路径规范化/外部 hold/读读结果；补两个真实 child 的 gate 与 active/max-active 断言，验证同资源排他和异资源/读读重叠。

### C6 / P2 / R48 的 R10 补充：A 会话负断言指向未使用的 fixture

位置：`crates/alva-app-core/tests/r10_compaction_swap_chain.rs:125–144,181–193`。

新建 session_a 后从未传给 agent；第一轮运行在 builder 自带的另一个 session。最后断言空的 session_a 无 marker 是恒真，不能排除初始真实会话被污染。B→C→B 及 B 中 summarizer/marker 的断言有效。把 A 绑定真实初始 session（或先真实 swap 到 A），断言首轮消息存在，再验证无污染。

### C7 / P2 / R52：五入口五剧本仍未闭环

位置：`crates/alva-engine-adapter-alva/src/adapter.rs:623–683,783–795`；`crates/alva-engine-runtime/src/AGENTS.md`。

Alva 现有 Normal/SpawnFailure/EofWithoutResult/OutputAfterFatal 四个 harness 调用，缺 KilledMidRun。Claude Full 仍缺全部 harness；quarantine 阈值、解除与路由消费保持未实现。执行记录已诚实把 Full/quarantine 留 open，但漏列 Alva KilledMidRun。Pi/Claude Print/Codex 五剧本静态接通；新 harness 对缺 fatal 的变异拒绝有效。

### C8 / P2 / R57：默认中文审批卡隐藏工具自己提供的英文效果说明

位置：`crates/alva-hub-desktop/web/src/components/hub-timeline.tsx:182–188`，`web/src/i18n.tsx:1480–1483`。

Rust summary → ApprovalRequest → Alva PermissionRequest.description → Hub DTO 已接通；但最终中文卡片使用 localizeSystemText，它把未收录字典的英文句子替换成通用 diagnosticUnavailable。R57 新测试自定义摘要 `publishes the draft post to the production blog` 恰好触发此分支。因此最终用户仍看不到工具自述的具体操作。工具提供的自由文本不应套产品文案的未知英语隐藏规则；保留原文（可附翻译），加中文 locale 的真实 permission 卡渲染验收。主审/UI 审查可补动态断言。

## 每卡结论矩阵

| 卡 | 结论 | 证据/边界 |
|---|---|---|
| R38 | 不通过，新增回归 | C1，两种 executor 独立 probe FAIL |
| R39 | 局部修复有效 | Drop 关闭+notify 与最终排空存在，独立退出信号测试有效；整体广播仍受 C1 影响 |
| R48 | 部分完成 | Inspector 真实逐级查询有效；C4/C5/C6 验收缺口保留 |
| R49 | 代码修复有效，组合验收仍少一段 | 终态前补 open ToolEnd，已闭合不重复；新增测试有效；卡要求的生产 side_effect SQLite 重开未在本轮新增 |
| R50 | 常规场景有效，代际交错失败 | 无能力/Unsupported 可释放；C2 |
| R51 | 主审负责 | 不在本报告重复结论 |
| R52 | 部分完成，必须 open | 新 fatal 变异门有效；C7 |
| R57 | DTO/TUI 有效，Hub 中文端失败 | C8；URL 二次审批还显式 None，未补该链 |
| R58 | 不通过 | 已修快结束自身清理及旧尾误删；C2/C3 仍 FAIL |
| R60 | 修复有效 | 两条 helper 已 await，结果错配和 batch error 均有真实断言，paired==2 |
| R61 | 修复有效 | 正式 build_engine_for_target 被测试调用；classify_engine/enforcement_for 直接真实 API；未复制生产构造逻辑 |

## 验证

- `cargo run --offline --manifest-path .scratch/kernel-boundary-review-round6/evidence/core/Cargo.toml --bin cross_session`：FAIL，B=0，预期 1。
- 同上 `--bin tokio_delivery`：FAIL，A listener pending 时 B=0，预期 1。
- 同上 `--bin stale_cancel`：FAIL，新轮 Failed，预期 Running。
- 同上 `--bin late_registration`：FAIL，新轮 Running 但 live=false。
- `cargo test --offline -p alva-agent-core -p alva-hub-core -p alva-agent-security -p alva-engine-runtime --features alva-engine-runtime/test-support --lib`：agent-core 68/68、security 193/193、engine-runtime 53/53、hub-core 40/40，合计 **354/354 PASS**，命令 exit 0。见 `evidence/core/core-tests.log`。

动态 probe 使用独立 Rust workspace，本地路径依赖指向审查 HEAD；最终四条日志来自根 Cargo.lock 派生依赖复跑（Tokio 1.52.3），四条命令均 exit 101，且失败原因均为契约断言，不是编译失败。只运行内存 fake engine、session 与 gate。无生产源变更。
