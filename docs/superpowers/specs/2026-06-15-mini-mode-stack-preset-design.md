# 迷你模式 / Stack Preset 机制设计

> 状态:设计已确认(方案 A:数据目录 + 档位标签),待写实现计划。
> 范围:子项目 ①「Preset 分档」。在注入重构(子项目 ②,已完成)之上构建。
> 日期:2026-06-15

---

## 1. 背景与目标

CLI(`alva-app-cli/src/agent_setup.rs` 的 `build_agent`)和 Tauri(`alva-app-tauri/src/agent.rs` 的 `ensure_agent`)各自**手抄**了一份 ~20 项的 plugin/middleware 装配列表,靠注释 "kept in lockstep" 保持同步。这是注入重构后留下的头号架构债(架构 review 标 CRITICAL):新增/调整一个能力要改两处,易漂移。

**目标**:
- 在 `alva-app-core` 提供**单一**装配真相 `build_stack(...)`,CLI 与 Tauri 都调它,删掉两份手抄。
- 提供分档 `StackProfile { Minimal, Standard, Full }`,实现用户要的「迷你模式」(Minimal:权限 + 增删改查搜)。
- 保留 Tauri 现有的**逐插件开关**能力(不回退),并能从同一数据自省渲染 GUI。

**非目标**:
- 不引入声明式宏 / inventory(方案 C)。
- 不改 Plugin/Registrar/Middleware 机制本身(子项目 ② 已定稿)。
- 不做 GUI 前端的视觉设计(本 spec 只定后端 + Tauri command 契约;前端选择器另行)。

---

## 2. MINIMAL 档边界(已确认)

「迷你模式」MINIMAL = 权限 + 增删改查搜 + 基础卫生:

| 来源 | 内容 |
|---|---|
| BaseAgentBuilder 自动装配 | `SecurityPlugin`(权限沙箱)、`SystemContextPlugin`(CLAUDE.md + git)、`MemoryPlugin`(InMemory) |
| Preset 加入(Plugin) | `CorePlugin`(read/create/edit/list/find/grep = 增改查搜)、`ShellPlugin`(删/通用,因无独立 delete 工具)、`SkillsPlugin`、`PermissionPlugin`、`ApprovalPlugin` |
| Preset 加入(Middleware) | `LoopDetectionMiddleware`、`DanglingToolCallMiddleware`、`ToolTimeoutMiddleware` |

- **STANDARD** = MINIMAL + `WebPlugin` / `TaskPlugin` / `TeamPlugin` / `McpPlugin` / `HooksPlugin` / `SubAgentPlugin` + `CompactionMiddleware` / `CheckpointMiddleware`。
- **FULL** = STANDARD + `BrowserPlugin` / `InteractionPlugin` / `PlanningPlugin` / `UtilityPlugin` / `AnalyticsPlugin`。

> 注:`ProviderRegistryPlugin` / `ToolLockRegistryPlugin` / `SpawnCommRegistryPlugin` / `BlackboardCommPlugin` 是基础设施/opt-in,按现状装配处理(provider-registry 当前 CLI 显式装,作为 always-on 基础设施纳入,不随 profile 变——见 §6)。

---

## 3. 核心类型(新模块 `crates/alva-app-core/src/stack/`)

放 app-core 层:它引用 `alva-agent-extension-builtin` 与 `alva-app-core::extension` 的具体 plugin/middleware;放 SDK(agent-core)会破 Rule 17。

```rust
/// 档位。Ord 派生使 `entry.min_profile <= profile` 即纳入。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StackProfile { Minimal, Standard, Full }

/// 可开关项的稳定标识(plugin + 卫生 middleware)。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackItemId {
    Core, Shell, Skills, Permission, Approval,              // MINIMAL plugins
    Web, Task, Team, Mcp, Hooks, SubAgents,                 // STANDARD plugins
    Browser, Interaction, Planning, Utility, Analytics,     // FULL plugins
    LoopDetection, DanglingToolCall, ToolTimeout,           // MINIMAL middleware
    Compaction, Checkpoint,                                 // STANDARD middleware
}

pub enum StackItemKind { Plugin, Middleware }

pub struct CatalogEntry {
    pub id: StackItemId,
    pub label: &'static str,        // GUI 显示名
    pub description: &'static str,  // GUI 描述
    pub category: &'static str,     // "tools" / "safety" / "context" / "infra" ...
    pub min_profile: StackProfile,  // 该项最低出现的档
    pub kind: StackItemKind,
}

/// 单一真相:每项 + 档位 + 元数据。
pub static STACK_CATALOG: &[CatalogEntry] = &[ /* 每个 StackItemId 一条 */ ];
```

