# Managed Agents API Parity — alva SDK 调研与 6 处对齐手术

> 调研背景：Anthropic 在 2026-04 发布的 **Managed Agents API**
> (`anthropic-sdk-typescript/src/resources/beta`)
> 是一套服务端托管的 Agent 平台 API（Session / Thread / Resource / Outcome /
> Coordinator）。本文盘点 alva-agent 当前 SDK 层（kernel + agent-core +
> agent-{context,memory,security} + protocol-*）能否作为 Managed Agents API
> 的后端引擎，并给出 6 处对齐改动。

> **结论：能。骨架已经在 SDK 里，6 处改动都是 SDK 内的小手术，不是结构性
> 改造。**

---

## TL;DR

| 层 | 判断 | 一句话 |
|---|---|---|
| 数据模型 / 事件协议 | ✅ 几乎完全对得上 | `AgentSession` 已经是 append-only event log + monotonic seq + history+live-tail subscribe + parent-child session + `ScopedSession` emitter |
| 运行时模型 | 🟡 能跑通但形状不一样 | spawn / HITL / outcomes 都有等价物，但语义离散：HITL 走 side-channel oneshot 不入 event log；coordinator 是 LLM-driven 不是声明式 roster；outcomes 跑在 `agent-graph` 里不在 session 上 |
| 平台基建（Vault/Files/Webhooks/Auth/多租户） | ❌ 完全没有，但本来就不应该在 SDK | App 层另起炉灶 |

真正的结构性差异是 **durable async runtime**（session 跑在 worker 里，client
detach/reattach），不是"我们没有 App"。

---

## 一、Anthropic Managed Agents 模型骨架

```
Session (sesn_xxx)                  ← 服务端可寻址、event-sourced
├── agent (snapshot)                 ← create 时拍快照，跑中不受 agent 改动影响
├── status: 'running'|'idle'|'rescheduling'|'terminated'
├── resources[]: file|github_repo|memory_store
├── outcome_evaluations[]
├── stats / usage (session 级)
└── threads:
    ├── primary thread (sthr_xxx, parent_thread_id=null)
    │   ├── agent / status / stats / usage (thread 级独立计数)
    │   └── events stream
    └── child threads (来自 multiagent.agents[] roster)
        ├── parent_thread_id 指向 primary 或更上层
        └── events stream（cross-post 到 primary 流）
```

事件 taxonomy 四个 namespace（按 producer 分）：

- `user.*` — client 发进来：`user.message`, `user.interrupt`,
  `user.tool_confirmation`, `user.custom_tool_result`, `user.define_outcome`
- `agent.*` — agent 跑出来：`agent.message`, `agent.thinking`,
  `agent.tool_use`, `agent.tool_result`, `agent.thread_message_sent/received`,
  `agent.thread_context_compacted`
- `session.*` — lifecycle：`session.status_*`, `session.thread_created`,
  `session.thread_status_*`, `session.error`, `session.deleted`
- `span.*` — 可观测性：`span.model_request_start/end`,
  `span.outcome_evaluation_start/ongoing/end`

控制原语：

- **`requires_action`** — 任何阻塞型 HITL（tool 批准 / custom tool result /
  outcome 评估）都进 `session.status_idle.stop_reason.event_ids[]`，client
  逐个解决
- **`user.interrupt`** — 带 `session_thread_id` 中断一根，不带就中断整个非
  archived 子树
- **multiagent coordinator** — 在 agent 定义时声明 `multiagent.agents[]` roster
  （1-20 个，可含 `{type:'self'}`，depth limit 1）

---

## 二、逐项映射（Anthropic → alva-agent SDK）

### A. Session（事件 log + 状态机）✅ 已基本对齐

