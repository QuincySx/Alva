# Context Engineering Spec

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建一套分层上下文管理系统，由专门的上下文管理 Agent 驱动，根据不同角色类型动态调整策略，解决 Context Rot 问题。

**Architecture:** 五层上下文模型 + 上下文管理 Agent（工具驱动，小模型）+ 四角色 Agent 类型（指导者/执行者/调研者/校验者），每种角色的上下文策略由管理 Agent 动态观察并调整，而非硬编码。

**Tech Stack:** Rust, alva-agent-core (Middleware + ContextTransform), alva-agent-memory (FTS + Vector), alva-types (Message/ContentBlock/AgentMessage)

---

## 1. 问题定义

Transformer 注意力复杂度 O(n²)，上下文越长关键信号越容易被噪声稀释（Context Rot）。在多 Agent 编排场景下问题更严重：

- 指导者的上下文被子 Agent 的执行细节淹没
- 执行者的上下文被无关的其他 Agent 对话污染
- MCP 工具（如图片解析）返回大量数据，几次调用就吃满窗口
- 跨任务记忆缺失导致重复劳动

核心矛盾：Agent 需要足够信息做决策，但不能被细节淹没。

## 2. 五层上下文模型

```
Context Window
│
│  ╔══════════════════════════════════════════════════════╗
│  ║ L0: Always Present (常驻层)                         ║  ← 短、硬、可执行
│  ║ Identity · conventions · hard constraints            ║     Prompt Cache 友好
│  ╚══════════════════════════════════════════════════════╝
│
│  ┌──────────────────────────────────────────────────────┐
│  │ L1: On-Demand (按需加载层)                           │  ← 描述符常驻
│  │ Skills · domain knowledge · runbooks                 │     完整内容触发时注入
│  └──────────────────────────────────────────────────────┘
│
│  ╔══════════════════════════════════════════════════════╗
│  ║ L2: Runtime Inject (运行时注入层)                    ║  ← 每轮按需拼入
│  ║ Timestamp · channel ID · user prefs · task state     ║     动态信息放最后
│  ╚══════════════════════════════════════════════════════╝
│
│  ╔══════════════════════════════════════════════════════╗
│  ║ L3: Persisted Memory (记忆层)                       ║  ← 跨会话经验
│  ║ MEMORY store — not in system prompt, read on demand  ║     不直接进 L0
│  ╚══════════════════════════════════════════════════════╝
│
│  ┌ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─┐
│  │ L4: Never Enters (系统层)                           │  ← 完全不进上下文
│  │ Hooks · code rules · deterministic logic             │     外部系统处理
│  └ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─┘
│
▼
```

### 层间排序与 Prompt Caching

```
[L0 常驻 — 稳定前缀，命中缓存] → [L1 Skill 描述符] → [L1 已加载 Skill 内容] → [L2 运行时] → [对话消息]
```

L0 + L1 描述符组成稳定前缀，变动最少，缓存命中率最高。L2 和对话消息放在最后，变动不破坏前缀。

### 每层的 CRUD 语义

| 层 | 写入时机 | 读取频率 | 修改频率 | 删除条件 |
|---|---|---|---|---|
| L0 | Agent 创建时 | 每轮 | 几乎不变 | Agent 删除 |
| L1 | Skill 注册时写描述符，触发时加载全文 | 描述符每轮；全文按需 | Skill 更新时 | Skill 卸载 |
| L2 | 每轮开始 | 当轮 | 每轮重建 | 自动（轮结束即废弃） |
| L3 | 对话结束后异步提取 | 被上下文管理 Agent 按需查询 | 发现过时/矛盾时更新 | 置信度衰减或用户显式删除 |
| L4 | 开发时配置 | 从不（不进上下文） | 代码修改 | N/A |

## 3. 三层架构：Store → Hooks → Plugin

```
┌─────────────────────────────────────────────────────────────────┐
│  Layer 3: Context Plugin (策略层)                               │
│                                                                 │
│  实现 ContextPlugin trait，插入到 hooks 中                       │
│  ┌─────────────────────┐  ┌─────────────────────┐              │
│  │ AgentContextPlugin  │  │ RulesContextPlugin  │  ← 可替换    │
│  │ (内置，Agent 驱动)   │  │ (纯规则，调试用)     │              │
│  │ 小模型做智能决策     │  │ 只用确定性规则       │              │
│  └─────────────────────┘  └─────────────────────┘              │
├─────────────────────────────────────────────────────────────────┤
│  Layer 2: Context Hooks (钩子层)                                │
│                                                                 │
│  alva-agent-core 的一部分，稳定 API                              │
│  6 个拦截点，每次交互强制经过                                     │
│  每个 hook 分两阶段：确定性处理 → 委托 Plugin 决策               │
├─────────────────────────────────────────────────────────────────┤
│  Layer 1: ContextStore + SDK (数据层)                           │
│                                                                 │
│  五层上下文模型的存储、CRUD、快照生成                              │
│  ContextManagementSDK trait — Plugin 操作上下文的唯一接口         │
└─────────────────────────────────────────────────────────────────┘
```

### 3.1 Layer 1: ContextStore + SDK (数据层)

ContextStore 是每个 Agent 实例的上下文容器，SDK 是操作它的接口。

**这层是纯代码，零 LLM 调用，100% 确定性。**

（SDK 方法详见 3.4 节）

### 3.2 Layer 2: Context Hooks (钩子层)

钩子层是 alva-agent-core 的一部分，**稳定 API，不会变**。每个 hook 分两阶段执行：

```
Hook 触发
  │
  ├─ Phase A: 确定性处理（代码写死，必定执行）
  │   ├ Token 计数
  │   ├ 层级排序
  │   ├ 硬截断兜底
  │   └ 元数据更新
  │
  └─ Phase B: 委托 Plugin 决策（Plugin 可选实现）
      ├ Agent 驱动的 Plugin → 调小模型判断
      ├ 纯规则 Plugin → 跑预设规则
      └ 无 Plugin → 跳过，只走 Phase A
```

即使没装 Plugin，Phase A 保证系统正常运行。Plugin 是增强，不是依赖。

### 3.3 Layer 3: ContextPlugin trait (策略层)

**核心原则：上下文里的每一样东西进来时都必须过 hook，没有例外。**

Plugin 按五层 + 对话 + 评估 + 生命周期组织，不是只管消息：

