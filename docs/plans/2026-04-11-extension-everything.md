# Extension Everything — App-Core 插件化重构

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 alva-app-core 中的 skills、mcp、hooks、analytics、auth、lsp、evaluation 模块全部转为 Extension 插件，让 BaseAgent 变成纯骨架。

**Architecture:** 每个可选能力变成独立的 `Extension` 实现。Extension trait 已经是 async 的（tools/middleware/configure 全异步）。BaseAgent builder 只保留核心骨架（Bus + SecurityMW + AgentState），所有功能通过 `.extension()` 注入。

**Tech Stack:** Rust, async_trait, alva-agent-core Middleware, alva-types Tool

---

## 前置知识

**Extension trait（已完成）：**
```rust
#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> { vec![] }
    async fn configure(&self, _ctx: &ExtensionContext) {}
}
```

**ExtensionContext：**
```rust
pub struct ExtensionContext {
    pub bus: BusHandle,
    pub workspace: PathBuf,
    pub tool_names: Vec<String>,
}
```

**builder.rs 调用顺序：**
1. `ext.tools().await` → 收集工具
2. `ext.middleware().await` → 收集中间件
3. `middleware_stack.configure_all(...)` → 配置中间件
4. `ext.configure(&ctx).await` → 配置扩展（bus/workspace/tool_names 可用）

**关键文件：**
- Extension trait: `crates/alva-app-core/src/extension/mod.rs`
- Builtins: `crates/alva-app-core/src/extension/builtins.rs`
- Builder: `crates/alva-app-core/src/base_agent/builder.rs`
- BaseAgent: `crates/alva-app-core/src/base_agent/agent.rs`
- CLI setup: `crates/alva-app-cli/src/agent_setup.rs`
- Eval main: `crates/alva-agent-eval/src/main.rs`
- lib.rs: `crates/alva-app-core/src/lib.rs`

---

### Task 1: SkillsExtension

**目标：** 将 SkillStore 创建逻辑从 builder.rs 移入 SkillsExtension，同时提供 SearchSkillsTool + UseSkillTool + SkillInjectionMiddleware。

**Files:**
- Modify: `crates/alva-app-core/src/extension/builtins.rs` — 添加 SkillsExtension
- Modify: `crates/alva-app-core/src/base_agent/builder.rs` — 删除 SkillStore 创建逻辑（步骤5，约 ~256-275 行），删除 `skill_dirs` 字段和 `skill_dir()` 方法
- Modify: `crates/alva-app-core/src/base_agent/agent.rs` — 将 `skill_store` 改为 `Option<Arc<SkillStore>>`，或通过 bus 获取
- Modify: `crates/alva-app-cli/src/agent_setup.rs` — 用 `.extension(Box::new(SkillsExtension::new(dirs)))` 替换 `.skill_dir()`
- Modify: `crates/alva-app-core/src/lib.rs` — 如需要

**设计要点：**

SkillStore 创建是 sync 的（`SkillStore::new(repo)`），scan 是 async 的（`store.scan().await`）。

模式：
- 构造函数创建 SkillStore（sync）— Arc 引用共享给 tools 和 middleware
- tools() 返回持有 Arc<SkillStore> 的 SearchSkillsTool + UseSkillTool  
- middleware() 返回持有 Arc<SkillStore> 的 SkillInjectionMiddleware
- configure() 调用 `store.scan().await` 填充数据 + 注册 store 到 bus

```rust
pub struct SkillsExtension {
    store: Arc<SkillStore>,
    loader: Arc<SkillLoader>,
    injector: Arc<SkillInjector>,
}

impl SkillsExtension {
    pub fn new(skill_dirs: Vec<PathBuf>) -> Self {
        let primary_dir = skill_dirs.first().cloned()
            .unwrap_or_else(|| PathBuf::from(".alva/skills"));
        let repo = Arc::new(FsSkillRepository::new(
            primary_dir.join("bundled"),
            primary_dir.join("mbb"),
            primary_dir.join("user"),
            primary_dir.join("state.json"),
        ));
        let store = Arc::new(SkillStore::new(repo.clone() as Arc<dyn SkillRepository>));
        let loader = Arc::new(SkillLoader::new(/* params from current code */));
        let injector = Arc::new(SkillInjector::new(/* params from current code */));
        Self { store, loader, injector }
    }
}

#[async_trait]
impl Extension for SkillsExtension {
    fn name(&self) -> &str { "skills" }
    fn description(&self) -> &str { "Skill discovery, loading, and injection" }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(SearchSkillsTool { store: self.store.clone() }),
            Box::new(UseSkillTool { store: self.store.clone(), loader: self.loader.clone() }),
        ]
    }

    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(SkillInjectionMiddleware::with_defaults(
            self.store.clone(), self.injector.clone(),
        ))]
    }

    async fn configure(&self, _ctx: &ExtensionContext) {
        let _ = self.store.scan().await;
    }
}
```