| Anthropic | alva-agent | 位置 / 差距 |
|---|---|---|
| `events.list(session_id)` | `AgentSession::query(EventQuery)` | `crates/alva-kernel-abi/src/agent_session.rs:211` 等价 |
| `events.stream(session_id)` | `AgentSession::subscribe_events(from_seq)` | `agent_session.rs:387, 964` 完全等价（`ListenableInMemorySession` 同样 history+live tail 无重复无丢失） |
| `events.send(...)` | `AgentSession::append_message` / `append` | 等价 |
| Server-assigned `seq` | `seq: u64` + atomic `fetch_add` | `agent_session.rs:668` 设计完全一样 |
| `processed_at` timestamp | `timestamp: i64` epoch millis | 一样 |
| Cross-post child→primary | `ListenableInMemorySession::subscribe` + `ForwardToSession` listener | `agent_session.rs:1540` 测试就是这个 pattern |
| `parent_session_id` | `AgentSession::parent_session_id()` | 已在 trait |
| Skeleton events（`run_start`/`iteration_start`/`component_registry`/`run_end`） | kernel-core 已经 emit | `crates/alva-kernel-core/src/run.rs:625, 690, 747` 比 Anthropic 还细 |

**缺**：单 session 的 trait 实例有了，但**没有 `SessionRegistry`**（list /
filter 跨 session 的 collection API）。

### B. Thread（执行单元）🟡 概念在但没有 first-class Thread

| Anthropic | alva-agent | 位置 / 差距 |
|---|---|---|
| `Thread { id, parent_thread_id, status, stats, usage }` | `SpawnScopeImpl { id, parent_id, depth, role, session_id }` + 子 `Session` + `Blackboard` | `crates/alva-agent-context/src/scope/scope_impl.rs:22`；概念分裂在三处 |
| thread.events list/stream | child session 的 query/subscribe | 等价 |
| cross-post | `ForwardToSession` listener | 等价 |
| per-thread status / stats / usage | **缺** —— `SpawnScopeImpl` 没有 status 字段；token usage 在 session 全局算 | 真缺 |
| `session.thread_created` / `agent.thread_message_sent` | `SubagentRunStart/End` 事件 + `BoardMessage` | 名字不一样但都有 |

### C. Coordinator（多 agent 编排）🟡 LLM-driven 不是声明式

| Anthropic | alva-agent | 位置 / 差距 |
|---|---|---|
| `multiagent.agents[]` 在 build 时声明 roster | LLM 调 `agent` tool 时动态指定 | `crates/alva-app-core/src/extension/agent_spawn.rs:230` 机制根本不同（更灵活但少了 create-time 校验） |
| depth limit 1 | `SpawnScopeImpl::max_depth` 运行时检查 | 机制等价 |
| `{type:'self'}` sentinel | 通过 tool 调用，无 sentinel | 不重要 |
| coordinator 通过 tool 隐式 spawn | `AgentSpawnTool::execute` | 等价 |
| `agent.thread_message_sent/received` | `BoardMessage` + @mention | 概念等价，事件名不同 |

### D. Resources（session 挂载 file/repo/memory_store）🟡 三套机制分裂

| Anthropic | alva-agent | 差距 |
|---|---|---|
| `session.resources[]` 统一抽象 | workspace（路径）/ memory backend / skill registry **三套独立挂载** | 缺统一抽象 |
| file / github_repository / memory_store 三种 variant | LocalToolFs + MemoryBackend + SkillRepository | 各管一摊 |
| `mount_path` / `access` / `instructions`（注入 system prompt） | skill 有 `InjectionPolicy::Auto/Explicit/Strict`；memory / repo 没有 | skill 部分齐 |
| 各资源 CRUD | 各自有，不统一 | 形状不齐 |

### E. Skills ✅ 比 Anthropic 还细

| Anthropic | alva-agent | 位置 / 差距 |
|---|---|---|
| metadata + SKILL.md + 多文件 | `SkillMeta` + `SkillBody` + `SkillResource[]` | `crates/alva-protocol-skill/src/types.rs` 三级完整 |
| 渐进式加载 | 同样三级（meta / body / resource） | 已有 |
| `version` 顶级字段 | 在 `metadata: HashMap` 里 | 升级到顶级即可 |
| `source: "custom"\|"anthropic"` | 缺 | 补 enum 即可 |
| multipart upload | ZIP / 本地目录 / RemoteUrl | App 层补 multipart endpoint |
| 注入 system prompt | `SkillInjector` | 等价 |

### F. MCP Integration ✅ 已对齐

| Anthropic | alva-agent | 差距 |
|---|---|---|
| `mcp_servers[]` 在 agent 定义里声明 | `McpSet { include, exclude, inherit_global }` | 等价 |
| MCP toolset 桥接 | `McpToolAdapter`，命名 `mcp:<server>:<tool>` | `crates/alva-protocol-mcp/src/tool_adapter.rs` 完整实现 |
| transport | Stdio + SSE | 比 Anthropic 全 |

