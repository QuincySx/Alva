# 组件开关(扁平 toggle)设计

> 状态:设计已确认,从 Stage A 开始实现。
> 范围:子项目 ①「组件装配收敛」——但落成**扁平逐组件 on/off**,**不做**档位(Minimal/Standard/Full)框架(那版已弃用,见 `2026-06-15-mini-mode-stack-preset-design.md`)。
> 日期:2026-06-15

---

## 1. 背景与目标

mini-mode 增量加回(P1-P4)已证明组件可一项项装、每步可测。现在要的不是"永远最小",而是:**所有组件都可用,但每个能单独开/关**。

**为什么要开关(核心动机,非洁癖)**:
- **工具集大小直接影响模型准确度**:工具越多 → 模型工具选择出错/混淆/幻觉调用概率越高、tool schema 吃 token 稀释注意力;弱模型尤甚。控制工具集 = 控制 agent 质量。
- **能 A/B 测**:配同一批任务,"只 Core+Shell" vs "全开" 各跑真模型,用回归 harness 量化准确度差异,数据驱动决定带哪些。
- **顺带消债**:CLI `build_agent` 和 Tauri `ensure_agent` 当前各手抄一份装配列表;统一到一份清单。

**非目标**:
- 不做档位 Profile(Minimal/Standard/Full)。就是扁平 `{id: bool}`。
- 不改 Plugin/Registrar/Middleware 机制(已定稿)。

---

## 2. 设计

### 2.1 核心(app-core 新模块 `crates/alva-app-core/src/components.rs`)

```rust
pub enum ComponentKind { Plugin, Middleware }

/// 一个可开关的 agent 组件的元数据(展示 + 默认开关)。
pub struct ComponentMeta {
    pub id: &'static str,           // "shell" / "web" / "task" / "browser" ...
    pub label: &'static str,        // UI 显示名
    pub description: &'static str,  // UI 描述
    pub category: &'static str,     // "tools" / "safety" / "context" / "collab" / "infra" ...
    pub default_on: bool,
    pub kind: ComponentKind,
}

/// 单一真相:全部可开关组件。substrate(approval/checkpoint 接线 + 自动
/// memory/security/system_context)永远在,不进此表。
pub static COMPONENTS: &[ComponentMeta] = &[ /* 见 §2.3 */ ];

/// 开关:id -> enabled,覆盖 default_on。缺省 = default_on。
pub type ComponentToggles = std::collections::HashMap<String, bool>;
pub fn is_on(toggles: &ComponentToggles, meta: &ComponentMeta) -> bool {
    *toggles.get(meta.id).unwrap_or(&meta.default_on)
}

/// 构造每个组件需要的 harness 输入(parameterized 组件从这里取参)。
pub struct ComponentContext {
    pub workspace: PathBuf,
    pub provider_registry: Option<Arc<ProviderRegistry>>, // ProviderRegistry/SubAgent 用
    pub skills: Option<(PathBuf, Option<PathBuf>)>,        // SkillsPlugin 的 (primary, bundled)
    pub mcp_config_paths: Vec<PathBuf>,
    pub subagent_depth: u32,
    pub hooks_settings: HooksSettings,
    pub subprocess_ext_dirs: Vec<PathBuf>,
    // 按真实需要补
}

/// 把开着的组件装到 builder 上。唯一装配真相,CLI/Tauri/测试都调它。
pub fn apply_components(
    mut b: BaseAgentBuilder,
    toggles: &ComponentToggles,
    ctx: &ComponentContext,
) -> BaseAgentBuilder {
    for meta in COMPONENTS {
        if !is_on(toggles, meta) { continue; }
        b = construct_and_attach(b, meta.id, ctx); // match id → .plugin()/.middleware();参数缺失(如无 provider_registry)则 skip+log
    }
    b
}
```

> `construct_and_attach` 的 `match id` 是这套机制唯一的 switchboard——但从**两份 app 手抄收敛到一处**,且与 `COMPONENTS`(数据)分离:数据管"有哪些、怎么显示、默认开关",match 管"怎么造"。新增组件 = 表加一条 + match 加一臂。

### 2.2 配置