builder.rs 变更：
- 删除 `skill_dirs: Vec<PathBuf>` 字段
- 删除 `pub fn skill_dir()` 方法
- 删除步骤 5 的 SkillStore 创建代码
- BaseAgent 的 `skill_store` 字段变为 `Option<Arc<SkillStore>>`，默认 None

**Step 1:** 在 builtins.rs 添加 SkillsExtension 实现

**Step 2:** 修改 builder.rs — 删除 skill_dirs 字段、skill_dir() 方法、SkillStore 创建代码

**Step 3:** 修改 agent.rs — skill_store 字段改为 Option，在 build() 中设 None

**Step 4:** 修改 agent_setup.rs — 替换 .skill_dir() 为 .extension(SkillsExtension)

**Step 5:** `cargo check && cargo test -p alva-app-core`

**Step 6:** Commit: `refactor: extract SkillsExtension from builder`

---

### Task 2: HooksExtension

**目标：** 将 HookExecutor 包装为 Middleware Extension，覆盖 PreToolUse / PostToolUse / SessionStart / SessionEnd 生命周期。

**Files:**
- Modify: `crates/alva-app-core/src/extension/builtins.rs` — 添加 HooksExtension
- 不改 hooks/ 模块本身（HookExecutor 保留，Extension 是封装层）

**设计要点：**

HooksExtension 需要 HooksSettings（钩子配置）和 workspace+session_id（执行环境）。
- 构造函数接收 HooksSettings
- middleware() 返回 HooksMiddleware
- HooksMiddleware 在 configure 阶段从 ExtensionContext 获取 workspace

```rust
pub struct HooksExtension {
    settings: HooksSettings,
}

impl HooksExtension {
    pub fn new(settings: HooksSettings) -> Self {
        Self { settings }
    }
}

struct HooksMiddleware {
    settings: HooksSettings,
    workspace: OnceLock<PathBuf>,
}

#[async_trait]
impl Middleware for HooksMiddleware {
    fn name(&self) -> &str { "hooks" }

    async fn on_agent_start(&self, _state: &mut AgentState) -> Result<(), MiddlewareError> {
        // Run SessionStart hooks
    }

    async fn on_agent_end(&self, _state: &mut AgentState, _error: Option<&str>) -> Result<(), MiddlewareError> {
        // Run SessionEnd hooks  
    }

    async fn before_tool_call(&self, _state: &mut AgentState, tool_call: &ToolCall) -> Result<(), MiddlewareError> {
        // Run PreToolUse hooks, block if exit code 2
    }

    async fn after_tool_call(&self, _state: &mut AgentState, tool_call: &ToolCall, _output: &mut ToolOutput) -> Result<(), MiddlewareError> {
        // Run PostToolUse / PostToolUseFailure hooks
    }
}
```

**Step 1:** 在 builtins.rs 添加 HooksMiddleware + HooksExtension 实现

**Step 2:** `cargo check && cargo test -p alva-app-core`

**Step 3:** Commit: `feat: add HooksExtension wrapping HookExecutor as middleware`

---

### Task 3: McpExtension

**目标：** 将 MCP 集成（目前是死代码）通过 Extension 正式接入 BaseAgent。

**Files:**
- Modify: `crates/alva-app-core/src/extension/builtins.rs` — 添加 McpExtension
- 不改 mcp/ 模块（McpManager、McpToolAdapter、McpRuntimeTool 保留）

**设计要点：**

MCP 连接是 async 的，configure() 是 async 的，完美适配。

```rust
pub struct McpExtension {
    config_paths: Vec<PathBuf>,  // mcp.json 路径列表（global + project）
}

impl McpExtension {
    pub fn new(config_paths: Vec<PathBuf>) -> Self { ... }
    
    /// 从 AlvaPaths 创建
    pub fn from_paths(paths: &AlvaPaths) -> Self {
        Self::new(vec![
            paths.global_mcp_config(),
            paths.project_mcp_config(),
        ])
    }
}

#[async_trait]
impl Extension for McpExtension {
    fn name(&self) -> &str { "mcp" }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        // 返回 McpRuntimeTool（服务器管理工具）
        // 实际 MCP 工具在 configure 阶段动态注册
        vec![]
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        // 1. 加载 mcp.json 配置
        // 2. 创建 McpManager
        // 3. 注册所有服务器
        // 4. auto-connect
        // 5. 将 McpManager 注册到 bus（供其他组件调用）
    }
}
```