### G. HITL / requires_action 🟡 机制有但语义离散

| Anthropic | alva-agent | 位置 / 差距 |
|---|---|---|
| `session.status_idle` + `stop_reason: requires_action { event_ids: [...] }` | `SecurityGuard::check_tool_call` 返回 `NeedHumanApproval { request_id }`；`SecurityMiddleware` `await` oneshot | 机制等价，**但走 side-channel 不进 session event log** |
| client 发 `user.tool_confirmation` 解决 | `BaseAgent::resolve_permission(request_id, decision)` | 等价 |
| 多 blocked event 聚合在 `event_ids[]` | **缺** —— 每条独立 oneshot，无聚合 view | 真缺 |
| plan mode / custom tool result / approval 统一走 requires_action | `PlanModeMiddleware` / 审批各自路径 | 没收口 |

### H. Outcomes / Evaluation 🟡 跑在外面不在 session 上

| Anthropic | alva-agent | 位置 / 差距 |
|---|---|---|
| `user.define_outcome` event + rubric | `EvaluationExtension` + `GradingCriterion { name, weight, description }` | `crates/alva-app-core/src/extension/evaluation/evaluator.rs:99` rubric 概念有 |
| `session.outcome_evaluations[]` 可查询 | 跑在 `agent-graph` 状态机里，**不挂 session** | 真缺 |
| `max_iterations` 限制 | `max_retries`（默认 3） | 等价 |
| `span.outcome_evaluation_*` 事件 | `agent-graph` GraphEvent | 名字不同，机制等价 |

### I. Models / Environments / Vault / Files / Webhooks / UserProfiles ❌ App 层

| Anthropic | alva-agent | 应该在哪 |
|---|---|---|
| Models registry | `LanguageModel` / `Provider` trait（无 list/metadata API） | SDK 给原语，App 暴露 REST |
| Environments（sandbox 配置） | `SecurityGuard` + `SandboxConfig`（Seatbelt） | SDK 已有原语，App 包成资源 |
| Vault | 完全没有 | App 层 |
| Files API | 无 | App 层 |
| Webhooks | 无（只有 `AnalyticsSink` 埋点） | App 层 |
| UserProfiles | 无 | App 层 |

### J. Durable Async Runtime ❌ 还是同步嵌入式

| Anthropic | alva-agent | 差距 |
|---|---|---|
| Session 服务端持续运行，client detach/reattach | `run_agent` 是 future，drop 即停 | 结构性差异 |
| `session.status = rescheduling` 表达 transient 错误恢复 | retry middleware 隐式 | 状态机不暴露 |
| 进程崩溃恢复 | 无 | App 层可加 |

---

## 三、6 处对齐手术（按优先级）

| # | 改动 | crate | 工作量 | 立即可见效果 |
|---|---|---|---|---|
| 1 | **HITL 审批事件入 session log + `pending_actions()` API** | `alva-agent-security` + `alva-agent-core` | 小 | 订阅 `session.subscribe_events` 能看到 `requires_action` 事件 |
| 2 | `AgentSession` 加 `SessionRegistry` trait（list/filter/archive），单 session 升级成可寻址集合 | `alva-kernel-abi` | 小 | App 层可写"按 status 过滤"查询 |
| 3 | `SpawnScopeImpl` + child Session + Blackboard 三件合一，提到一等 `Thread { id, parent_thread_id, status, stats, usage }` | `alva-agent-context` + `alva-kernel-abi` | 中 | per-thread token 用量、状态可观测 |
| 4 | 给 multiagent 加"可选声明式 roster"：`AgentBuilder::multiagent_roster([profile])`；保留 ad-hoc spawn 作为 escape hatch | `alva-agent-core` | 小 | create-time 校验 + 引用计数 |
| 5 | `Resource` 抽象（file/repo/memory_store/skill），统一 mount API 和 CRUD | **`alva-app-core`**（harness 层）+ App 层接入 | 中 | 我们自己的 App 可统一管资源；SDK 内核保持干净 |
| 6 | evaluation outcome 挂到 session：outcome 是 session 一等字段，可 query | `alva-app-core` + 新建 `OutcomeRegistry` | 小 | `session.outcomes()` 直接可查 |