```rust
/// 上下文管理插件 trait — 实现此 trait 即可接入 hooks 系统
///
/// 方法分五类：
///   生命周期：bootstrap / maintain / dispose
///   五层管控：on_inject_* — 每层内容进入上下文前的拦截
///   每轮处理：on_user_message / assemble / ingest / after_turn
///   事件响应：on_tool_result_persist / on_budget_exceeded / on_sub_agent_*
///
/// 所有方法有默认空实现，Plugin 只需 override 关心的方法。
#[async_trait]
pub trait ContextPlugin: Send + Sync {

    // ─── 生命周期 ───

    /// 会话初始化：从历史数据导入上下文、加载记忆
    /// 只在会话首次激活时调用一次
    async fn bootstrap(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) -> Result<(), ContextError> { Ok(()) }

    /// 每轮开始前的维护：可重写历史条目、清理过期数据
    async fn maintain(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) -> Result<(), ContextError> { Ok(()) }

    /// 插件销毁时清理资源
    async fn dispose(&self) -> Result<(), ContextError> { Ok(()) }

    // ─── 五层注入管控（每层内容进上下文前必过）───

    /// L0 常驻层：system prompt 片段注入前
    /// 可修改、追加、删除 system prompt 的各个 section
    async fn on_inject_system_prompt(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        sections: Vec<PromptSection>,
    ) -> Vec<PromptSection> { sections }

    /// L1 按需加载层：Skill 内容加载进上下文前
    /// 可修改 Skill 内容、拒绝加载、替换为摘要
    async fn on_inject_skill(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        skill_name: &str,
        skill_content: String,
    ) -> InjectDecision<String> { InjectDecision::Allow(skill_content) }

    /// L2 运行时注入层：文件/附件注入前
    /// 可修改内容、拒绝注入、替换为摘要/引用
    async fn on_inject_file(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        file_path: &str,
        content: String,
        content_tokens: usize,
    ) -> InjectDecision<String> { InjectDecision::Allow(content) }

    /// L2 运行时注入层：运行时元数据注入前（时间戳、渠道、用户偏好等）
    async fn on_inject_runtime(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        runtime_data: RuntimeContext,
    ) -> RuntimeContext { runtime_data }

    /// L2 多模态内容（图片/音频/视频）进上下文前
    /// 不管来源是用户消息、工具返回、还是文件附件，统一走这里
    async fn on_inject_media(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        media_type: &str,        // "image/png", "audio/mp3", ...
        source: MediaSource,
        size_bytes: usize,
        estimated_tokens: usize,
    ) -> InjectDecision<MediaAction> { InjectDecision::Allow(MediaAction::Keep) }

    /// L2 RAG/检索结果进上下文前
    /// 向量搜索或知识库检索的结果，不是文件也不是记忆
    async fn on_inject_retrieval(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        query: &str,
        chunks: Vec<RetrievalChunk>,
        total_tokens: usize,
    ) -> Vec<RetrievalChunk> { chunks }

    /// L3 记忆层：记忆注入到上下文前
    /// 可过滤、重排序、修改、限制数量
    async fn on_inject_memory(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        facts: Vec<MemoryFact>,
    ) -> Vec<MemoryFact> { facts }

    /// L3 记忆层：从对话中提取记忆前
    /// 可修改提取的候选、拒绝存储、调整置信度
    async fn on_extract_memory(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        candidates: Vec<MemoryFact>,
    ) -> Vec<MemoryFact> { candidates }

    // ─── 每轮处理 ───

    /// 用户发消息时：决定注入什么（记忆？Skill？运行时信息？）
    async fn on_user_message(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        message: &AgentMessage,
    ) -> Vec<Injection> { vec![] }

    /// 组装发给 LLM 的上下文（核心方法）
    /// 在确定性的层级排序之后调用，Plugin 可做最终裁剪
    /// 传入 token_budget 和 model 信息，返回裁剪后的消息列表
    async fn assemble(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        messages: Vec<AgentMessage>,
        token_budget: usize,
    ) -> Vec<AgentMessage> { messages }

    /// 消息入库前的处理（拦截/修改/标记）
    async fn ingest(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        message: &mut AgentMessage,
    ) -> IngestAction { IngestAction::Keep }

    /// 轮次结束后：异步处理（提取记忆、更新模式、预判下轮压缩）
    async fn after_turn(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) {}

    // ─── 观测与评估 ───

    /// Agent 开始执行（整个生命周期起点）
    async fn on_agent_start(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
    ) {}

    /// Agent 执行结束（整个生命周期终点，可审计完整过程）
    async fn on_agent_end(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        error: Option<&str>,
    ) {}

    /// LLM 返回后：观测模型原始输出，可用于质量评估
    async fn on_llm_output(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        response: &AgentMessage,
    ) {}

    /// 工具执行前：评估是否合理，可阻止执行
    /// 返回 ToolCallAction::Allow 放行，Block 阻止并附带原因
    async fn before_tool_call(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> ToolCallAction { ToolCallAction::Allow }

    /// 工具执行后、入库前：观测原始结果 + 决定怎么持久化
    async fn after_tool_call(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        tool_name: &str,
        result: &AgentMessage,
        result_tokens: usize,
    ) -> ToolResultAction { ToolResultAction::Keep }

    // ─── 上下文管理 ───

    /// 工具结果持久化前：决定裁剪/替换/外化（向后兼容，默认委托给 after_tool_call）
    /// 独立于普通消息，因为工具结果的处理逻辑不同
    async fn on_tool_result_persist(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        tool_name: &str,
        result: &AgentMessage,
        result_tokens: usize,
    ) -> ToolResultAction { ToolResultAction::Keep }

    /// 上下文超预算时的压缩决策
    async fn on_budget_exceeded(
        &self,
        sdk: &dyn ContextManagementSDK,
        agent_id: &str,
        snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> { vec![CompressAction::SlidingWindow { keep_recent: 20 }] }

    /// 创建子 Agent 时：决定传哪些父上下文
    async fn on_sub_agent_spawn(
        &self,
        sdk: &dyn ContextManagementSDK,
        parent_id: &str,
        child_config: &AgentRoleConfig,
        task_description: &str,
    ) -> Vec<ContextEntry>;

    /// 子 Agent 完成时：决定结果怎么回流
    async fn on_sub_agent_complete(
        &self,
        sdk: &dyn ContextManagementSDK,
        parent_id: &str,
        child_result: &str,
        result_tokens: usize,
    ) -> InjectionPlan;
}

/// 注入决策 — 通用的"允许/修改/拒绝"三态
pub enum InjectDecision<T> {
    Allow(T),                      // 原样注入
    Modify(T),                     // 修改后注入
    Reject { reason: String },     // 拒绝注入，附原因
    Summarize { summary: String }, // 替换为摘要后注入
}

/// System prompt 的一个段落
pub struct PromptSection {
    pub id: String,                // 唯一标识，如 "identity", "conventions", "constraints"
    pub content: String,
    pub priority: Priority,        // 压缩时的保留优先级
}

/// 运行时上下文数据
pub struct RuntimeContext {
    pub timestamp: String,
    pub session_metadata: HashMap<String, String>,
    pub user_preferences: HashMap<String, String>,
    pub channel_info: Option<String>,
    pub custom: HashMap<String, serde_json::Value>,
}

/// 消息入库动作
pub enum IngestAction {
    Keep,                          // 原样保留
    Modify(AgentMessage),          // 修改后保留
    Skip,                          // 不入库（不进上下文）
    TagAndKeep { priority: Priority }, // 保留并标记优先级
}

/// 注入动作
pub enum Injection {
    Memory(Vec<MemoryFact>),
    Skill { name: String, content: String },
    Message(AgentMessage),
    RuntimeContext(String),         // L2 运行时信息
}

/// 工具调用评估动作
pub enum ToolCallAction {
    Allow,                                  // 放行
    Block { reason: String },               // 阻止，返回原因给 Agent
    AllowWithWarning { warning: String },   // 放行但注入警告
}

/// 多模态内容来源
pub enum MediaSource {
    UserMessage { message_id: String },
    ToolResult { tool_name: String, message_id: String },
    FileAttachment { file_path: String },
}

/// 多模态内容处理方式
pub enum MediaAction {
    Keep,                              // 原样保留
    Describe { description: String },  // 替换为文字描述
    Externalize { path: String },      // 外化到文件留引用
    Remove,                            // 直接移除
}

/// RAG 检索结果片段
pub struct RetrievalChunk {
    pub source: String,      // 来源文档/路径
    pub text: String,
    pub score: f64,
    pub metadata: HashMap<String, String>,
}

/// 压缩动作 — Plugin 告诉 hooks 层该做什么
pub enum CompressAction {
    SlidingWindow { keep_recent: usize },
    Summarize { range: MessageRange, hints: Vec<String> },
    ReplaceToolResult { message_id: String, summary: String },
    Externalize { range: MessageRange, path: String },
    RemoveByPriority { priority: Priority },
}

/// 工具大结果的处理方式
pub enum ToolResultAction {
    Keep,                          // 全量保留
    Replace { summary: String },   // 替换为摘要
    Externalize { path: String },  // 外化到文件
    Truncate { max_lines: usize }, // 截断
}

/// 子 Agent 结果回流计划
pub enum InjectionPlan {
    FullResult,                    // 全量注入
    Summary { text: String },      // 只注入摘要
    Externalized { path: String, summary: String },  // 外化+一句话
    Error { message: String },     // 错误摘要
}
```

