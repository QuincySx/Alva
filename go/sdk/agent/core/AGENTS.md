# L2.5 · sdk/agent/core
> 无默认产品政策的 SDK Plugin 装配与 Agent 入口。

## 地位
L2.5；只依赖 agent/kernel/wire 与标准库，不能依赖 harness、platform、apps、integration 或 UI。

## 逻辑
Builder 先验证 plugin 重名，再注册 tool/middleware/prompt/metadata/capability。
完成所有 Register 贡献后封闭 Registrar 和 bus，再按顺序执行可失败 Finalize，晚期只读能力可见完整
Register 装配快照、Model 和独立 ToolDefinitions。Finalize 可返回新 tools，由 Builder 校验重名并接入真实执行，
单独记录在各 plugin 的 FinalizedTools；不是通过保留可写 Registrar 追加。
LateContext.Session 是真正 reader-only wrapper（ID/messages），不能通过类型断言恢复 append/flush 权限。
Registrar.ContextSource接受独立只读工作视图贡献，不覆盖ContextSystem或原始ConversationLog；重名、typed-nil及封闭后的贡献明确拒绝。
Agent.Run 委托可替换 AgentLoop，并拒绝同一 Agent 并发 run。MemorySession 是 SDK 的原子内存 reference backend。
Build在任何贡献实现/回调前捕获完整Builder配方；Register/Finalize通过原Builder配置的改变仅用于下次Build，不能让未Register插件进入本次Finalize或改写本次loop/prompt/config。

## 约束
SDK 对 plugin/tool/middleware 重名 fail-fast；同名插件替换**默认插件**只由 harness 实现，不是 SDK 的覆盖行为。
Registrar 仅同步装配期使用，不能保留后异步写入。Finalize 除显式返回 tools 外不能改动注册集；
所有 finalizer 都观察同一份完整 Register 贡献，而不是依赖之前 finalizer 的副作用。
Builder与Registrar仅同步使用，不保证与setter并发。Register/Finalize前后检查取消，取消后不再开始下一回调或返回Agent；不强制终止不合作的回调。任何退出路径（含panic展开）都封闭已交出的Registrar/bus，原panic仍向上传播。
ContextSystem在setter/Registrar注册时复制描述符；Run另复制context描述符和middleware切片，隔离替换loop对后续Run的配置修改。Hooks/Handle/Apply/Counter及middleware实现保持显式共享身份，内部可变状态与生命周期由实现负责，不冒充深克隆服务。
Plugin/Middleware/Command身份和Command说明须为合法UTF-8；无效贡献在后续消费者IO前失败。
MemorySession 批量预校验并增量更新 facts/messages/indexes，不在每次 append 重新投影全部历史。
任意未知合法 extension 原样存储且不进 prompt；存储归属和 causal parent 必须校验。
运行行为统一由Middleware贡献，不再有PhaseHandler/Effect/Order执行注册表面，也不保留无生产注册或消费方的Phase metadata注册；agent ABI的Phase词汇独立保留，command仍仅metadata。
Builder.Middleware和Registrar.Middleware只接受一个可选装配order，默认3000，同值保持注册顺序；较小order先进入，after/返回反序。实现自身不携带Priority；直接kernel调用方提交的slice顺序就是执行顺序。
依赖在构造、Register或确有晚期需求的Finalize中接线，不再执行Middleware.Configure。
RunWithConfig只覆盖一次模型设置，不污染装配默认prompt/config。Agent.State是真实mutable state；同Agent并发Run仍拒绝。
ToolLockRegistry是可选跨Agent资源锁；Coordinator不重复加锁，全局屏障与冲突等待可取消，锁申请整体原子。
非ConcurrentSafe工具即使未声明资源，在跨Agent registry内也保守作为全局屏障；不把空claims当作可安全并发。
单函数适配器覆盖已有11个typed hooks，每个只实现一个目标可选接口；nil函数、空/非法UTF-8名字在注册时失败，直接调用无效适配器也返回error。没有空hook或透传wrapper，审计只计真实调用。
PrepareTool适配器原样转发Runtime与本轮ActualTool，和提交后的BeforeTool/WrapTool严格分离，不让AEP改参绕过本轮工具绑定与资源规划。
Analytics/provider工厂属于agent独立能力和ABI，不藏入core；kernel真实hook与LLM cache指纹/无正文流摘要已有耐久审计，其余Rust phase timeline与完整fixtures尚未逐项等价验收。