**§三\*：#5 的位置纠错**

第一版我把 `Resource` 放进了 `alva-kernel-abi`，是错的。Agent loop 不读
不挂载 resource，挂载是 tool / extension 的事。Resource 抽象**不属于
SDK 内核**。

2026-05-12 修正：移到 `alva-app-core`（harness 层）。这样：
- SDK（kernel-abi / agent-core）保持"agent loop 真用的"那点契约，不被
  污染
- harness 层（我们自己的 deer-flow / cli / tauri）可以有 opinion，把
  Resource 当一等公民暴露给 App
- 第三方搭自己 harness 的话，可以选择用 `alva-app-core::resource` 或者
  完全跳过——SDK 不强加

**注**：以上手术全是改善对齐，不是结构性改造。骨架（event log /
subscribe stream / parent-child session / ScopedSession / tool registry /
permission gate / skill loader / mcp adapter）已经全部在 SDK 里。

---

## 四、那"自部署对外暴露 API"具体差什么？

把 SDK 当后端、对外暴露 REST/SSE，需要在 SDK 之外加：

### App 层必须做的（SDK 帮不了）
- HTTP/SSE/WebSocket server：把 `AgentSession::subscribe_events` 包成 SSE
  endpoint，把 `append_message` 包成 POST
- 多租户 + 认证：org_id / workspace_id / user_id 路由
- Session 持久化 store：现有 SQLite/JSON 实现 + "按 org 查询的索引"
- Managed sandbox：现在是同进程 Seatbelt（macOS only），要做远端管理需要
  Firecracker / containerd
- Vault / Files / Webhooks：从零起步的应用层服务
- billing / quota / rate limit

### SDK 必须做的
就是第三节的 6 处。

### 真正的结构性难点（不归 SDK，也不归 App）
**Durable async runtime** —— session 跑在 worker actor 里，client
detach/reattach。如果只做"嵌入式 SDK + 同步调用"形态的 self-hosted API
可以不做；如果要做"真正像 Anthropic 那样后台跑、client 来去自如"，这是
必须的基础设施。

---

## 五、首手术：HITL → session event log

**目标**：让审批请求/解决都以 `SessionEvent` 形式进入 session 日志，订阅
`subscribe_events` 的客户端可以直接看到 `requires_action` 事件出现，
并能用 `query` 找到所有未解决的 pending action。

**改动点**：

1. `crates/alva-agent-security/src/middleware/security.rs`
   - 在 `NeedHumanApproval` 分支发出 `notifier` **之前**先 append 一个
     `requires_action` 事件到 `state.session`
   - 等待结束后（Decision/Cancelled/TimedOut）append 一个
     `requires_action_resolved` 事件，`parent_uuid` 指向 `requires_action`
     的 uuid

2. `crates/alva-agent-security/src/lib.rs`
   - 加一个独立的 `pending_actions(session: &dyn AgentSession)` helper：
     扫 `requires_action` 事件，过滤掉已被 `requires_action_resolved`
     引用的，返回 `Vec<PendingAction>`

3. 测试：
   - tool 触发 NeedHumanApproval → 订阅看见 `requires_action` 事件
   - resolve 后看见 `requires_action_resolved` 事件
   - `pending_actions` 返回未解决项

**不改动**：side-channel oneshot 保留，只是**额外**写 event log。向后兼容。

---

## 历史

- 2026-05-11 初稿，调研完成
- 2026-05-11 手术 #1 完成：HITL 审批入 session event log
  - 新建 `crates/alva-agent-security/src/pending_actions.rs`（`PendingAction` /
    `ResolveStatus` / `pending_actions()` helper + 事件类型常量）
  - 改 `crates/alva-agent-security/src/middleware/security.rs`：
    `NeedHumanApproval` 分支在 notifier 发送前 append `requires_action`
    事件；5 个终结分支（allow / reject / disconnected / cancelled /
    timed_out / no_handler）各 append `requires_action_resolved`
  - 11 个新测试覆盖事件日志写入、`pending_actions()` 查询、live-tail
    subscriber，全部绿色；side-channel oneshot 完全保留向后兼容