**注意：** MCP 工具是动态发现的（连接后才知道有哪些工具）。configure() 阶段连接服务器后，需要一种方式把发现的工具注入 agent。

方案：在 ExtensionContext 中添加 `tool_registry: Arc<Mutex<ToolRegistry>>` 或通过 bus 注册动态工具。
或者：McpExtension 在构造时就连接（async new），tools() 返回已发现的工具。

这个任务比较复杂，需要先看 McpManager 和 McpTransportFactory 的具体实现来决定最佳方案。

**Step 1:** 调研 McpManager 和 McpTransportFactory 的构造要求

**Step 2:** 在 builtins.rs 添加 McpExtension 实现

**Step 3:** 在 agent_setup.rs 中注册 McpExtension

**Step 4:** `cargo check && cargo test`

**Step 5:** Commit: `feat: add McpExtension to wire up MCP integration`

---

### Task 4: Simple Extension Wrappers（批量）

**目标：** 为 analytics、auth、lsp、evaluation 模块各写一个 Extension 封装。这些模块目前都未接入 BaseAgent，写 Extension 是首次接入。

**Files:**
- Modify: `crates/alva-app-core/src/extension/builtins.rs` — 添加 4 个 Extension

**4a. AnalyticsExtension**
```rust
pub struct AnalyticsExtension {
    service: Arc<AnalyticsService>,
}
// configure(): 注册 FileAnalyticsSink 到 ctx.workspace
```

**4b. AuthExtension**
```rust  
pub struct AuthExtension {
    oauth_config: OAuthConfig,
}
// tools(): authenticate tool
// configure(): 加载 token store
```

**4c. LspExtension**
```rust
pub struct LspExtension;
// tools(): query_diagnostics
// configure(): 发现 workspace 中的 LSP 服务器
```

**4d. EvaluationExtension**
```rust
pub struct EvaluationExtension {
    contract: Option<SprintContract>,
    config: EvaluatorConfig,
}
// middleware(): SprintContractMiddleware（如果有 contract）
```

**Step 1:** 在 builtins.rs 添加 4 个 Extension

**Step 2:** `cargo check`

**Step 3:** Commit: `feat: add analytics/auth/lsp/evaluation extensions`

---

### Task 5: Builder 清理

**目标：** 清理 builder.rs 中被 Extension 替代的代码，更新 BaseAgent 结构体。

**Files:**
- Modify: `crates/alva-app-core/src/base_agent/builder.rs`
- Modify: `crates/alva-app-core/src/base_agent/agent.rs`

**变更：**
- builder.rs: 确认 skill_dirs、SkillStore 创建已在 Task 1 中删除
- agent.rs: skill_store 字段已变为 Option
- 删除 middleware_presets 模块（已被 ProductionExtension/GuardrailsExtension 替代）
- 确认所有旧路径都更新

**Step 1:** 删除 middleware_presets

**Step 2:** 清理 lib.rs 的 re-exports（如果有已移除的类型）

**Step 3:** `cargo check && cargo test`

**Step 4:** Commit: `refactor: clean up builder after extension migration`

---

### Task 6: 更新调用方

**目标：** 更新 CLI 和 eval 使用新的 Extension 模式。

**Files:**
- Modify: `crates/alva-app-cli/src/agent_setup.rs`
- Modify: `crates/alva-agent-eval/src/main.rs`

CLI agent_setup.rs 应变为：
```rust
let agent = BaseAgentBuilder::new()
    .workspace(workspace)
    .system_prompt(&system_prompt)
    .extension(Box::new(AllStandardExtension))
    .extension(Box::new(ProductionExtension))
    .extension(Box::new(SkillsExtension::new(vec![
        paths.project_skills_dir(),
        paths.global_skills_dir(),
    ])))
    .extension(Box::new(HooksExtension::new(settings.hooks)))
    .with_sub_agents()
    .sub_agent_max_depth(3)
    .build(model).await?;
```

**Step 1:** 更新 agent_setup.rs

**Step 2:** 更新 eval main.rs（如需要）

**Step 3:** `cargo check --workspace && cargo test -p alva-app-core`

**Step 4:** Commit: `refactor: update CLI and eval to use extension-based setup`

---

### Task 7: 最终验证

**Step 1:** `cargo check --workspace` — 全量编译

**Step 2:** `cargo test --workspace` — 全量测试

**Step 3:** 确认无 regression