### 3.1 装配上下文 `StackContext`

承载 parameterized plugin 需要的 harness 输入(从现 CLI/Tauri 装配处抽取):

```rust
pub struct StackContext {
    pub workspace: PathBuf,
    pub approval: ApprovalPlugin,        // 由 caller 预建(见 §5),build_stack 只是装入
    pub skills: SkillsConfig,            // project_skills_dir + bundled_dir
    pub mcp_config_paths: Vec<PathBuf>,
    pub subagent_depth: u32,             // 现为 3
    pub hooks_settings: HooksSettings,
    pub subprocess_ext_dirs: Vec<PathBuf>, // loader(若纳入)
    // ... 按真实需要补
}
```

### 3.2 覆盖 `StackOverrides`

GUI 逐插件开关:在 profile 默认之上 enable/disable 单项。

```rust
pub struct StackOverrides(pub HashMap<StackItemId, bool>);
impl StackOverrides { pub fn enabled(&self, id, default: bool) -> bool { ... } }
```

---

## 4. 装配函数 + 自省

```rust
/// 唯一装配真相。CLI/Tauri 都调它,删两份手抄。
pub fn build_stack(
    mut builder: BaseAgentBuilder,
    profile: StackProfile,
    overrides: &StackOverrides,
    ctx: &StackContext,
) -> BaseAgentBuilder {
    for entry in STACK_CATALOG {
        let default_on = entry.min_profile <= profile;
        if !overrides.enabled(entry.id, default_on) { continue; }
        match construct(entry, ctx) {
            Some(StackItem::Plugin(p))     => builder = builder.plugin(p),
            Some(StackItem::Middleware(m)) => builder = builder.middleware(m),
            None => tracing::info!(?entry.id, "stack item unavailable (feature-gated); skipped"),
        }
    }
    builder
}

/// 按 id 构造(parameterized 从 ctx 取参)。feature-gated 不可用返 None。
fn construct(entry: &CatalogEntry, ctx: &StackContext) -> Option<StackItem> {
    Some(match entry.id {
        StackItemId::Core  => StackItem::Plugin(Box::new(CorePlugin)),
        StackItemId::Shell => StackItem::Plugin(Box::new(ShellPlugin)),
        StackItemId::Permission => StackItem::Plugin(Box::new(PermissionPlugin::new())),
        StackItemId::Approval   => StackItem::Plugin(Box::new(ctx.approval.clone())),
        StackItemId::SubAgents  => StackItem::Plugin(Box::new(SubAgentPlugin::new(ctx.subagent_depth))),
        StackItemId::Mcp        => StackItem::Plugin(Box::new(McpPlugin::new(ctx.mcp_config_paths.clone()))),
        StackItemId::LoopDetection => StackItem::Middleware(Arc::new(LoopDetectionMiddleware::new())),
        // ... 每个 id 一条;参数从 ctx 取
        #[cfg(feature = "browser")]
        StackItemId::Browser => StackItem::Plugin(Box::new(BrowserPlugin)),
        #[cfg(not(feature = "browser"))]
        StackItemId::Browser => return None,
        // ...
    })
}

/// GUI 自省:某 profile 下每项 + 默认开关。
pub fn catalog_for_profile(profile: StackProfile) -> Vec<StackItemView> {
    STACK_CATALOG.iter().map(|e| StackItemView {
        id: e.id, label: e.label, description: e.description,
        category: e.category, enabled_default: e.min_profile <= profile,
    }).collect()
}
```