- 2026-05-14 **大重构**：把所有 API 反推到内核的抽象搬回 App 层
  - **触发**：复盘时确认 #2 / #3 走错了路——SessionRegistry / ThreadView /
    ThreadStats 都是用 Anthropic REST API 形状反推出来的 schema，被错塞
    进 `alva-kernel-abi`。kernel 应只露 agent loop 真用的契约，其他抽象
    是 App 自己堆的。
  - **Phase 1**（session_registry + thread → 合并搬到 app-core）：
    - 删 `crates/alva-kernel-abi/src/session_registry.rs`（1086 行）
    - 删 `crates/alva-kernel-abi/src/thread.rs`（377 行）
    - 内容合并到新建 `crates/alva-app-core/src/session_registry.rs`：
      - `SessionMetadata` 吸收原 `ThreadView` 的两个派生字段
        （`session_group_id: Option<String>` + `depth: Option<u32>`），
        作为 `Option<>` 字段；raw metadata 时为 None，enrichment helper
        填充
      - 删掉 `ThreadView` 单独类型——一个 struct 走天下，需要 Anthropic
        形状的 JSON 时由 App 在序列化层做
      - `thread_view` / `thread_tree` / `primary_thread_for` 改为返回
        enriched `SessionMetadata`（不再是单独类型）
      - `climb_to_root` / `enrich_with_tree_fields` 私有 helper 内联
    - 22 个测试全部移过去（原 15 个 session_registry + 10 个 thread
      合并，去重后 22 个）
  - **Phase 2**（roster: agent-core → app-core 并去掉 builder 接线）：
    - 移 `crates/alva-agent-core/src/roster.rs` → `crates/alva-app-core
      /src/roster.rs`
    - 删 `AgentBuilder::multiagent_roster(_strict)` 两个方法
    - 删 `AgentBuilder` 的 `multiagent_roster` / `multiagent_roster_
      strict` 字段
    - 删 `build()` 第 2.5 步（roster validate + publish）
    - 删 agent-core 集成 test 里 6 个 roster 测试（保留注释说明搬迁
      路径）
    - **后续 wiring**：哪个 harness extension 想用 roster，自己
      `bus_writer.provide(Arc::new(MultiagentRosterCap{...}))`；推荐位置
      是未来的 `SubAgentExtension::with_roster(...)`
  - **Phase 3**（确认 #5 / #6 没被 wire 到 apps）：
    - grep `alva-app-cli` + `alva-app-tauri` 没有 `ResourceRegistry` /
      `OutcomeRegistry` 引用，验证 schema 还在「等真接 REST 再说」状态
  - **测试**：kernel-abi 146（-25）/ agent-core lib 0 集成 2（-6 个 roster）
    / app-core 149（+37：22 session_registry + 15 roster）；workspace 干净
  - **kernel-abi 是否还含「API 反推」的抽象**：盘了一遍，只剩 `AgentSession`
    trait + `parent_session_id` 这两条 agent loop 真用的契约——干净了
- 2026-05-12 手术 #6 完成（`alva-app-core`，harness 层）：outcome 入 session
  - 新建 `crates/alva-app-core/src/outcome.rs`（14 个单元测试）：
    - `OutcomeStatus` 8 个 variant（Pending / Running / Evaluating /
      NeedsRevision / Satisfied / MaxIterationsReached / Failed /
      Interrupted）+ `is_terminal()` + `as_str()` 字符串对齐 Anthropic
    - `Rubric` 3 variant：`Text { content }`（inline 文本）/ `File {
      file_id }`（Anthropic parity）/ `Criteria { criteria:
      Vec<GradingCriterion> }`（alva 结构化加权评分）
    - `OutcomeScore { weighted_score, passed, per_criterion }` —— 评分
      breakdown
    - `Outcome { id (outc_<hex>), session_id, description, rubric,
      status, current_iteration, max_iterations, latest_score,
      explanation, created_at, updated_at, completed_at }`
    - `OutcomeParams` / `OutcomePatch`（带 `clear_*` builder methods）/
      `OutcomeFilter`（statuses 列表 + terminal_only flag）
    - `OutcomeRegistry` trait：`define` / `retrieve` / `update` /
      `record_iteration` / `list_session` / `delete`
    - **`record_iteration` 原子状态机**：pass → `Satisfied`；fail +
      iteration < cap → `NeedsRevision`；fail + iteration ≥ cap →
      `MaxIterationsReached`；首次进 terminal 自动 stamp `completed_at`
    - `InMemoryOutcomeRegistry` 参考实现
    - `render_outcomes_for_session` 把非 terminal outcome 拼成
      `## Active Outcomes` system prompt 块（Text/File/Criteria 三种
      rubric 都有渲染分支）
  - 顺手补：`GradingCriterion` 加 `PartialEq` 以让 `Rubric` 能 derive
    `PartialEq`（Rust trait 平移补丁，零行为变化）
  - 14 个测试覆盖状态机所有路径 / `completed_at` 单向性 / 列表过滤 /
    Rubric 3 variant serde / 渲染剔除 terminal outcome
  - 112 / 112 测试通过；workspace clean