普通单点贡献可在Plugin.Register中直接写`return r.Middleware(core.BeforeModel("my-context", fn))`；需要组合多钩子时注册多个适配器，或提供实现所需typed接口的现有Middleware。无需Phase/Effect/Order声明。少数顺序要求写在注册处，如`r.Middleware(layer, 1500)`；Finalize观察的Assembly.Middleware就是排定后的真实顺序。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| SDK entry | `agent.go` | Builder、Agent、loop 替换、assembly snapshot |
| Assembly ownership | `assembly_test.go` | 回调改Builder仅下次生效、取消不出版、panic封闭、context/每轮配置所有权与非法身份拒绝 |
| Extension assembly | `plugin.go` | Plugin/Finalize/Registrar/LateContext 与贡献元数据 |
| Reference store | `session.go` | 原子内存 log、typed outcome 索引、records query |
| Execution regression | `agent_test.go` | 真 model→tool→model、取消/错误/插件/投影/替换 |
| Typed-nil regressions | `nil_test.go` | executable model/log/loop/plugin/tool/middleware/context与nil hook在调用方法/Plugin IO前拒绝；empty log ID同样fail-fast |
| Store regression | `session_test.go` | 批量原子性、causal identity、extension 与 copy |
| Backend conformance | `session_contract_test.go` | 运行kernel/abi/sessiontest的跨后端共同事实语义；容量政策由后端独立验收 |
| Response capacity | `response_limit_test.go` | AfterModel不能扩张响应突破wire与原子结算共同上限 |
| Function hooks | `middleware.go`, `middleware_test.go` | 单函数窄接口、11个真实hook/审计、装配局部顺序、nil拒绝与失败收尾 |
| Shared locks | `tool_lock.go`, `tool_lock_test.go` | 跨Agent公平冲突队列、reader共享、取消与coordinator |
| Runtime regressions | `state_hooks_test.go`, `config_test.go`, `concurrency_test.go` | live state、每次配置与真实并行settlement |
| Preparation regression | `prepare_test.go` | 原声明/有效参数、冲突重算、替代结果、原子失败与改参拒绝 |
| Wrapper lifetime | `tool_contract_test.go` | next lease、迟到拒绝、真实错误不可吞、mixed veto与after收尾 |
| Durable output | `committed_event_test.go` | 完整assistant耐久事实/实际ID、deepcopy、失败append/partial与投递失败settlement |
| Model binding | `model_binding_test.go` | BeforeModel生效、wrapper/并发SetModel下次生效、多next仍绑定同实际模型 |
| Model lifetime | `model_lease_test.go` | 已进入next真实join、迟到next/callback拒绝、transport retry及并发chunk协议错误保留 |
| Invocation acceptance | `invocation_test.go` | 真实身份/会话与工具目录绑定、只读准入/修复重试、公开失败与重复continuation |
| Working source acceptance | `context_source_test.go` | 贡献封闭/重复/typed-nil、实际只读reader、失败不回全量与原事实保留 |
| Hook audit | `hook_audit_test.go` | 阶段而非逐层写、洋葱顺序/真实call、固定错误无正文、settle后审计失败/取消/veto回归 |
| LLM audit | `llm_audit_test.go` | 2000chunk只产固定审计条数、正文/私有错误不泄露、cache显式0与审计存储失败拒绝Tool并配对结算 |
