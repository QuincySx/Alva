# ADR-0008: 内核与 agent 契约分层（含对话记录与会话存储拆分）

- Status: Accepted
- Date: 2026-09-09
- Scope: alva-kernel-abi, alva-kernel-core, alva-agent-abi（新建）, alva-agent-core
- Supersedes: 内核边界重构（.scratch/kernel-boundary, tickets 05/06/12/13/14/16）

## Context

重构前，`alva-kernel-abi` 同时装着两类东西：**循环真正需要的**（消息、工具、
模型、取消、错误、调度词汇）和 **agent 产品概念**（任务、作用域、派生、阶段、
埋点、provider 注册表）。会话契约更是一个三十方法的巨物：读消息、追加、查询、
计数、回滚、快照、订阅、压缩、恢复、刷盘、关闭、清空全在一个 trait 上。

后果有三：

1. "极其通用的内核"是愿望不是事实——第三方实现一个工具要面对二十多个方法，
   换一个会话后端要实现三十个方法。
2. 产品概念与循环机制互相锁死：动任务类型会碰内核契约的快照，反过来内核
   契约的任何收缩都要惊动产品层。
3. 会话契约里沉积着**合约谎言**：文档承诺"关闭之后其它操作返回错误"但大半
   方法没有错误返回能力；压缩借"清空整个会话"实现，把 Inspector/审计/因果
   图一起删掉。

## 判据（本 ADR 的核心）

> **一个只会翻译文本的 agent，如果不需要某个概念也能跑起来，那个概念就不在内核里。**

以及反面：

> **长期没有第二个实现者、也没有真实消费者的 trait 方法或模块，不是"给未来
> 留的扩展点"，是死表面。**

## Decision

### 1. 三层契约

```text
alva-kernel-abi   循环词汇：消息/工具/模型/取消/错误、ConversationLog、
                  SessionStore、SessionRecord、调度（Scheduling/ResourceKey）、
                  scope::context（上下文装配）
alva-agent-abi    产品概念：Task、Scope+Spawn、Phase、Analytics、Provider 注册表
alva-agent-core   SDK 装配（Plugin/Registrar/AgentBuilder）
```

- `alva-agent-abi` 依赖内核契约层，被 SDK 装配层消费；内核**不得**（传递）
  依赖它——CI Rule 19 焊死。
- 第三方拿内核搭一个完全不同形态的 agent 时，可以整层不用 agent-abi。

### 2. 会话契约拆成两个

- **ConversationLog**——内核对"历史"的全部要求：读得到消息、追加得进消息
  （含骨架事件与 typed 记录）、`supports_atomic_records`、`flush`。一个
  二十行的内存实现就能满足。
- **SessionStore**——harness 与 UI 的超集需求：查询、计数、回滚、快照、
  订阅、压缩、持久化、子会话派生。
- `AgentSession` 保留为二者的组合标记。内核循环（run_agent / run_loop /
  ToolBatchCoordinator / ActualLlmCall）只持有 `Arc<dyn ConversationLog>`，
  查询/回滚/快照/订阅/压缩在类型层面进不了内核。

### 3. 合约谎言在类型层面兑现

- ConversationLog 除身份外的每个方法返回 `Result<_, SessionError>`；新增
  `AgentError::Session` 承载传播。
- 刷盘承诺改为与实现一致：run 结束 + 周期性（10 次迭代或 30 秒），测试钉住；
  优雅停机的 flush 归 harness。
- `compact_messages` 无默认实现（"清空重来"曾是压缩默认路径，是 Inspector
  断掉的根因）。
- Listenable 会话广播顺序 == seq 顺序（提交临界区入 FIFO 队列），订阅者经
  mpsc 信箱消费，慢消费者不阻塞追加与订阅。

### 4. 具体实现搬出内核

- 内存会话后端（InMemory / Listenable）→ `alva-agent-core`（SDK 装配层）；
  `run_child_agent` 不再隐式造会话，child session 由调用方提供。
- 注入/压缩的执行（apply_injections / apply_compressions）→ `alva-agent-context`；
  内核的 ContextRuntime 只在正确时机询问上下文系统，落地经 ContextSystem 上
  的 `ContextApply` 执行器完成。
- 跨 agent 工具资源锁表 → `alva-agent-core`（ToolLockRegistry + ToolLockMiddleware，
  总线可选发现）；内核只留调度词汇与波次调度。
- LLM 埋点上报 → 分析中间件的 `wrap_llm_call` 钩子；内核不再认识埋点接收方。

## Alternatives Considered

- **一次性拆分内核 ABI**（先拆后清）：否决。依赖 kernel-abi 的有 29 个 crate，
  先拆等于把死表面搬运过新边界再删，diff 大到无法 review。
- **ConversationLog 泛型化以摆脱 SessionRecord**：否决。typed 记录是工具协调器
  提交意图/结果的内核词汇，泛型化是为一处用点引入全库复杂度。
- **在 kernel-abi 内做"概念分区"（module 注释隔离）**：否决。模块内分区没有
  强制力，半年后就会被当成技术债合并回去——依赖方向的 CI 规则才有强制力。

## Consequences

### Positive

- 第三方实现一个会话后端的门槛从 30 个方法降到 8 个（且都可失败、语义诚实）。
- 内核契约公开表面收缩 37%（快照 diff 可查）；Tool trait 22 → 7 方法。
- 行数（src，不含测试）：kernel-abi 4946 → 3714；kernel-core 7117 → 5373。
  README 的估计值（6001/8624 → 约 2500/3500）以重构前估算，起点偏高；
  剩余差额对应拆分**新增**的内核契约面（双会话契约 + 调度描述符 + ContextApply
  执行器 trait），而非未清除的旧概念。
- "极通用内核"由 CI Rule 19（内核不得依赖 agent-abi）持续验证。

### Negative

- 会话后端实现者现在要拆成两个 impl 块（ConversationLog / SessionStore）。
- kernel-core 对外测试需要 dev-dependency 指向装配层 crate（后端回退路径）。
- agent-abi 与 kernel-abi 对 `SessionRecord` 的层位约定（内核词汇）需要新读者
  读一次 ADR——它不在 agent-abi，容易凭名字猜错。