- 2026-05-12 手术 #5 完成（在 `alva-app-core`，不在 SDK）：统一 Resource 抽象
  - **重要的架构纠错**：第一次实现把 `SessionResource` / `ResourceRegistry`
    放到了 `alva-kernel-abi`，违反"Kernel + agent-core 是稳定 SDK 不接受
    功能扩展"原则。Agent loop 不挂载 resource——挂载是 tool/extension
    的事。**移到 `alva-app-core`**（harness 层）：我们自己的 App 可以统一
    管，SDK 保持干净，第三方 harness 自由选择是否接入。
  - 新建 `crates/alva-app-core/src/resource.rs`（14 个单元测试）：
    - `SessionResource { id, session_id, kind, mount_path, access,
      instructions, description, created_at, updated_at }`
    - `ResourceKind` 4 variant：`File { file_id }` / `GitHubRepository {
      url, checkout, authorization_token }` / `MemoryStore {
      memory_store_id }` / `Skill { skill_id, version }`（前 3 镜像
      Anthropic，第 4 是 alva 加的）
    - `RepoCheckout` enum（Branch / Commit）
    - `ResourceAccess` enum（ReadWrite / ReadOnly，默认 ReadWrite）
    - `ResourceParams` 添加用 + `ResourcePatch` 更新用（沿用 `Option<
      Option<T>>` 区分清空/不动/设置）
    - `ResourceFilter` 按 `kind_tag` 过滤
    - `ResourceRegistry` trait：`add` / `retrieve` / `update` /
      `list_session` / `delete`
    - `InMemoryResourceRegistry` 参考实现：HashMap + RwLock + atomic id
      counter（`rsrc_<hex>` 风格）
    - `render_resource_instructions(registry, session_id)` 辅助：拼出
      "## Session Resources" system-prompt 块，让 LLM 知道 mount 了啥
  - 14 个测试覆盖 CRUD / 过滤 / 清空字段 / Anthropic 字符串 tag 对齐 /
    每种 kind 的 serde round-trip / system-prompt 渲染
  - workspace 干净
- 2026-05-11 手术 #4 完成：声明式 multiagent roster
  - **设计决策**：opt-in 声明，不破坏 ad-hoc spawn。`AgentBuilder` 上两个
    新方法 `multiagent_roster(r)` / `multiagent_roster_strict(r)`：前者把
    roster 当 advisory 元数据公开在 bus 上（App 层自由决定是否使用），后
    者把 cap 标记 `strict = true`（消费者如 `AgentSpawnTool` 应该拒绝
    roster 之外的 spawn 目标）。不声明 roster = 现有行为完全不变。
  - 新建 `crates/alva-agent-core/src/roster.rs`（含 16 个单元测试）：
    - `RosterEntry` + `RosterEntryKind { Agent { id, version }, SelfRef }`
      —— 镜像 Anthropic `MultiagentRosterEntryParams` 三种 variant（字符串
      id / 版本化引用 / `{type:'self'}` sentinel）
    - `MultiagentRoster::validate()` —— 1-20 entries / 重复 id 检测
      （即使版本不同也算重复）/ 至多一个 SelfRef
    - `RosterError` enum：`BadSize` / `DuplicateId` / `MultipleSelf`
    - `MultiagentRosterCap` `#[bus_cap]` —— 把 roster + strict flag 放
      bus 上供消费者读
    - 实用方法：`contains_agent` / `allows_self` / `agent_ids()`
      iterator / `with_description` builder
  - `crates/alva-agent-core/src/agent_builder.rs`：
    - 加两个字段（`multiagent_roster: Option<MultiagentRoster>` +
      `multiagent_roster_strict: bool`）和两个 builder 方法
    - `build()` 第 2.5 步：validate（fail-fast，无效 roster → `AgentError
      ::Other` 包 `RosterError`）+ publish 到 `bus_writer` 作 `Arc<Multi
      agentRosterCap>`。在 extensions activate / configure **之前**，所以
      extension 在 lifecycle 里就能读到
  - `crates/alva-agent-core/Cargo.toml`：补 `serde` (derive) + `thiserror`
    （之前缺，其他 SDK crate 都已经有）
  - 6 个新集成测试（含 `bus.has::<Cap>()` / `bus.get::<Cap>()` 验证发布
    点 / strict flag / 三种 invalid roster fail-build 路径）+ 16 个单元
    测试（含 serde round-trip）
  - 28 + 8 = 36 测试全过；workspace clean