> `construct` 的 match 是这套机制唯一的 "switchboard"——但从**两份 app 手抄收敛到一处**,且与 `STACK_CATALOG`(数据)分离:catalog 管"有哪些项、属哪档、怎么显示",construct 管"怎么造"。新增能力 = catalog 加一条 + construct 加一臂。

---

## 5. Approval 副输出处理(已确认)

`ApprovalPlugin::with_channel()` 产出 `(plugin, rx)`,`rx` 是 harness 要消费的审批流。**caller 先建**:

```rust
let (approval, approval_rx) = ApprovalPlugin::with_channel();
let ctx = StackContext { approval, /* ... */ };
let builder = build_stack(BaseAgentBuilder::new(), profile, &overrides, &ctx);
// caller 自己持有 approval_rx 接审批
```

→ `build_stack` 保持统一(construct 里 `ctx.approval.clone()` 装入);副输出由 caller 管。`ApprovalPlugin` 需 `Clone`(内部 channel sender 可 clone)——实现时确认/补。

---

## 6. CLI / Tauri 接入

### CLI(`agent_setup.rs::build_agent`)
- 删 ~20 行 `.plugin()/.middleware()`。
- 默认 `StackProfile::Full`(**行为不变**),`build_stack(b, profile, StackOverrides::none(), &ctx)`。
- 新增 CLI flag `--profile minimal|standard|full`(默认 full),让 `pi --profile minimal` 进迷你模式。

### Tauri(`agent.rs::ensure_agent`)
- 删 ~120 行 `if on(name) { builder = ... }`。
- profile 来自 session 设置(默认 Full,与现状一致);session 的现有 `plugin_config`(HashMap<name,bool>)映射成 `StackOverrides`。
- `build_stack(b, profile, &overrides, &ctx)`。
- Tauri command:`list_stack_items(profile) -> Vec<StackItemView>`(取代现 `list_plugins()`/`default_plugin_state()`),GUI 渲染 profile 选择器 + 逐项开关。
- **不回退**现有逐插件开关:override 机制承接 `plugin_config`。

### 基础设施项(消歧义:纳入 catalog,标 infra)
`ProviderRegistryPlugin` / `ToolLockRegistryPlugin` / `SpawnCommRegistryPlugin` / `BlackboardCommPlugin` / `SubprocessLoaderPlugin` 这类 always-on 基础设施 / opt-in:**纳入 `STACK_CATALOG`**,`min_profile = Minimal`、`category = "infra"`。`build_stack` 照常装(单一真相);GUI 的 `list_stack_items` 标记 `category="infra"`,前端**过滤掉 infra 类不显示开关**(用户不该手动关基础设施)。`StackItemId` enum 需补齐这几个。

### 完整性要求(关键,防迁移丢项)
`STACK_CATALOG` **必须覆盖当前 CLI `build_agent` + Tauri `ensure_agent` 注册的每一项**——实现计划阶段先把两份现有列表逐项列出(plugin + middleware + infra + loader),逐一映射到 catalog 条目并定 `min_profile`,确保迁移**不静默丢任何一项**。迁移后 CLI 默认 `Full` 产出的 item 集合应与重构前 CLI 注册集合**一致**(用测试断言)。

---

## 7. 测试

- `build_stack` 单测:三个 profile 各自产出的 `StackItemId` 集合**精确**符合 §2(用一个不真造 plugin 的"dry-run"变体或断言 builder 装入的 plugin name 集)。
- overrides:Minimal + override{Web:true} → 含 Web;Full + override{Browser:false} → 不含 Browser。
- `catalog_for_profile(Minimal)` 的 enabled_default 集合 = MINIMAL 项。
- feature-gated(browser 关)→ construct 返 None,build_stack 跳过不 panic。
- CLI `--profile minimal` 端到端:agent 只含 MINIMAL 能力(集成测试或 dry-run)。

---

## 8. 收益

- CLI/Tauri 两份手抄 → 一处 `STACK_CATALOG` + `build_stack`,漂移债清除。
- 「迷你模式」`StackProfile::Minimal` 可用(CLI `--profile minimal` / GUI 选择器)。
- GUI 逐插件开关从同一数据自省,不回退。
- 新增能力只改一处(catalog + construct)。
- 是后续声明式 `capability!`(方案 C)的天然落点。