### 3.4 内置的两个 Plugin 实现

**AgentContextPlugin（默认，生产用）：**
- 用小模型（Haiku/GPT-4o-mini）做智能决策
- 有记忆进化能力
- 成本：每次触发约 500-2000 tokens

**RulesContextPlugin（调试/降级用）：**
- 纯规则，零 LLM 调用
- `on_budget_exceeded` → 直接 sliding_window(keep: 20)
- `on_large_tool_result` → 超过 5000 tokens 直接截断
- `on_sub_agent_complete` → 超过 1000 tokens 直接截断
- 成本：零

开发阶段先用 RulesContextPlugin 把管线跑通，再换 AgentContextPlugin 做智能化。

### 3.5 Hook 生命周期全图（21 个 hooks 的触发时序）

```
═══════════════════════════════════════════════════════════════════════
 PHASE 1: 会话初始化（只执行一次）
═══════════════════════════════════════════════════════════════════════

  ❶ bootstrap
  │  "我是谁？之前聊过什么？"
  │  · 导入历史对话到 ContextStore
  │  · 加载持久化记忆
  │  · 初始化五层结构
  │
  ▼

═══════════════════════════════════════════════════════════════════════
 PHASE 2: 每轮循环开始（用户每发一次消息走一遍）
═══════════════════════════════════════════════════════════════════════

  ❷ on_agent_start                              [可选，纯观测]
  │  "新的一轮开始了"
  │
  ❸ maintain
  │  "开始干活前先整理一下"
  │  · 重写/清理历史条目
  │  · 清理过期数据
  │
  ▼
───────────────────────────────────────────────────────────────────────
 PHASE 3: 用户消息进入
───────────────────────────────────────────────────────────────────────

  ❹ on_user_message                             [入口总闸]
  │  "用户说了什么？需要给他补充什么？"
  │  │
  │  ├─❺ on_inject_memory                       [L3 记忆注入]
  │  │    "有相关记忆要注入吗？"
  │  │    · 查询记忆库 → 过滤/重排序 → 注入
  │  │
  │  ├─❻ on_inject_skill                        [L1 Skill 加载]
  │  │    "需要加载某个 Skill 吗？"
  │  │    · 匹配描述符 → 允许/拒绝/摘要
  │  │
  │  ├─❼ on_inject_file                         [L2 文件注入]
  │  │    "用户带了附件？"
  │  │    · 允许/拒绝/摘要/截断
  │  │
  │  ├─❽ on_inject_media                        [L2 多模态]
  │  │    "消息里有图片/音频？"
  │  │    · 保留/描述(调工具)/外化/移除
  │  │
  │  └─❾ on_inject_runtime                      [L2 运行时]
  │       "拼入时间戳、渠道、偏好等"
  │
  ▼
───────────────────────────────────────────────────────────────────────
 PHASE 4: 组装上下文 → 发 LLM
───────────────────────────────────────────────────────────────────────

  ❿ on_inject_system_prompt                     [L0 系统 prompt]
  │  "system prompt 要改吗？"
  │  · 修改/追加/删除 sections
  │
  ⓫ assemble                                   [最终组装]
  │  "在 token 预算内组装完整上下文"
  │  · 按层排序：L0 → L1 → L2 → L3 → 对话
  │  · 检查总 token
  │  │
  │  └─ 超预算？
  │     └─⓬ on_budget_exceeded                  [压缩触发]
  │         "超了，用什么策略压？"
  │         · sliding_window / summarize / externalize / remove
  │
  ▼
  ╔═══════════════════════════════════╗
  ║  [Middleware: before_llm_call]    ║  ← 执行层（选模型、改参数）
  ╚═══════════════════════════════════╝
  │
  ▼
  ┌─────────┐
  │   LLM   │
  └────┬────┘
       │
  ╔═══════════════════════════════════╗
  ║  [Middleware: after_llm_call]     ║  ← 执行层（观测 token/延迟）
  ╚═══════════════════════════════════╝
  │
  ⓭ on_llm_output                              [观测模型输出]
  │  "模型返回了什么？质量如何？"
  │
  ▼
───────────────────────────────────────────────────────────────────────
 PHASE 5: Agent 决策 → 可能调工具 / 派子 Agent / 直接回复
───────────────────────────────────────────────────────────────────────

  Agent 决定调工具？
  │
  ├─⓮ before_tool_call                         [工具评估]
  │    "这个调用合理吗？"
  │    · Allow / Block / AllowWithWarning
  │    │
  │    ├─ Block → 工具不执行，返回 reason 给 Agent
  │    │
  │    └─ Allow → [工具执行]
  │                 │
  │                 ⓯ after_tool_call           [工具结果处理]
  │                    "结果怎么处理？"
  │                    · Keep / Replace / Externalize / Truncate
  │                    │
  │                    └─ 结果里有图片？
  │                       └─❽ on_inject_media   [复用，处理工具返回的图片]
  │
  Agent 决定派子 Agent？
  │
  ├─⓰ on_sub_agent_spawn                       [准备子上下文]
  │    "子 Agent 需要知道什么？"
  │    · 创建 ContextStore
  │    · 决定继承哪些父上下文
  │    │
  │    ▼ [子 Agent 运行中...]
  │    │
  │    ├─⓱ on_sub_agent_turn                   [观测子进展]
  │    │    "第 N 轮了，在干什么？"
  │    │    · Continue / Steer(纠偏) / Terminate(终止)
  │    │
  │    ├─⓲ on_sub_agent_tool_call              [拦截子工具调用]
  │    │    "子 Agent 要调什么工具？安全吗？"
  │    │    · Allow / Block
  │    │
  │    ▼ [子 Agent 完成]
  │    │
  │    ⓳ on_sub_agent_complete                  [结果回流]
  │       "结果怎么给父 Agent？"
  │       · FullResult / Summary / Externalized / Error
  │
  ▼
───────────────────────────────────────────────────────────────────────
 PHASE 6: 收尾
───────────────────────────────────────────────────────────────────────

  ⓴ ingest                                     [消息入库]
  │  "这条消息要存吗？怎么存？"
  │  · Keep / Modify / Skip / TagAndKeep
  │
  ㉑ after_turn                                 [异步后处理]
  │  "这轮结束了，要做什么？"
  │  │
  │  └─ on_extract_memory                       [记忆提取]
  │     "对话里有值得记住的吗？"
  │     · 过滤候选 → 去重 → 调置信度 → 存入记忆库
  │
  ▼
  [等待用户下一条消息 → 回到 PHASE 2]

═══════════════════════════════════════════════════════════════════════
 PHASE 7: 会话结束
═══════════════════════════════════════════════════════════════════════

  on_agent_end                                  [生命周期终点]
  │  "结束了，审计/统计/清理"
  │
  dispose                                       [释放资源]
```