- 2026-05-11 手术 #3 完成：Thread 一等化（projection 路径）
  - **设计决策**：alva 模型里 `AgentSession` 1:1 等价 Anthropic Thread
    （每个执行单元一个事件 log），所以 thread_id ≡ session_id。Thread "一等化"
    做法：扩 SessionMetadata 加 thread-level 字段 + 给 SessionRegistry
    加 tree-walking / atomic-accumulate 方法 + 新文件 `thread.rs` 提供
    Anthropic-shape 的 `ThreadView` 投影。**不引入** Session/Thread 并行类型
    （会跟 alva 已有架构打架）。
  - `crates/alva-kernel-abi/src/session_registry.rs`：
    - 加 `ThreadStats { startup_ms / active_ms / duration_ms }` +
      `ThreadUsage { input_tokens / output_tokens / cache_creation_* /
      cache_read_* }`
    - `SessionMetadata` 加 `stats: ThreadStats` + `usage: ThreadUsage`
      （`#[serde(default)]` 保持向后兼容）
    - `SessionMetadataPatch` 加 `stats` / `usage` 字段 + builder 方法
    - `SessionRegistry` trait 加 4 个新方法：`children(parent)` /
      `descendants(root)` BFS / `record_usage(id, &UsageMetadata)` /
      `record_active_ms(id, delta)`。前两个有 default impl；后两个 default
      非原子（read-then-write），InMemory impl **覆盖为原子**（hold write
      lock 跨 read+add+write）
  - 新建 `crates/alva-kernel-abi/src/thread.rs`：
    - `ThreadView` 投影（thread_id / parent_thread_id / session_group_id /
      depth / agent_id / status / stats / usage / created_at /
      updated_at / archived_at）
    - 异步辅助：`thread_view` / `thread_tree` / `primary_thread_for`，
      内部通过 `climb_to_root` 爬 parent 链解 group_id 和 depth
    - 64 跳上限防御 pathological 循环；孤儿父链优雅退化（reachable root
      作为 group_id）
  - 10 个新测试覆盖 view 投影、深度/group_id 计算、孤儿 fallback、BFS
    tree、tree-walking helper、atomic usage/stats 累加、patch 覆盖
  - 171 / 171 测试通过；workspace 干净
- 2026-05-11 手术 #2 完成：`SessionRegistry` trait + in-memory 参考实现
  - 新建 `crates/alva-kernel-abi/src/session_registry.rs`
  - `SessionRegistry` trait：`insert` / `get` / `metadata` / `update` /
    `archive` / `delete` / `list` / `count`
  - `SessionMetadata`（session_id / parent_session_id / status / agent_id /
    title / metadata / created_at / updated_at / archived_at）
  - `SessionStatus` enum：`Running | Idle | Rescheduling | Terminated`
    （对齐 Anthropic）
  - `SessionFilter`（statuses / agent_id / parent_session_id /
    created_after / created_before / include_archived / order / limit /
    after cursor）+ `SessionPage` opaque cursor 分页
  - `SessionMetadataPatch` builder 风格 + 区分 `None` (不动) / `Some(None)`
    (清空) / `Some(Some(_))` (设置)
  - `InMemorySessionRegistry` 参考实现：HashMap + RwLock，排序按
    created_at + session_id 复合键（避免 ms 时间戳碰撞导致顺序不稳定）
  - 15 个新测试覆盖 CRUD / 过滤 / 排序 / 分页 / archive 隐藏，全绿