`~/.alva/config.json`(CLI 与 Tauri 已共用此文件)加一段:
```jsonc
{ "components": { "browser": false, "analytics": false } }  // 缺省 = default_on
```
读成 `ComponentToggles`。Tauri 现有 session 级 `plugin_config` 映射进来(session 覆盖 > 全局 config > default_on)。

### 2.3 组件清单(初版,default_on 待定/见注)

| id | 来源 | default_on | 说明 |
|---|---|---|---|
| core | CorePlugin | ✅ | 文件增改查搜 |
| shell | ShellPlugin | ✅ | execute_shell |
| loop-detection / dangling-tool-call / tool-timeout | builtins mw | ✅ | 卫生 |
| permission | PermissionPlugin | ✅ | HITL/plan |
| compaction | CompactionMiddleware | ✅ | 长会话压缩 |
| skills | SkillsPlugin | ✅ | 渐进式 skill |
| web | WebPlugin | ✅ | internet_search + read_url |
| provider-registry / tool-lock | infra plugin | ✅ | infra |
| task / team | Task/TeamPlugin | ✅ | 协作工具 |
| sub-agents | SubAgentPlugin | ✅ | spawn 子 agent |
| mcp | McpPlugin | ⬜(看你用不用) | MCP 服务器,动态加一堆工具,对准确度影响最大 |
| hooks | HooksPlugin | ✅ | 用户钩子(不加工具) |
| checkpoint | CheckpointMiddleware | ✅ | 自动存档(不加工具) |
| subprocess-loader | SubprocessLoaderPlugin | ⬜ | 第三方 AEP 加载器 |
| interaction | InteractionPlugin | ✅ | ask_human |
| planning / utility | Planning/UtilityPlugin | ⬜ | 可能与 shell/core 重叠 |
| analytics | AnalyticsPlugin | ⬜ | 埋点(不加工具) |
| browser | BrowserPlugin | ⬜ | chromiumoxide 重依赖 |

> default_on 是初值,Stage A 实现时按"刚需 + 不重叠"定;用户可随时在 config / Tauri UI 改。

---

## 3. 四处接入

- **CLI** `agent_setup.rs::build_agent`:删手写链,`apply_components(builder, toggles, ctx)`;substrate(approval/checkpoint)仍在 apply 之外手动接(它要 approval_rx 副输出)。
- **Tauri** `agent.rs::ensure_agent`:同样;`plugin_config` → ComponentToggles;`list_plugins()` → 由 `COMPONENTS` 派生。
- **测试 harness** `agent_capabilities.rs`:`build_mini_agent` 改成能传 `ComponentToggles`,默认子集 = 当前镜像;新增能力可按子集 build → A/B 准确度。
- **Tauri 设置页 UI**:Tauri command 返回 `COMPONENTS`(id/label/desc/category/default_on)+ 当前 toggles;前端逐组件 on/off 开关,存回 config。

---

## 4. 分阶段实现(每步编译+测试验证)

- **Stage A**:`components.rs`——`ComponentMeta` + `COMPONENTS` 全表 + `ComponentContext` + `ComponentToggles` + `apply_components`(含 construct switchboard)。单测:全开 toggles 装出的 plugin/middleware 集合 = 预期全集;关某项后不含它;缺 provider_registry 时 sub-agents/provider-registry 优雅 skip。**不接 CLI/Tauri**,纯新增 + 单测。
- **Stage B**:CLI `build_agent` 改用 `apply_components`(默认全开 ≈ 现状)。验证:CLI 编译;agent_capabilities mock 7/7 + 注册断言仍过;可用 config/env 关组件。
- **Stage C**:Tauri `ensure_agent` 改用 `apply_components` + `plugin_config` 映射;`list_plugins()` 由 COMPONENTS 派生。验证:Tauri 编译;两边装配收敛到一份。
- **Stage D**:Tauri 设置页 UI——逐组件开关 + 存 config。
- **Stage E**:测试 harness 支持任意 `toggles` build;加"工具集大小 vs 准确度"A/B real 用例(可选)。

---

## 5. 收益

- 全部组件可用,但**按需开关**,控制工具集大小 = 控 agent 准确度/token。
- CLI/Tauri 装配统一到 `COMPONENTS` + `apply_components`,消两边手抄债。
- 测试能 A/B 不同配置 × 不同模型,数据驱动选型。
- Tauri 用户在设置页直接开关。