### 3.6 旧的拦截点详细设计（已废弃，以上图为准）

上下文管理 Agent 在以下点强制介入：

```
用户发消息                    Agent 回复                   工具调用
    │                           │                           │
    ▼                           ▼                           ▼
┌─────────┐              ┌─────────┐              ┌──────────────┐
│ Hook 1  │              │ Hook 3  │              │   Hook 4     │
│ on_user │              │ on_     │              │   on_tool    │
│ _message│              │ before  │              │   _result    │
│         │              │ _llm    │              │              │
└────┬────┘              └────┬────┘              └──────┬───────┘
     │                        │                          │
     │ 决定注入什么            │ 最终裁剪                  │ 大结果处理
     │ (记忆? Skill?)         │ (超预算就压缩)            │ (替换? 外化?)
     ▼                        ▼                          ▼
工作 Agent 收到                LLM 收到                  工具结果进上下文
增强后的消息                   裁剪后的上下文              (可能已被压缩)


创建子 Agent                 子 Agent 完成               Agent 回复后
    │                           │                           │
    ▼                           ▼                           ▼
┌──────────────┐         ┌──────────────┐           ┌─────────────┐
│   Hook 5     │         │   Hook 6     │           │   Hook 2    │
│   on_sub     │         │   on_sub     │           │   on_after  │
│   _agent     │         │   _agent     │           │   _response │
│   _create    │         │   _complete  │           │             │
└──────┬───────┘         └──────┬───────┘           └──────┬──────┘
       │                        │                          │
       │ 准备子 Agent           │ 结果回流策略              │ 提取记忆
       │ 的初始上下文            │ (全量? 摘要? 引用?)       │ 更新元数据
       ▼                        ▼                          ▼
子 Agent 拿到                父 Agent 收到              记忆库更新
精确的任务上下文              精简的结果                  优先级刷新
```

#### 每个 Hook 的具体职责

**Hook 1: `on_user_message(agent_id, message) → EnrichedMessage`**
```
每次用户发消息时触发，决定增强什么：

必做（代码，确定性）：
  ├ 注入 L2 运行时信息（时间戳、session 元数据）
  └ 检查是否有显式指令（"记住这个"/"忘掉"）→ 直接写记忆

Agent 决策（小模型）：
  ├ 要不要注入记忆？→ 查询记忆库，按相关性判断
  ├ 要不要加载 Skill？→ 匹配 Skill 描述符
  └ 消息里有没有"不要放上下文"类指令？→ 标记后续处理
```

**Hook 2: `on_after_response(agent_id, response) → void`**
```
Agent 回复后触发，异步处理，不阻塞：

必做（代码，确定性）：
  ├ 更新消息元数据（token 计数、时间戳）
  ├ 追踪工具调用模式（哪个工具用了多少 token）
  └ 刷新 last_referenced_at

Agent 决策（小模型，异步队列）：
  ├ 提取记忆候选（规则过滤 → LLM 兜底）
  └ 预判下一轮是否需要压缩（提前规划，下轮 Hook 3 执行）
```

**Hook 3: `on_before_llm(agent_id, messages) → Vec<Message>`**
```
发给 LLM 之前的最后关卡，确保上下文健康：

必做（代码，确定性）：
  ├ 按层排序（L0 → L1 描述符 → L1 已加载 → L2 → 对话）
  ├ 计算总 token
  └ 过滤 compacted/externalized 条目

条件触发（代码 + Agent）：
  如果 token > budget * 0.7：
    ├ 先执行确定性规则（丢 Disposable 优先级的条目）
    ├ 还不够 → Agent 决策用哪个压缩工具
    └ 还不够 → 紧急 sliding_window

  如果 token > budget * 0.95：
    └ 硬截断（保留 L0 + 最近 N 条），不过 Agent
```

**Hook 4: `on_tool_result(agent_id, tool_name, result) → ModifiedResult`**
```
每次工具返回结果时触发：

必做（代码，确定性）：
  ├ 计算结果 token 数
  ├ 超过 2000 行 / 50KB → 截断 + 标记
  └ 追踪到 tool_patterns

Agent 决策（小模型）：
  如果 result_tokens > large_threshold (默认 5000)：
    ├ 需要全量保留？（看工具类型和角色）
    ├ 可以替换为摘要？→ ReplaceToolResult
    └ 可以外化到文件？→ ExternalizeToFile + 留引用
```

**Hook 5: `on_sub_agent_spawn(parent_id, child_config) → ContextStore`**
```
创建子 Agent 时，准备它的初始上下文：

必做（代码，确定性）：
  ├ 创建空 ContextStore
  ├ 注入 L0（从父 Agent 继承身份 + 约束的精简版）
  └ 注入任务指令作为第一条 HumanMessage

Agent 决策（小模型）：
  ├ 父 Agent 的哪些上下文需要传给子 Agent？
  ├ 传全文还是传摘要？
  └ 需不需要注入相关记忆？
```

