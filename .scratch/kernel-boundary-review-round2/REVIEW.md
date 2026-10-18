# Review：`6e5bf33...134b8b3`

## Standards

1. P2，设计启发（possible Shotgun Surgery）：当前会话分别存在 Agent.full_session、BaseAgent.full_session、AgentState.session 与 CompactionMiddleware 缓存，切换需要多处同步且已漏更。建议在装配层统一当前会话所有权；随 R10 处理，不另开重复卡。
2. P3，明确规范违例：app-core/src/AGENTS.md:62 的 Analytics 仍写 phase_handler，与 R02 的 middleware 注册不符；违反 FRACTAL-DOCS.md 职责变更级联更新规则。归入 R13。
3. P3，明确规范违例：kernel-core/src/AGENTS.md:37–38 仍列迁出的 tool_lock.rs/pending_queue.rs，与同文件 :14 冲突；违反模块清单真实一致及级联规则。归入 R13。

## Spec

1. P1，R05：agent_spawn.rs:558 读取 store_session，但 tool_batch.rs:640–646 未注入，持久化子会话退回内存。违反保留上层 SessionStore/Inspector 完整能力。新卡 R09。
2. P1，R05：middleware.rs:212 缓存初始 session，swap_session 不更新；:398–413 将新会话压缩结果写入旧会话。违反切分契约后保留产品能力。新卡 R10。
3. P1，R01：agent_spawn.rs:663 只 with_bus，缺 workspace 配置，同文件相对/绝对资源锁可分离。违反保留 workspace 路径解析与同资源写冲突验收。新卡 R11。
4. P2，R03：历史回放已含排队事件，但下一批 listener 快照包含新订阅者，导致重复。违反历史→实时无遗漏无重复。实际补充测试 FAIL。归入 R12。
5. P2，R03：赢得投递锁的 append 仍 await 任意异步监听回调。违反慢监听者不阻塞追加。归入 R12。

定向既有测试 5/5 PASS；补充重复订阅测试 0/1 PASS，失败信息与源码在 evidence。R07/R08 未发现实质偏离，未发现需另列的 scope creep。未做全量测试，未修改生产代码。

Standards 共 3 项，最高为 P2 设计启发；Spec 共 5 项，最高为 P1 产品会话/资源锁回归。共建 5 张新卡（R09–R13），重复关联问题不重复开卡。