**Hook 6: `on_sub_agent_complete(parent_id, child_id, result) → InjectionPlan`**
```
子 Agent 完成后，决定结果怎么回流到父 Agent：

Agent 决策（小模型）：
  ├ 结果小（< 500 tokens）→ 全量作为 tool_result
  ├ 结果中等（500-5000）→ 摘要 + 关键标识符
  ├ 结果大（> 5000）→ 外化到文件 + 一句话结论
  └ 失败/超时 → 错误摘要 + 建议
```

### 3.4 SDK 方法

上下文管理 Agent 通过 SDK 操作其他 Agent 的上下文，这些是**代码实现的确定性工具**：

```rust
/// 上下文管理 SDK — 系统内置 Agent 独享的特权接口
pub trait ContextManagementSDK: Send + Sync {
    // ─── 读取（任何 Hook 都可用）───

    /// 获取目标 Agent 的上下文快照（不含完整内容，只有元数据）
    fn snapshot(&self, agent_id: &str) -> ContextSnapshot;

    /// 获取 token 预算和用量
    fn budget(&self, agent_id: &str) -> BudgetInfo;

    /// 查看某条消息的完整内容
    fn read_message(&self, agent_id: &str, message_id: &str) -> Option<ContextEntry>;

    /// 最近 N 轮的工具调用模式分析
    fn tool_patterns(&self, agent_id: &str, last_n: usize) -> Vec<ToolPattern>;

    // ─── 注入（向上下文添加内容）───

    /// 在消息列表中插入一条（用于注入记忆、Skill 等）
    fn inject_message(&self, agent_id: &str, layer: ContextLayer, message: AgentMessage);

    /// 从记忆库查询并注入
    fn inject_memory(&self, agent_id: &str, query: &str, max_tokens: usize) -> Vec<MemoryFact>;

    /// 从外化文件回读
    fn inject_from_file(&self, agent_id: &str, path: &str, lines: Option<(usize, usize)>);

    // ─── 直接写操作（增删改存量上下文）───

    /// 删除一条消息
    fn remove_message(&self, agent_id: &str, message_id: &str);

    /// 删除指定范围的消息
    fn remove_range(&self, agent_id: &str, range: &MessageRange);

    /// 重写一条消息的内容（保留 id 和元数据，替换内容）
    fn rewrite_message(&self, agent_id: &str, message_id: &str, new_content: AgentMessage);

    /// 批量重写（maintain 阶段常用，可一次性改多条历史消息）
    fn rewrite_batch(&self, agent_id: &str, rewrites: Vec<(String, AgentMessage)>);

    /// 清空某一层的全部内容
    fn clear_layer(&self, agent_id: &str, layer: ContextLayer);

    /// 清空全部对话消息（保留 L0 常驻层）
    fn clear_conversation(&self, agent_id: &str);

    /// 核弹：清空全部上下文（含 L0），重置到空状态
    fn clear_all(&self, agent_id: &str);

    // ─── 压缩（缩减上下文的快捷方法）───

    /// 滑动窗口：只保留最近 N 条对话消息（L0/L1 不动）
    fn sliding_window(&self, agent_id: &str, keep_recent: usize);

    /// 替换某条工具结果为摘要文本
    fn replace_tool_result(&self, agent_id: &str, message_id: &str, summary: &str);

    /// 将指定范围的消息外化到文件，上下文里只留路径引用
    fn externalize(&self, agent_id: &str, range: MessageRange, path: &str);

    /// 请求 LLM 摘要（异步，返回摘要文本）
    async fn summarize(&self, agent_id: &str, range: MessageRange, hints: &[String]) -> String;

    // ─── 元数据（标记管理）───

    /// 标记消息优先级
    fn tag_priority(&self, agent_id: &str, message_id: &str, priority: Priority);

    /// 标记为排除（下次压缩时优先移除）
    fn tag_exclude(&self, agent_id: &str, message_id: &str);

    // ─── 记忆（跨会话）───

    /// 查询记忆
    fn query_memory(&self, query: &str, max_results: usize) -> Vec<MemoryFact>;

    /// 写入记忆
    fn store_memory(&self, fact: MemoryFact);

    /// 删除记忆
    fn delete_memory(&self, fact_id: &str);

    // ─── 子 Agent 管理 ───

    /// 为子 Agent 创建初始上下文
    fn create_child_context(&self, parent_id: &str, child_config: &AgentRoleConfig) -> ContextStore;

    /// 从父 Agent 上下文中提取与任务相关的片段
    fn extract_relevant(&self, agent_id: &str, task_description: &str, max_tokens: usize) -> Vec<ContextEntry>;
}
```

### 3.5 确定性 vs Agent 决策的边界

这是可控性的关键——**哪些用代码写死，哪些让 Agent 判断**：

```
确定性（代码实现，100% 可预测）          Agent 决策（小模型，有判断力）
──────────────────────────────────────────────────────────────────
✓ 层级排序（L0 → L1 → L2 → 对话）     ✧ 要不要注入记忆？注入哪些？
✓ Token 计数和预算检查                  ✧ 工具大结果：替换还是外化？
✓ 硬截断（95% 预算兜底）               ✧ 子 Agent 回流结果：全量还是摘要？
✓ 工具结果行数/字节截断                 ✧ 哪些旧消息可以丢弃？
✓ 消息元数据更新                        ✧ Skill 是否需要加载？
✓ 记忆去重（SHA1 + Dice）              ✧ 压缩时保留哪些、丢弃哪些？
✓ 记忆置信度衰减                        ✧ 是否需要从外化文件回读？
✓ Prompt Cache 友好的排序               ✧ 子 Agent 需要父上下文的哪些片段？
✓ 运行时信息注入（时间戳等）
✓ 显式指令检测（"记住"/"忘掉"）
```

即使 Agent 完全不工作（模型挂了），确定性规则也能保证系统不崩：
- 层级排序保证 Prompt Cache
- 硬截断保证不溢出
- 工具截断保证单条不爆
- 记忆去重保证不重复

Agent 决策是**锦上添花**——让上下文更精准、更智能，但不是系统存活的前提。

### 3.6 工具集

```rust
/// 上下文管理 Agent 可用的工具
pub enum ContextTool {
    // === 感知工具（只读）===
    /// 返回当前上下文的 token 数、各层占比、消息条数
    InspectContext,
    /// 查看某条消息的内容和元数据
    InspectMessage { message_id: String },
    /// 返回 token 预算、模型窗口上限、已用/剩余
    InspectBudget,
    /// 分析最近 N 轮的 tool call 模式（哪个工具贡献最多 token）
    AnalyzeToolPatterns { last_n_turns: usize },

    // === 压缩工具（写入）===
    /// 滑动窗口：丢弃最早的 N 条消息
    SlidingWindow { keep_recent: usize },
    /// LLM 摘要：对指定范围的消息生成结构化摘要
    Summarize {
        range: MessageRange,
        keep_hints: Vec<String>,  // 强制保留的关键信息类型
    },
    /// 工具结果替换：用一句话结论替换原始工具输出
    ReplaceToolResult {
        message_id: String,
        summary: String,
    },
    /// 外化到文件：将消息内容写入文件，上下文里只留路径引用
    ExternalizeToFile {
        range: MessageRange,
        file_path: String,
    },

    // === 注入工具（写入）===
    /// 从记忆库查询并注入到上下文
    InjectMemory { query: String, max_tokens: usize },
    /// 从外化文件回读指定片段
    InjectFromFile { file_path: String, line_range: Option<(usize, usize)> },

    // === 元数据工具 ===
    /// 标记消息的保留优先级
    TagPriority { message_id: String, priority: Priority },
    /// 标记消息为下次压缩时优先移除
    TagExclude { message_id: String },
}

pub enum Priority { Critical, High, Normal, Low, Disposable }

pub struct MessageRange {
    pub from: MessageSelector,  // ByIndex(0) | ById("msg-xxx") | FromStart
    pub to: MessageSelector,    // ByIndex(10) | ById("msg-yyy") | ToEnd
}
```

### 3.4 决策框架（System Prompt 核心片段）

```markdown
## 你的角色
你是上下文管理专家。你的工作是为其他 Agent 维护健康的上下文窗口。

## 决策原则
1. 先观察，再行动 — 用 InspectContext 和 AnalyzeToolPatterns 了解现状
2. 最小干预 — 只在必要时压缩，能不动就不动
3. 保护前缀稳定性 — 优先压缩靠后的层（运行时 > 按需加载 > 常驻）
4. 保留标识符 — UUID/hash/路径/端口/URL 必须原样保留，绝不改动

## 保留优先级（压缩时严格遵守）
1. 用户原始意图和约束 — Critical，不可压缩
2. 架构决策和理由 — Critical，不可压缩
3. 精确标识符（文件路径、commit hash、端口号） — Critical，原样保留
4. 已修改文件列表和关键变更 — High
5. 验证状态 pass/fail — High
6. 未完成的 TODO — High
7. 工具输出 — Low，可替换为一句结论
8. 中间推理过程 — Disposable，可丢弃

## 你不知道的
- 你不知道当前 Agent 具体在干什么任务
- 你不知道哪些业务信息重要

## 你怎么判断
1. 观察 tool call 模式：频繁图片解析 → 视觉密集型，积极外化旧图片结果
2. 观察 token 增长：哪个工具贡献最多 → 优先压缩该工具的历史输出
3. 观察引用模式：旧消息从未被后续引用 → 安全丢弃
4. 遵守用户指令："不要放上下文" → 绝对排除，并通知记忆更新

## 你的记忆
- 查阅你的记忆，看类似任务之前怎么处理的
- 如果用户纠正了你的决策，更新记忆
```

### 3.5 进化机制

```
用户反馈            信号                      调整行为
─────────────────────────────────────────────────────────
显式反馈            "这个不用放上下文"          TagExclude + 记忆：该类内容排除
                   "把之前那个拉回来"          InjectFromFile + 记忆：压缩太激进
token 消耗          某 MCP 工具反复返回大结果    记忆：该工具自动 ReplaceToolResult
                   某类消息从不被引用           记忆：降低保留优先级
任务结果            执行者缺上下文出错          记忆：回溯压缩错误，调高保留阈值
```

进化规则写入上下文管理 Agent 自身的 L3 记忆层，下次同类场景复用。

## 4. Agent 角色类型

### 4.1 四种内置角色

| 角色 | 本质 | 上下文特征 | 状态性 |
|------|------|-----------|--------|
| **指导者 (Conductor)** | 和用户对齐，拆任务，分派 | 广而浅：全局意图 + 任务拓扑 + 各 Agent 结论摘要 | 有状态 |
| **执行者 (Executor)** | 动手完成具体任务 | 窄且精：任务指令 + 相关引用 | 可配置（有状态/无状态） |
| **调研者 (Researcher)** | 搜索信息，分析资料 | 宽且深：原始数据按需加载，产出压缩成结论 | 通常无状态 |
| **校验者 (Validator)** | 检查产出质量 | 对比型：需求 spec(左) + 产出物(右) | 通常无状态 |

### 4.2 角色 vs 工具

**角色 = 编排中的职责**（指导者/执行者/调研者/校验者），描述 Agent 在工作流里的位置。
**工具 = Agent 可调用的能力**（视觉解析/代码执行/网页抓取/数据解析/...），实现方式透明（可以是 Agent-backed、API、MCP、本地函数）。

角色不限定工具，工具不限定角色。一个 PPT 执行者和一个代码执行者调用不同的工具集，但都是执行者。视觉解析是工具，不是角色——任何角色都可以调用它。

### 4.3 角色配置

执行者可以是 PPT 生成、Excel 编辑、FFmpeg 剪辑、代码编写——任何具体任务。定义时需要配置：

```rust
pub struct AgentRoleConfig {
    /// 角色类型
    pub role: AgentRole,
    /// 领域描述（自然语言）
    pub domain: String,
    /// 是否有状态（跨轮记忆）
    pub stateful: bool,
    /// 工具集
    pub tools: ToolSet,
    /// 校验方式（可选：派另一个 Agent 校验）
    pub validation: Option<ValidationConfig>,
    /// 上下文预算偏好
    pub context_budget: ContextBudget,
}

pub enum AgentRole {
    Conductor,
    Executor,
    Researcher,
    Validator,
}

pub enum ContextBudget {
    Auto,                    // 上下文管理 Agent 自动决定
    Fixed { max_tokens: usize },
    Fraction { of_window: f32 },  // 如 0.6 = 用窗口的 60%
}

pub struct ValidationConfig {
    /// 校验 Agent 的角色配置（通常是一个无状态 Validator）
    pub validator: Box<AgentRoleConfig>,
    /// 校验时机
    pub trigger: ValidationTrigger,
}

pub enum ValidationTrigger {
    AfterEachStep,      // 每步都验
    AfterCompletion,    // 完成后验
    OnDemand,           // 手动触发
}
```

### 4.4 上下文管理 Agent 对不同角色的默认行为

上下文管理 Agent 不硬编码策略，但它的 system prompt 中包含**观察指引**：

```
## 角色观察指引（参考，非规则）

当你管理 Conductor 的上下文时：
- 倾向：保摘要丢细节
- 警惕：子 Agent 原始输出堆积
- 工具返回的大结果（如视觉解析）积极 ReplaceToolResult

当你管理 Executor 的上下文时：
- 先观察它在做什么类型的任务（通过 tool call 模式判断）
- 有状态执行者：保留文件追踪信息，增量压缩旧推理
- 工具输出占比高时（如频繁调视觉解析工具）：积极外化旧结果

当你管理 Researcher 的上下文时：
- 倾向：加载完数据后积极外化原始数据，只保留分析结论

当你管理 Validator 的上下文时：
- 警惕：双视图（需求+产出）是它的核心，不能压缩任何一边
```

## 5. 数据模型

### 5.1 ContextEntry（上下文条目）

扩展现有 `AgentMessage`，增加上下文管理元数据：

```rust
/// 上下文条目 = AgentMessage + 管理元数据
pub struct ContextEntry {
    pub id: String,
    pub message: AgentMessage,
    pub metadata: ContextMetadata,
}

pub struct ContextMetadata {
    /// 所属层级
    pub layer: ContextLayer,
    /// 保留优先级（可由上下文管理 Agent 动态调整）
    pub priority: Priority,
    /// 估算 token 数
    pub estimated_tokens: usize,
    /// 是否已被压缩/替换
    pub compacted: bool,
    /// 外化文件路径（如果已外化）
    pub externalized_path: Option<String>,
    /// 原始内容的摘要（如果已替换）
    pub replacement_summary: Option<String>,
    /// 来源 Agent
    pub source_agent: Option<String>,
    /// 创建时间
    pub created_at: i64,
    /// 最后被引用时间（用于判断是否可丢弃）
    pub last_referenced_at: Option<i64>,
}

pub enum ContextLayer {
    AlwaysPresent,   // L0
    OnDemand,        // L1
    RuntimeInject,   // L2
    Memory,          // L3
    // L4 不进上下文，无对应枚举值
}
```

### 5.2 ContextStore（上下文存储）

```rust
/// 管理单个 Agent 实例的上下文
pub struct ContextStore {
    /// 按层组织的条目
    entries: Vec<ContextEntry>,
    /// token 预算
    budget: ContextBudget,
    /// 模型窗口大小
    model_window: usize,
    /// 外化文件目录
    externalize_dir: PathBuf,
}

impl ContextStore {
    /// 当前总 token 数
    pub fn total_tokens(&self) -> usize;
    /// 各层 token 占比
    pub fn layer_breakdown(&self) -> HashMap<ContextLayer, usize>;
    /// 构建发给 LLM 的消息列表（按层排序，跳过已外化的）
    pub fn build_llm_messages(&self) -> Vec<Message>;
    /// 追加条目
    pub fn append(&mut self, entry: ContextEntry);
    /// 替换条目内容（压缩用）
    pub fn replace_content(&mut self, id: &str, summary: String);
    /// 外化条目到文件
    pub fn externalize(&mut self, id: &str, path: &str) -> Result<()>;
    /// 按范围移除条目
    pub fn remove_range(&mut self, range: &MessageRange);
    /// 按优先级排序，返回最低优先级的 N 个条目
    pub fn lowest_priority(&self, n: usize) -> Vec<&ContextEntry>;
}
```

### 5.3 ContextSnapshot（快照，传给上下文管理 Agent）

上下文管理 Agent 不看完整内容，只看快照：

```rust
/// 上下文快照 — 传给上下文管理 Agent 做决策
pub struct ContextSnapshot {
    pub total_tokens: usize,
    pub budget_tokens: usize,
    pub model_window: usize,
    pub usage_ratio: f32,  // total / window
    pub layer_breakdown: HashMap<ContextLayer, LayerStats>,
    pub entries: Vec<EntrySnapshot>,
    pub recent_tool_patterns: Vec<ToolPattern>,
}

pub struct LayerStats {
    pub token_count: usize,
    pub entry_count: usize,
    pub percentage: f32,
}

pub struct EntrySnapshot {
    pub id: String,
    pub layer: ContextLayer,
    pub priority: Priority,
    pub estimated_tokens: usize,
    pub source: Option<String>,       // 来源 agent 名
    pub content_type: ContentType,    // Text/ToolResult/Image/Code/...
    pub age_turns: usize,             // 多少轮前产生的
    pub last_referenced_turns: Option<usize>,  // 多少轮前被引用过
    pub preview: String,              // 前 100 字符预览
}

pub struct ToolPattern {
    pub tool_name: String,
    pub call_count: usize,
    pub avg_result_tokens: usize,
    pub total_result_tokens: usize,
}
```

## 6. 压缩策略实现

### 6.1 滑动窗口

最简单，直接丢旧消息：

```rust
impl ContextStore {
    pub fn sliding_window(&mut self, keep_recent: usize) {
        // 保留 L0 + L1 描述符（常驻不动）
        // 从对话消息中只保留最近 keep_recent 条
        // 丢弃的消息标记 compacted = true
    }
}
```

### 6.2 LLM 摘要

用小模型生成结构化摘要，保留关键信息：

```rust
pub struct SummarizeRequest {
    pub messages: Vec<ContextEntry>,
    pub keep_hints: Vec<String>,  // 强制保留的信息类型
}

pub struct SummarizeResult {
    pub summary: String,           // 结构化摘要
    pub preserved_identifiers: Vec<String>,  // 提取的精确标识符
}

// 摘要的结构化 prompt 模板
const SUMMARIZE_PROMPT: &str = r#"
Summarize the following conversation segment. Output in this structure:

## Goal
[One sentence: what the user wants to achieve]

## Key Decisions
[Bullet list of architectural/design decisions made, with reasons]

## Modified Files
[Exact file paths that were created/modified, with one-line description each]

## Verification Status
[What has been tested/verified, pass/fail]

## Open Items
[Unfinished TODO items, blocking issues]

## Preserved Identifiers
[List ALL UUIDs, hashes, IPs, ports, URLs, file paths mentioned — verbatim, no modifications]

IMPORTANT: Do NOT modify any identifier. Copy them exactly as they appear.
"#;
```

### 6.3 工具结果替换

单条替换，成本最低：

```rust
impl ContextStore {
    pub fn replace_tool_result(&mut self, message_id: &str, summary: &str) {
        // 找到 message_id 对应的 ToolResult
        // 保留 tool_call_id（保持调用链完整）
        // 用 summary 替换 content
        // 标记 replacement_summary
    }
}
```

### 6.4 外化到文件

写文件，上下文里只留引用：

```rust
impl ContextStore {
    pub fn externalize_to_file(&mut self, range: &MessageRange, path: &str) -> Result<()> {
        // 1. 把范围内的消息完整写入 path（JSON 格式，可追溯）
        // 2. 在上下文里插入引用消息：
        //    "[此处内容已外化到 {path}，如需查看可使用 InjectFromFile 工具]"
        // 3. 移除原始消息
        // 4. 标记 externalized_path
    }

    pub fn inject_from_file(&mut self, path: &str, line_range: Option<(usize, usize)>) -> Result<()> {
        // 1. 读取文件（可选行范围）
        // 2. 作为 RuntimeInject 层条目插入
        // 3. 标记 priority = Low（回读内容用完可丢）
    }
}
```

## 7. 记忆系统

### 7.1 记忆提取（三层过滤）

借鉴 LobsterAI 的方案，规则优先 + LLM 兜底：

```
用户消息 → 显式检测（正则："记住这个"/"忘掉"）
                 ↓ miss
           → 隐式模式匹配（个人事实、偏好、约束）
                 ↓ 边界 case
           → LLM 判断（小模型，10min TTL 缓存）
                 ↓
           → 去重（SHA1 + bigram Dice，阈值 0.82）
                 ↓
           → 写入 MemoryStore
```

### 7.2 记忆数据模型

```rust
pub struct MemoryFact {
    pub id: String,
    pub text: String,
    pub fingerprint: String,       // SHA1 hash for dedup
    pub confidence: f32,           // 0.0-1.0
    pub category: MemoryCategory,
    pub source_session: String,
    pub created_at: i64,
    pub last_accessed_at: i64,
    pub access_count: u32,
}

pub enum MemoryCategory {
    UserPreference,    // "我喜欢简洁的回答"
    UserProfile,       // "我是后端工程师"
    ProjectContext,    // "我们用 Rust + GPUI"
    TaskPattern,       // "视觉解析任务上次压缩太激进"
    Constraint,        // "代码不要用 unwrap"
}
```

### 7.3 记忆注入

注入到 user message 前缀（非 system prompt），利于 Prompt Caching：

```xml
<user_memory>
- 用户是后端工程师，偏好简洁回答
- 项目使用 Rust + GPUI，macOS 首发
- 代码规范：不用 unwrap，错误用 thiserror
</user_memory>

{actual_user_message}
```

Max 2000 tokens，按 confidence × access_count 排序，取 top-N。

### 7.4 记忆衰减

长期未访问的记忆 confidence 衰减：

```
effective_confidence = confidence * decay_factor(days_since_last_access)
decay_factor(d) = max(0.3, 1.0 - d * 0.005)  // 200 天衰减到 0.3 下限
```

低于阈值（0.3）的记忆标记为 archived，不再注入但不删除。

## 8. 集成到现有架构

### 8.1 作为 Middleware 接入

上下文管理 Agent 通过现有 `Middleware` trait 接入 agent-core：

```rust
pub struct ContextManagementMiddleware {
    context_store: Arc<Mutex<ContextStore>>,
    context_agent: Arc<ContextAgent>,  // 小模型驱动的管理 Agent
    config: ContextConfig,
}

#[async_trait]
impl Middleware for ContextManagementMiddleware {
    async fn before_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        let store = self.context_store.lock().await;

        // 1. 检查是否需要触发上下文管理
        if store.usage_ratio() > self.config.trigger_threshold {
            let snapshot = store.snapshot();
            let actions = self.context_agent.decide(snapshot).await?;
            self.execute_actions(actions, &mut store).await?;
        }

        // 2. 构建发给 LLM 的消息（按层排序）
        *messages = store.build_llm_messages();
        Ok(())
    }

    async fn after_tool_call(
        &self,
        ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        result: &mut ToolResult,
    ) -> Result<(), MiddlewareError> {
        let mut store = self.context_store.lock().await;

        // 追踪工具调用模式
        store.track_tool_call(&tool_call.name, result.estimated_tokens());

        // 大结果自动标记低优先级
        if result.estimated_tokens() > self.config.large_result_threshold {
            store.tag_priority(&result.id, Priority::Low);
        }

        Ok(())
    }
}
```

### 8.2 与 ContextTransform 管线配合

现有 `ContextTransform` 处理确定性变换，`ContextManagementMiddleware` 处理智能决策：

```
消息流:
  AgentState.messages
    → ContextTransform pipeline (确定性：格式转换、标记过滤)
    → ContextManagementMiddleware.before_llm_call (智能：压缩、注入)
    → convert_to_llm (格式适配)
    → LLM
```

### 8.3 与 SubAgentConfig 配合

子 Agent 创建时，上下文管理 Agent 为其准备独立的 ContextStore：

```rust
impl SubAgentContextFactory {
    pub fn create_for_role(
        parent_store: &ContextStore,
        role_config: &AgentRoleConfig,
    ) -> ContextStore {
        let mut child_store = ContextStore::new(role_config.context_budget);

        // L0: 从 parent 继承身份 + 约束（精简版）
        child_store.inject_layer(ContextLayer::AlwaysPresent,
            parent_store.get_layer(ContextLayer::AlwaysPresent));

        // 不继承 parent 的对话历史
        // 只注入任务指令作为 L2

        child_store
    }
}
```

## 9. 实现分阶段

### Phase 1: 基础数据模型 + 存储

- `ContextEntry` / `ContextMetadata` / `ContextLayer` / `Priority` 类型定义
- `ContextStore` CRUD 实现
- `ContextSnapshot` 快照生成
- 单元测试：层级管理、token 计算、条目增删

### Phase 2: 确定性压缩工具

- `SlidingWindow` 实现
- `ReplaceToolResult` 实现
- `ExternalizeToFile` + `InjectFromFile` 实现
- `ContextTransform` 集成（作为管线步骤）
- 单元测试：各压缩策略的正确性

### Phase 3: LLM 摘要

- `Summarize` 工具实现（调用小模型）
- 结构化摘要 prompt 模板
- 增量摘要（有旧摘要时用 UPDATE 模式合并）
- 标识符保护测试

### Phase 4: 上下文管理 Agent

- `ContextAgent` 实现（工具驱动 Agent）
- System prompt 编写（决策框架 + 保留优先级 + 观察指引）
- `ContextManagementMiddleware` 接入 agent-core
- 集成测试：触发条件 → 决策 → 执行

### Phase 5: Agent 角色类型

- `AgentRoleConfig` 定义
- `SubAgentContextFactory` 实现
- 内置角色模板（指导者/执行者/调研者/校验者）
- 用户自定义角色 UI

### Phase 6: 记忆系统

- 三层记忆提取（显式/隐式/LLM）
- `MemoryFact` 存储 + 去重
- 记忆注入（user message 前缀）
- 置信度衰减 + 归档
- 上下文管理 Agent 进化记忆

### Phase 7: 端到端编排

- 指导者 → 执行者 → 校验者的完整 flow
- 多执行者交叉校验
- Agent-backed 工具（视觉解析等）的调用与上下文隔离
- 压力测试：长对话、多子Agent、大工具输出

## 10. 风险与应对

| 风险 | 影响 | 应对 |
|------|------|------|
| 上下文管理 Agent 决策错误 | 压缩了关键信息 | 外化而非删除（可追溯）；用户反馈修正 + 记忆进化 |
| 额外 token 成本 | 每次触发多一轮 LLM 调用 | 用小模型；只在阈值触发时运行 |
| 管理 Agent 自身的延迟 | 影响响应速度 | 异步触发；紧急压缩走确定性规则不过 Agent |
| 递归问题（谁管管理者） | 无限嵌套 | 管理 Agent 无状态，每次拿快照做决策，自身不积累上下文 |
| 摘要丢关键标识符 | 后续工具调用失败 | Prompt 强调原样保留；后处理校验标识符完整性 |

## 11. 不做的事（YAGNI）

- 不做实时 Prompt Caching 优化 — 靠层级排序自然命中
- 不做跨 Agent 实例的共享上下文 — 通过记忆层间接共享
- 不做对话分支（pi-mono 树状模型） — 当前只需线性对话
- 不做 GUI 上的"上下文可视化" — 后续可加，不是 P0
