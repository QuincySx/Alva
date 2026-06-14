# 注入机制统一（Plugin + Middleware）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 agent 的注入机制从「Tool + Extension + Middleware + ExtensionEvent 四概念」收敛到「Tool + Plugin + Middleware 三概念」：删冗余 Event 层、单点 middleware 免壳、Extension 双阶段双句柄合并为 `Plugin::register` 单句柄。

**Architecture:** 采用「additive → migrate → delete」三段式，保证**每个 commit 都能编译 + 测试通过**。先加 `Plugin`/`Registrar`/`LateContext` 并用 `ExtensionAsPlugin` 适配器让旧 `Extension` 继续工作；再逐组迁移 ~30 个实现；然后砍 Event 层 + 删空壳；最后移除旧 `Extension` trait 并重命名。`register()` 遵守 provide-only（只提供能力、不读别家），跨插件晚期读放 `finalize()`。

**Tech Stack:** Rust workspace（30 crates）、`async_trait`、typed Bus（`alva-kernel-bus`）、`MiddlewareStack`（`alva-kernel-core`）。验证：`cargo check`/`cargo test` + `cargo check --target wasm32-unknown-unknown` + `scripts/ci-check-deps.sh`（Rule 17）。

**Spec:** `docs/superpowers/specs/2026-06-14-plugin-middleware-unification-design.md`

---

## 文件结构（改动地图）

**新增**
- `crates/alva-agent-core/src/extension/plugin.rs` — `Plugin` trait
- `crates/alva-agent-core/src/extension/registrar.rs` — `Registrar` + `LateContext`
- `crates/alva-agent-core/src/extension/adapter.rs` — `ExtensionAsPlugin`（过渡期，Phase 6 删）
- `crates/alva-app-extension-loader/src/aep_bridge.rs` — `AepBridgeMiddleware`

**重写**
- `crates/alva-agent-core/src/agent_builder.rs` — `build()` 装配流程（step 4–10 → 单 register + finalize）
- `crates/alva-agent-core/src/extension/mod.rs` — 导出新类型
- ~30 个 `impl Extension` → `impl Plugin`（builtin wrappers + app-core extension/*）
- `crates/alva-app-cli/src/agent_setup.rs` + `crates/alva-app-tauri/src/agent.rs` — 装配列表（`.extension`→`.plugin` / 裸 `.middleware`）

**删除**
- `crates/alva-agent-core/src/extension/events.rs`（`ExtensionEvent`/`EventResult`）
- `ExtensionBridgeMiddleware`（`bridge.rs` 事件分发）
- `HostAPI::on`/`on_as` + `ExtensionHost` handler map + `emit()`
- 7 个纯中间件 wrapper 类型
- `Extension` trait + `ExtensionContext`/`FinalizeContext` + `ExtensionAsPlugin`（Phase 6）

---

## 验证基线（每个 Phase 末尾跑）

- `cargo check --workspace` — 全绿
- `cargo test -p alva-agent-core` — 核心装配测试
- 涉及 wasm 的 crate：`cargo check -p alva-host-wasm --target wasm32-unknown-unknown`
- Phase 4/5/6/7 末尾：`cargo test --workspace` + `bash scripts/ci-check-deps.sh`

---

# Phase 1 — 新核心 API（additive，旧 Extension 经适配器照常工作）

### Task 1: 新增 `Registrar` + `LateContext`

> ✅ 已完成（commit `01ba341` + `ad725f7`）。最终实现去掉了 `'a`/`PhantomData`（字段全 owned/Arc，无借用），`take_tools` 为 `pub(crate)`，pub 方法补了 doc。下方代码块为原始草案，以仓库实际代码为准。

**Files:**
- Create: `crates/alva-agent-core/src/extension/registrar.rs`
- Modify: `crates/alva-agent-core/src/extension/mod.rs`

- [ ] **Step 1: 写 `registrar.rs`**

```rust
//! Registrar — 单一跨层装配句柄（取代 HostAPI + ExtensionContext）。
//! Plugin::register() 通过它注册 tools / middleware / bus 服务 / system prompt / command。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use async_trait::async_trait;
use alva_kernel_abi::{BusHandle, BusWriter, LanguageModel};
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::scope::context::ContextLayer;
use alva_kernel_core::middleware::Middleware;
use super::host::{ExtensionHost, RegisteredCommand};

/// 单一装配句柄。内部对 ExtensionHost / bus_writer 内部可变，方法均 `&self`。
pub struct Registrar<'a> {
    host: Arc<RwLock<ExtensionHost>>,
    plugin_name: String,
    bus: BusHandle,
    bus_writer: BusWriter,
    workspace: PathBuf,
    /// register() 期间收集的 tool；build() 在 register 后 drain。
    tools: Mutex<Vec<Box<dyn Tool>>>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Registrar<'a> {
    pub fn new(
        host: Arc<RwLock<ExtensionHost>>,
        plugin_name: String,
        bus: BusHandle,
        bus_writer: BusWriter,
        workspace: PathBuf,
    ) -> Self {
        Self {
            host, plugin_name, bus, bus_writer, workspace,
            tools: Mutex::new(Vec::new()),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn tool(&self, t: Box<dyn Tool>) {
        self.tools.lock().unwrap().push(t);
    }
    pub fn tools(&self, ts: Vec<Box<dyn Tool>>) {
        self.tools.lock().unwrap().extend(ts);
    }
    pub fn middleware(&self, mw: Arc<dyn Middleware>) {
        self.host.write().unwrap().register_middleware(mw);
    }
    pub fn provide<T: Send + Sync + ?Sized + 'static>(&self, value: Arc<T>) {
        self.bus_writer.provide(value);
    }
    pub fn system_prompt(&self, layer: ContextLayer, text: impl Into<String>) {
        self.host.write().unwrap()
            .append_system_prompt(self.plugin_name.clone(), layer, text.into());
    }
    pub fn command(&self, name: &str, description: &str) {
        self.host.write().unwrap().register_command(RegisteredCommand {
            name: name.to_string(),
            description: description.to_string(),
            source_extension: self.plugin_name.clone(),
        });
    }

    pub fn workspace(&self) -> &Path { &self.workspace }
    pub fn bus(&self) -> &BusHandle { &self.bus }
    pub fn bus_writer(&self) -> &BusWriter { &self.bus_writer }
    pub fn plugin_name(&self) -> &str { &self.plugin_name }

    /// build() 在 register() 跑完后取走收集到的 tool。
    pub fn take_tools(&self) -> Vec<Box<dyn Tool>> {
        std::mem::take(&mut self.tools.lock().unwrap())
    }
}

/// 晚期上下文——所有 plugin register() 跑完、model + 完整 tool 已知后传给 finalize()。
pub struct LateContext {
    pub bus: BusHandle,
    pub bus_writer: BusWriter,
    pub workspace: PathBuf,
    pub model: Arc<dyn LanguageModel>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub max_iterations: u32,
}
```

- [ ] **Step 2: 在 `mod.rs` 导出**

在 `crates/alva-agent-core/src/extension/mod.rs` 加：
```rust
mod registrar;
pub use registrar::{Registrar, LateContext};
```
（保留现有 `mod host; mod context; ...` 不动。）

- [ ] **Step 3: 编译验证**

Run: `cargo check -p alva-agent-core`
Expected: PASS（新类型暂无消费者，应无警告失败；如有 dead_code 警告允许，下一 Task 即用）。

- [ ] **Step 4: Commit**

```bash
git add crates/alva-agent-core/src/extension/registrar.rs crates/alva-agent-core/src/extension/mod.rs
git commit -m "feat(agent-core): add Registrar + LateContext (single setup handle)"
```

---

### Task 2: 新增 `Plugin` trait + `ExtensionAsPlugin` 适配器

**Files:**
- Create: `crates/alva-agent-core/src/extension/plugin.rs`
- Create: `crates/alva-agent-core/src/extension/adapter.rs`
- Modify: `crates/alva-agent-core/src/extension/mod.rs`

- [ ] **Step 1: 写 `plugin.rs`**

```rust
//! Plugin — 装配期跨层捆绑包（取代 Extension）。
use async_trait::async_trait;
use std::sync::Arc;
use alva_kernel_abi::tool::Tool;
use super::registrar::{Registrar, LateContext};

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }

    /// 唯一装配阶段：注册 tools / middleware / bus 服务 / system prompt / command。
    /// provide-only：只提供能力，不读别家 plugin 提供的 bus 能力（要读放 finalize）。
    async fn register(&self, r: &Registrar);

    /// 可选晚期钩子：动态 tool 发现 + 跨插件晚期接线（读别家在 register 提供的能力）。
    async fn finalize(&self, _cx: &LateContext) -> Vec<Arc<dyn Tool>> { vec![] }
}
```

- [ ] **Step 2: 写 `adapter.rs`（过渡期桥接旧 Extension）**

```rust
//! ExtensionAsPlugin — 过渡期适配器：把旧 `Extension` 当 `Plugin` 跑。
//! Phase 6 所有实现迁移完成后连同 Extension trait 一起删除。
use async_trait::async_trait;
use std::sync::Arc;
use alva_kernel_abi::tool::Tool;
use super::{Extension, HostAPI, ExtensionContext, FinalizeContext};
use super::registrar::{Registrar, LateContext};

pub struct ExtensionAsPlugin(pub Box<dyn Extension>);

#[async_trait]
impl super::plugin::Plugin for ExtensionAsPlugin {
    fn name(&self) -> &str { self.0.name() }
    fn description(&self) -> &str { self.0.description() }

    async fn register(&self, r: &Registrar) {
        // 复刻旧顺序：tools() → activate(HostAPI) → configure(ExtensionContext)。
        r.tools(self.0.tools().await);
        let api = HostAPI::new(r.host_arc(), self.0.name().to_string());
        self.0.activate(&api);
        let ctx = ExtensionContext {
            bus: r.bus().clone(),
            bus_writer: r.bus_writer().clone(),
            workspace: r.workspace().to_path_buf(),
            tool_names: Vec::new(), // 死字段，无人读（见 spec review）
        };
        self.0.configure(&ctx).await;
    }

    async fn finalize(&self, cx: &LateContext) -> Vec<Arc<dyn Tool>> {
        let fctx = FinalizeContext {
            bus: cx.bus.clone(),
            bus_writer: cx.bus_writer.clone(),
            workspace: cx.workspace.clone(),
            model: cx.model.clone(),
            tools: cx.tools.clone(),
            max_iterations: cx.max_iterations,
        };
        self.0.finalize(&fctx).await
    }
}
```

- [ ] **Step 3: 给 Registrar 加 `host_arc()`（适配器要拿 host clone 构造 HostAPI）**

在 `registrar.rs` 的 `impl Registrar` 里加：
```rust
    pub fn host_arc(&self) -> std::sync::Arc<std::sync::RwLock<super::host::ExtensionHost>> {
        self.host.clone()
    }
```

- [ ] **Step 4: 在 `mod.rs` 导出**

```rust
mod plugin;
mod adapter;
pub use plugin::Plugin;
pub use adapter::ExtensionAsPlugin;
```

- [ ] **Step 5: 编译验证**

Run: `cargo check -p alva-agent-core`
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add crates/alva-agent-core/src/extension/plugin.rs crates/alva-agent-core/src/extension/adapter.rs crates/alva-agent-core/src/extension/registrar.rs crates/alva-agent-core/src/extension/mod.rs
git commit -m "feat(agent-core): add Plugin trait + ExtensionAsPlugin transition adapter"
```

---

### Task 3: 让 `AgentBuilder` 驱动 Plugin（行为不变，旧 Extension 经适配器跑）

**Files:**
- Modify: `crates/alva-agent-core/src/agent_builder.rs`（字段 + `.extension()`/`.plugin()` + `build()` step 4–10）

- [ ] **Step 1: 改 builder 字段与注册方法**

把 `extensions: Vec<Box<dyn Extension>>` 改为 `plugins: Vec<Box<dyn Plugin>>`。
`.extension()`（agent_builder.rs:92）改为包装：
```rust
    pub fn extension(mut self, e: Box<dyn Extension>) -> Self {
        self.plugins.push(Box::new(ExtensionAsPlugin(e)));
        self
    }
    pub fn plugin(mut self, p: Box<dyn Plugin>) -> Self {
        self.plugins.push(p);
        self
    }
```
（保留 `.middleware()` agent_builder.rs:100 不动。）

- [ ] **Step 2: 重写 `build()` step 4–10 为单 register + finalize**

替换 agent_builder.rs:158–243 区段为：
```rust
        // 4. Register phase: 每个 plugin 一次性注册 tools/middleware/bus/prompt/command。
        //    provide-only：register() 不读别家 plugin 的 bus 能力，故顺序无关。
        let workspace_for_ctx = self.workspace.clone().unwrap_or_default();
        let mut all_tools: Vec<Box<dyn Tool>> = Vec::new();
        for p in &self.plugins {
            let reg = Registrar::new(
                host.clone(),
                p.name().to_string(),
                bus.clone(),
                bus_writer.clone(),
                workspace_for_ctx.clone(),
            );
            p.register(&reg).await;
            all_tools.extend(reg.take_tools());
        }
        all_tools.extend(self.extra_tools);

        // 5. Build middleware stack: plugin-registered + 裸 extra，统一 priority 排序。
        let mut middleware_stack = MiddlewareStack::new();
        {
            let mut host_mut = host.write().unwrap();
            for mw in host_mut.take_middlewares() {
                middleware_stack.push_sorted(mw);
            }
        }
        for mw in self.extra_middleware {
            middleware_stack.push_sorted(mw);
        }
        // 注意：ExtensionBridgeMiddleware 仍在此推入；Phase 4 删除事件层时一并移除。
        middleware_stack.push_sorted(Arc::new(ExtensionBridgeMiddleware::new(host.clone())));

        // 6. 注册 register 阶段产出的 tool。
        let mut registry = ToolRegistry::new();
        for tool in all_tools {
            registry.register(tool);
        }

        // 7. configure middleware shared infra（不变）。
        middleware_stack.configure_all(&MiddlewareContext {
            bus: Some(bus.clone()),
            workspace: self.workspace.clone(),
            session: None,
        });

        // 8. Finalize phase：晚期 tool 发现 + 跨插件接线。
        let late_ctx = LateContext {
            bus: bus.clone(),
            bus_writer: bus_writer.clone(),
            workspace: workspace_for_ctx,
            model: model.clone(),
            tools: registry.list_arc(),
            max_iterations: self.max_iterations,
        };
        for p in &self.plugins {
            for tool in p.finalize(&late_ctx).await {
                registry.register_arc(tool);
            }
        }
        let tools_arc: Vec<Arc<dyn Tool>> = registry.list_arc();
```

- [ ] **Step 3: 修 import**

agent_builder.rs 顶部 use 加 `Registrar, LateContext, Plugin, ExtensionAsPlugin`；保留 `ExtensionBridgeMiddleware`（Phase 4 删）。删除不再用的 `HostAPI`（若仅 build 用过）/ `ExtensionContext` / `FinalizeContext` import（适配器仍用，故 crate 内仍存在）。

- [ ] **Step 4: 全量编译 + 测试**

Run: `cargo check -p alva-agent-core && cargo test -p alva-agent-core`
Expected: PASS。所有现有 extension 经 `ExtensionAsPlugin` 跑，行为应与改前一致。
若 `base_agent_overrides.rs` 等测试通过，证明 default-replacement / 装配未回归。

- [ ] **Step 5: 下游编译**

Run: `cargo check -p alva-app-core -p alva-app-cli`
Expected: PASS（`.extension()` 签名未变，调用点不动）。

- [ ] **Step 6: Commit**

```bash
git add crates/alva-agent-core/src/agent_builder.rs
git commit -m "refactor(agent-core): build() drives Plugin via single register()+finalize()

旧 Extension 经 ExtensionAsPlugin 适配器跑，行为不变。activate+configure
两阶段合并为 register() 单阶段；tool_names 死字段不再传递。"
```

---

### Task 3b: `BaseAgentBuilder` 改用 Plugin 存储（保留默认替换契约）

**Files:** `crates/alva-app-core/src/base_agent/builder.rs`

> 必须在 Task 5/6 之前做：BaseAgentBuilder 自动装配的默认 Memory/Security/SystemContext
> 一旦迁成 Plugin，`.extension(Box::new(..))` 会编译失败。

- [ ] **Step 1: 字段 + 方法**

把内部 `extensions: Vec<Box<dyn Extension>>` 改为 `plugins: Vec<Box<dyn Plugin>>`。
`.extension()`（builder.rs:84）包装、新增 `.plugin()`：
```rust
    pub fn extension(mut self, ext: Box<dyn Extension>) -> Self {
        self.plugins.push(Box::new(ExtensionAsPlugin(ext)));
        self
    }
    pub fn plugin(mut self, p: Box<dyn Plugin>) -> Self {
        self.plugins.push(p);
        self
    }
```

- [ ] **Step 2: skip-by-name 改在 plugin 列表上判断**

builder.rs:185–215 的三段 `self.extensions.iter().any(|e| e.name() == "memory")` 改为
`self.plugins.iter().any(|p| p.name() == "memory")`（security / system_context 同理）。
默认插入 `self.plugins.insert(0, Box::new(ExtensionAsPlugin(Box::new(MemoryExtension::default()))))`
（此刻 Memory 等仍是 Extension，用适配器包；Task 5/6 迁完后这三行改成直接 `Box::new(XxxPlugin)`）。

- [ ] **Step 3: 传给内层 AgentBuilder**

build()（builder.rs:230 附近）`for ext in self.extensions { agent_builder.extension(ext) }`
改为 `for p in self.plugins { agent_builder = agent_builder.plugin(p) }`。

- [ ] **Step 4: 编译 + 测试**

Run: `cargo check -p alva-app-core && cargo test -p alva-app-core`
Expected: PASS（`base_agent_overrides.rs` 同名替换测试须绿，证明默认替换契约保留）。

- [ ] **Step 5: Commit**

```bash
git add crates/alva-app-core/src/base_agent/builder.rs
git commit -m "refactor(app-core): BaseAgentBuilder stores Plugin; keep skip-by-name default-replacement"
```

---

# Phase 2 — 迁移 builtin extensions（按类别分 commit）

> 迁移模式：把 `impl Extension for X` 改成 `impl Plugin for X`，**同一 commit 删除旧 impl**
> （`ExtensionAsPlugin` 覆盖未迁移的，已迁移的不再 impl Extension，无 coherence 冲突）。
> 装配调用点 `.extension(Box::new(X))` 暂不动——`.extension()` 仍接受 `Box<dyn Extension>`，
> 而迁移后的 X 不再是 Extension。**因此迁移一个类型时，其调用点要同步改成 `.plugin(Box::new(X))`。**

### Task 4: 6 个 tool-group wrapper → Plugin

**Files（每个一处 impl）:**
- `crates/alva-agent-extension-builtin/src/wrappers/core.rs`
- `.../wrappers/shell.rs` / `interaction.rs` / `web.rs` / `utility.rs` / `browser.rs`
- 调用点：`crates/alva-app-cli/src/agent_setup.rs`、`crates/alva-app-tauri/src/agent.rs`

- [ ] **Step 1: 改写模板（以 CoreExtension 为例）**

`core.rs` 旧：
```rust
#[async_trait]
impl Extension for CoreExtension {
    fn name(&self) -> &str { "core" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { core_tools() }
}
```
改为：
```rust
#[async_trait]
impl Plugin for CoreExtension {
    fn name(&self) -> &str { "core" }
    async fn register(&self, r: &Registrar) { r.tools(core_tools()); }
}
```
import：`use alva_agent_core::{Plugin, Registrar};`（替换 `Extension`）。

- [ ] **Step 2: 对其余 5 个套用同模板**

| 文件 | name() | register 内容 |
|---|---|---|
| `shell.rs` | `"shell"` | `r.tools(shell_tools());` |
| `interaction.rs` | `"interaction"` | `r.tools(interaction_tools());` |
| `web.rs` | `"web"` | `r.tools(web_tools());` |
| `utility.rs` | `"utility"` | `r.tools(utility_tools());` |
| `browser.rs` | `"browser"` | `r.tools(browser_tools());` |

> 实现时先 `grep -n "fn name" wrappers/<f>.rs` 确认每个真实 name() 字面量与 preset 函数名，
> 按真实值替换上表（上表为预期值，以源码为准）。

- [ ] **Step 3: 改调用点**

在 `agent_setup.rs` 和 `agent.rs` 把这 6 个的 `.extension(Box::new(CoreExtension))` 等改为
`.plugin(Box::new(CoreExtension))`（共 6 处 × 2 文件）。

- [ ] **Step 4: 编译 + 测试**

Run: `cargo check -p alva-agent-extension-builtin -p alva-app-cli -p alva-app-tauri && cargo test -p alva-agent-extension-builtin`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/alva-agent-extension-builtin/src/wrappers/{core,shell,interaction,web,utility,browser}.rs crates/alva-app-cli/src/agent_setup.rs crates/alva-app-tauri/src/agent.rs
git commit -m "refactor(builtin): migrate 6 tool-group wrappers Extension->Plugin"
```

---

### Task 5: bus-publish wrapper → Plugin（7 个）

**Files:**
- builtin: `wrappers/task.rs` / `team.rs` / `memory.rs`
- app-core: `extension/tool_lock_registry.rs` / `provider_registry.rs` / `spawn_comm_registry.rs` / `approval.rs`
- 调用点：`agent_setup.rs`、`agent.rs`

- [ ] **Step 1: 改写模板（以 MemoryExtension 为例）**

旧 `configure` 里 `ctx.bus_writer.provide(Arc::new(self.service.clone()))` →
```rust
#[async_trait]
impl Plugin for MemoryExtension {
    fn name(&self) -> &str { "memory" }
    async fn register(&self, r: &Registrar) {
        r.tools(memory_tools());                       // 若该 wrapper 有 tools()
        r.provide::<dyn MemoryBackend>(self.backend.clone()); // 按真实服务类型/字段
    }
}
```

- [ ] **Step 2: 逐个套用**

| 文件 | name() | provide 的服务（以源码为准） | 是否带 tools |
|---|---|---|---|
| `task.rs` | `"task"` | `TaskService` | 是 |
| `team.rs` | `"team"` | `TeamService` | 是 |
| `memory.rs` | `"memory"` | `MemoryService`/`MemoryBackend` | 是 |
| `tool_lock_registry.rs` | `"tool-lock-registry"` | `ToolLockRegistry` | 否 |
| `provider_registry.rs` | `"provider-registry"` | `ProviderRegistry` | 否 |
| `spawn_comm_registry.rs` | `"spawn-comm-registry"` | `DefaultSpawnCommRegistry` | 否 |
| `approval.rs` | `"approval"` | `ApprovalNotifier` | tools 为空 |

> 每个先读源码确认 `provide` 的真实类型参数与字段名，再照填。`provide::<T>` 的 `T` 与现有
> `bus_writer.provide(...)` 推断的类型一致。

- [ ] **Step 3: 改这 7 个的调用点**（`.extension`→`.plugin`）

- `agent_setup.rs` + `agent.rs`：这 7 个的 `.extension(Box::new(X))` → `.plugin(Box::new(X))`。
- **`base_agent/builder.rs:189`**：Memory 是 BaseAgentBuilder 自动装配项——把默认插入从
  `Box::new(ExtensionAsPlugin(Box::new(MemoryExtension::default())))` 改回
  `Box::new(MemoryExtension::default())`（现在它直接是 Plugin）。

- [ ] **Step 4: 编译 + 测试**

Run: `cargo check -p alva-agent-extension-builtin -p alva-app-core -p alva-app-cli -p alva-app-tauri && cargo test -p alva-app-core`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: migrate 7 bus-publish wrappers Extension->Plugin"
```

---

### Task 6: 跨层 builtin（Security / SystemContext）→ Plugin

**Files:** `wrappers/security.rs`、`wrappers/system_context.rs`、调用点

- [ ] **Step 1: SecurityExtension**

旧：`activate` 注册 `SecurityMiddleware`，`configure` provide `SecurityGuard`/`ModeControl`。
合并：
```rust
#[async_trait]
impl Plugin for SecurityExtension {
    fn name(&self) -> &str { "security" }
    fn description(&self) -> &str { /* 原值 */ }
    async fn register(&self, r: &Registrar) {
        r.tools(/* 原 tools() 内容，如有 */);
        r.middleware(Arc::new(SecurityMiddleware::new(/* 原参数 */)));
        r.provide::<dyn SecurityGuard>(self.guard.clone());
        // 原 configure 里 provide 的其余服务，逐条搬过来
    }
}
```
> 读 `security.rs` 现有 activate+configure，把两段 body **原样合并**进 register（顺序：tools → middleware → provide）。SecurityMiddleware 之前用 `OnceLock` 从 bus 拿 guard——保持不变。

- [ ] **Step 2: SystemContextExtension**

旧：`activate` 存 HostAPI，`configure` `append_system_prompt(AlwaysPresent, CLAUDE.md)` + `append_system_prompt(RuntimeInject, git_status)`。
新（无需再存 HostAPI——register 直接有 Registrar）：
```rust
async fn register(&self, r: &Registrar) {
    let user_ctx = get_user_context(r.workspace()).await;
    if let Some(md) = user_ctx.get("claudeMd") {
        r.system_prompt(ContextLayer::AlwaysPresent, format!("<project_context>\n{}\n</project_context>", md.trim()));
    }
    let sys_ctx = get_system_context(r.workspace()).await;
    if let Some(status) = sys_ctx.get("gitStatus") {
        r.system_prompt(ContextLayer::RuntimeInject, format!("<git_status>\n{}\n</git_status>", status.trim()));
    }
}
```
→ 顺手消除「存 HostAPI 跨阶段」workaround。

- [ ] **Step 3: 调用点 + 编译 + 测试**

- `agent_setup.rs` / `agent.rs`：Security/SystemContext 的 `.extension`→`.plugin`。
- **`base_agent/builder.rs:197`（Security）+ `:212`（SystemContext）**：BaseAgentBuilder
  自动装配项，默认插入从 `ExtensionAsPlugin(Box::new(...))` 改回直接 `Box::new(XxxExtension)`。

Run: `cargo check -p alva-agent-extension-builtin -p alva-app-core -p alva-app-cli -p alva-app-tauri && cargo test -p alva-app-core`
Expected: PASS（默认替换契约测试须绿）。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(builtin): migrate Security + SystemContext to Plugin (merge activate+configure)"
```

---

# Phase 3 — 迁移 app-core extensions

### Task 7: 跨层 app-core（Skills / Permission / Analytics / Pending / Lsp / Mcp）→ Plugin

**Files:** `extension/skills/extension.rs`、`permission.rs`、`analytics.rs`、`pending.rs`、`lsp/mod.rs`、`mcp/extension.rs`、调用点

- [ ] **Step 1: 逐个合并 activate+configure→register**

对每个：把 `activate` 的 `api.middleware(...)` → `r.middleware(...)`；`configure` 的 `bus_writer.provide(...)`/scan → register body。模板：
```rust
async fn register(&self, r: &Registrar) {
    r.tools(/* 原 tools()，如有 */);
    r.middleware(Arc::new(/* 原 middleware */));
    r.provide::<dyn Svc>(/* 原服务 */);
    /* 原 configure 的异步初始化，如 SkillsExtension 的 store.scan() */
}
```

| 文件 | name() | middleware | provide / 其它 |
|---|---|---|---|
| `skills/extension.rs` | `"skills"` | `SkillInjectionMiddleware` | tools(2) + `store.scan()` |
| `permission.rs` | `"permission"` | `PlanModeMiddleware` | `PlanModeControl` + `PermissionModeService` |
| `analytics.rs` | `"analytics"` | `AnalyticsMiddleware` | 建 `JsonlSink` + provide `AnalyticsSink` |
| `pending.rs` | `"pending"` | `PendingMiddleware` | `PendingService` |
| `lsp/mod.rs` | `"lsp"` | — | tools + `LspManager` |
| `mcp/extension.rs` | `"mcp"` | — | tools（动态——见注） |

> **MCP 注意**：若 MCP 的 tool 列表需异步发现（连接 server 后才知道），把发现放 `finalize()` 返回；
> 若 register 期即可拿到则放 register。读 `mcp/extension.rs` 现有 `tools()` 是否依赖晚期，决定放哪。

- [ ] **Step 2: 调用点 + 编译 + 测试**

Run: `cargo check -p alva-app-core -p alva-app-cli -p alva-app-tauri && cargo test -p alva-app-core`
Expected: PASS。

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor(app-core): migrate Skills/Permission/Analytics/Pending/Lsp/Mcp to Plugin"
```

---

### Task 8: 晚期插件（SubAgent / BlackboardComm）→ Plugin::finalize

**Files:** `extension/agent_spawn.rs`、`extension/blackboard_comm.rs`、调用点

- [ ] **Step 1: SubAgentExtension**

旧 `finalize(&FinalizeContext) -> Vec<Tool>`（agent_spawn.rs:499）直接映射到新 `Plugin::finalize(&LateContext)`：字段名一致（`cx.model`/`cx.tools`/`cx.max_iterations`/`cx.bus`）。`register()` 留空或仅注册静态部分。
```rust
#[async_trait]
impl Plugin for SubAgentExtension {
    fn name(&self) -> &str { "sub-agent" }
    async fn register(&self, _r: &Registrar) {}
    async fn finalize(&self, cx: &LateContext) -> Vec<Arc<dyn Tool>> {
        // 原 finalize body：读 cx.bus.get::<ProviderRegistry>() /
        // SpawnCommunicationRegistry，构造 SpawnScopeImpl + spawn tool。
    }
}
```

- [ ] **Step 2: BlackboardCommExtension（装配期跨插件读 → finalize）**

旧 `configure` 读 `SpawnCommunicationRegistry` 并 `registry.register(...)`（blackboard_comm.rs:61）。
搬到 `finalize`（此时 registry 已被 SpawnCommRegistry 在 register 阶段 provide）：
```rust
#[async_trait]
impl Plugin for BlackboardCommExtension {
    fn name(&self) -> &str { "blackboard-comm" }
    async fn register(&self, _r: &Registrar) {}
    async fn finalize(&self, cx: &LateContext) -> Vec<Arc<dyn Tool>> {
        if let Some(registry) = cx.bus.get::<dyn SpawnCommunicationRegistry>() {
            registry.register(Arc::new(BlackboardCommunication::new(self.board_registry.clone())));
        } else {
            tracing::warn!("blackboard-comm: SpawnCommunicationRegistry not present; skipping");
        }
        vec![]
    }
}
```

- [ ] **Step 3: 调用点 + 编译 + 测试**

Run: `cargo check -p alva-app-core -p alva-app-cli -p alva-app-tauri && cargo test -p alva-app-core`
Expected: PASS。SubAgent 相关测试（spawn）需绿，证明晚期读顺序正确。

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(app-core): SubAgent + BlackboardComm via Plugin::finalize (late wiring)"
```

---

# Phase 4 — 砍 Event 第 4 层 + AEP 改接 middleware

### Task 9: 新增 `AepBridgeMiddleware`，AEP loader 改用它

**Files:**
- Create: `crates/alva-app-extension-loader/src/aep_bridge.rs`
- Modify: `crates/alva-app-extension-loader/src/loader.rs`（删 `register_plugin_handlers` + `on_as`，改注册一个 middleware）

- [ ] **Step 1: 写 `aep_bridge.rs`**

```rust
//! AepBridgeMiddleware — 把 AEP 子进程插件接到 middleware 钩子（取代旧事件桥）。
use async_trait::async_trait;
use std::sync::Arc;
use alva_kernel_core::middleware::{Middleware, MiddlewareError};
use alva_kernel_core::state::AgentState;
use crate::proxy::RemoteExtensionProxy; // 按真实路径

pub struct AepBridgeMiddleware {
    plugins: Vec<Arc<RemoteExtensionProxy>>,
}

impl AepBridgeMiddleware {
    pub fn new(plugins: Vec<Arc<RemoteExtensionProxy>>) -> Self { Self { plugins } }

    fn subscribers(&self, aep_event: &str) -> impl Iterator<Item = &Arc<RemoteExtensionProxy>> {
        self.plugins.iter().filter(move |p| {
            p.init_result().event_subscriptions.iter().any(|s| s == aep_event)
        })
    }
}

#[async_trait]
impl Middleware for AepBridgeMiddleware {
    fn name(&self) -> &str { "aep-bridge" }

    async fn on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        // 旧 on_agent_start 订阅
        for p in self.subscribers("on_agent_start") { p.dispatch_agent_start().await; }
        // 旧 on_user_message → 从 session 取最新用户消息文本
        let msgs = state.session.messages().await;
        if let Some(text) = latest_user_text(&msgs) {
            for p in self.subscribers("on_user_message") { p.dispatch_user_message(&text).await; }
        }
        Ok(())
    }

    async fn before_tool_call(
        &self, _state: &mut AgentState, tool_name: &str, tool_call_id: &str, args: &serde_json::Value,
    ) -> Result<(), MiddlewareError> {
        for p in self.subscribers("before_tool_call") {
            if let Some(reason) = p.dispatch_before_tool_call(tool_name, tool_call_id, args).await {
                return Err(MiddlewareError::blocked(reason)); // 按真实阻断 API
            }
        }
        Ok(())
    }
    // after_tool_call / on_agent_end 同理映射。
}

fn latest_user_text(msgs: &[alva_kernel_abi::AgentMessage]) -> Option<String> {
    msgs.iter().rev().find_map(|m| match m {
        alva_kernel_abi::AgentMessage::Standard(s) if s.is_user() => Some(s.text_content()),
        _ => None,
    }) // 按真实 AgentMessage API 调整
}
```
> 各方法签名以 `crates/alva-kernel-core/src/middleware.rs:53-141` 的真实 `Middleware` 钩子签名为准（`before_tool_call`/`after_tool_call`/`on_agent_start`/`on_agent_end` 的参数）。`RemoteExtensionProxy` 的 `dispatch_*` 当前是同步（`dispatch_event_sync`）——可保留同步调用或升级 async。`MiddlewareError::blocked` 用真实的阻断构造方式（读 shared.rs）。

- [ ] **Step 2: loader 改注册 middleware**

`SubprocessLoaderExtension` 迁到 Plugin（若尚未），其 `finalize`（或 register，按插件加载时机）里：
```rust
r.middleware(Arc::new(AepBridgeMiddleware::new(self.loaded_plugins.clone())));
```
删除 `register_plugin_handlers`（loader.rs:230）与 `aep_to_core_event_type` 中对 `on_as` 的使用（映射表保留给 `subscribers` 复用）。

- [ ] **Step 3: 编译 + 测试**

Run: `cargo check -p alva-app-extension-loader && cargo test -p alva-app-extension-loader`
Expected: PASS。补一个测试：构造带 `before_tool_call` 订阅的假插件，验证 `before_tool_call` 钩子被调用、阻断生效。

- [ ] **Step 4: Commit**

```bash
git add crates/alva-app-extension-loader/src/aep_bridge.rs crates/alva-app-extension-loader/src/loader.rs
git commit -m "feat(loader): AepBridgeMiddleware — route AEP plugins via middleware hooks"
```

---

### Task 10: 删除 Event 层

**Files:**
- Delete: `crates/alva-agent-core/src/extension/events.rs`
- Modify: `extension/host.rs`（删 handler map / `emit` / `on` / `on_as` / `register_handler`）、`extension/bridge.rs`（删事件分发；该 middleware 整体删除）、`extension/mod.rs`（删导出）、`agent_builder.rs`（step 5 不再 push bridge）

- [ ] **Step 1: 删 bridge 推入**

agent_builder.rs 删除 `middleware_stack.push_sorted(Arc::new(ExtensionBridgeMiddleware::new(host.clone())));` 与其 import。

- [ ] **Step 2: 删 `bridge.rs` + `events.rs`**

`rm crates/alva-agent-core/src/extension/bridge.rs crates/alva-agent-core/src/extension/events.rs`，并从 `mod.rs` 删 `mod bridge; mod events;` 及相关 `pub use`。

- [ ] **Step 3: 清 host.rs**

删 `handlers` 字段、`register_handler`、`emit`、`HandlerFn` type。`HostAPI` 删 `on`/`on_as`（适配器 ExtensionAsPlugin 不再调用它们——确认 adapter 只用了 `middleware`/`register_command`/`append_system_prompt`，是的）。保留 `register_middleware`/`take_middlewares`/`append_system_prompt`/`register_command`/commands。

- [ ] **Step 4: 编译 + 测试**

Run: `cargo check --workspace && cargo test -p alva-agent-core -p alva-app-extension-loader`
Expected: PASS（事件层 0 in-tree 消费者，AEP 已转 middleware）。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor(agent-core): delete ExtensionEvent layer (bridge/events/on/emit)

事件层是 middleware 的同步残废镜像，in-tree 0 消费者，AEP 已改接 middleware。"
```

---

# Phase 5 — 裸 middleware 免壳（删 7 个空壳）

### Task 11: 删纯中间件 wrapper，调用点改 `.middleware()`

**Files:**
- Delete 类型：`extension/loop_detection.rs`、`dangling_tool_call.rs`、`tool_timeout.rs`、`compaction.rs`、`checkpoint.rs`、`hooks/extension.rs`(若纯注册)、`evaluation/extension.rs`(若纯注册)
- Modify: `extension/mod.rs`（删导出）、`agent_setup.rs`、`agent.rs`

- [ ] **Step 1: 确认哪些是「纯注册」**

逐个读：若 impl 只有 `name()` + `activate(){ api.middleware(...) }`，则删类型；
若除注册 middleware 外还有逻辑（HooksExtension/EvaluationExtension 可能有），**保留为 Plugin**，本 Task 跳过它。

- [ ] **Step 2: 调用点换 `.middleware()`**

`agent_setup.rs` / `agent.rs`：
```rust
// 旧
.extension(Box::new(alva_app_core::extension::ToolTimeoutExtension))
// 新
.middleware(Arc::new(alva_kernel_core::builtins::ToolTimeoutMiddleware::default()))
```
按真实 middleware 路径逐个替换（LoopDetection/DanglingToolCall/ToolTimeout 在 `alva_kernel_core::builtins`；Compaction 在 `alva_host_native::middleware`；Checkpoint 在 `alva_host_native::middleware`）。

- [ ] **Step 3: 删类型 + 清导出**

删除上述纯壳文件，`extension/mod.rs` 删对应 `mod`/`pub use`。

- [ ] **Step 4: 编译 + 测试**

Run: `cargo check --workspace && cargo test -p alva-app-cli`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: drop pure middleware-wrapper plugins; register middleware directly"
```

---

# Phase 6 — 移除旧 Extension trait + 重命名

### Task 12: 删 Extension/适配器 + 重命名 Plugin/Registrar 体系

**Files:** `extension/mod.rs`、`adapter.rs`、`context.rs`、`agent_builder.rs`、`base_agent/builder.rs`、全部已迁移类型文件、调用点

- [ ] **Step 1: 确认无 `impl Extension` 残留**

Run: `grep -rn "impl Extension for" crates/ | grep -v test`
Expected: 空（所有都迁完）。若有残留，回到对应 Phase 补迁。

- [ ] **Step 2: 删 Extension trait + 适配器 + 旧 context**

删 `extension/mod.rs` 里 `Extension` trait 定义、`mod adapter`、`ExtensionAsPlugin`；删 `context.rs`（`ExtensionContext`/`FinalizeContext`，已被 `Registrar`/`LateContext` 取代）。
`agent_builder.rs` 删 `.extension()`（只留 `.plugin()`）；`base_agent/builder.rs` 删 `.extension()`（skip-by-name 已在 Task 3b 改为 Plugin，此处仅删旧 `.extension()` 方法 + 确认无残留调用）。

- [ ] **Step 3: 重命名（可选但已确认要做）**

`XxxExtension` → `XxxPlugin`（类型名 + 调用点）。建议用 `cargo` 编译驱动：改一个名编译报错处即调用点。按 spec §4 重命名表执行。
> 量大，可拆成「按 crate 一个 commit」。

- [ ] **Step 4: 全量编译 + 测试**

Run: `cargo check --workspace && cargo test --workspace`
Expected: PASS。

- [ ] **Step 5: Commit（按 crate 分批）**

```bash
git add -A
git commit -m "refactor: remove Extension trait + adapter; rename to Plugin/Registrar"
```

---

# Phase 7 — 文档 + 全量验证

### Task 13: 更新文档 + 跑全部 CI 门禁

**Files:** `AGENTS.md`、`crates/alva-kernel-abi/src/scope/context/traits.rs`（"8-hook" 注释）、spec 状态

- [ ] **Step 1: 更新 AGENTS.md + 所有 SDK doc 示例**

> 注:迁移过程会累积一批 doc-only 漂移,**集中到本步用 grep 一次性扫干净**,不在每个迁移 Task 里反复改。至少覆盖:
> - `.extension(Box::new(X))` 示例(已迁移类型)→ 改 `.plugin(Box::new(X))`:`base_agent/builder.rs` struct doc、`AGENTS.md`、`docs/ARCHITECTURE.md`。
> - 迁移后文件里 struct/模块 doc、`// INPUT:` 机器头注释仍写 `Extension`/`ExtensionContext`/`Extension::configure`/`Extension::activate`/`Extension::name` 的,改成 `Plugin`/`Registrar`/`Plugin::register`/`Plugin::name`。
> - grep 线索:`grep -rn "Extension::configure\|Extension::activate\|crate::extension::{Extension\|\.extension(Box::new\|configure()" crates/ --include=*.rs`(逐条判断是真实代码还是 stale doc;真实代码到此应已无,剩的多是注释)。
> 已知点:`provider_registry.rs:16`、`approval.rs:17/25`、`spawn_comm_registry.rs:1/11/29/65`、`tool_lock_registry.rs:1`、`communication.rs:177`、`lsp/mod.rs:110`、`guard.rs:87`(`SecurityExtension::configure`→`register`)等(以最终 grep 为准)。
> **需事实改正(非仅改名)**:`blackboard_comm.rs:26` 注释称 `SpawnCommunicationRegistry` 由 `BaseAgentBuilder::build()` 默认提供——错误,它是 opt-in(`SpawnCommRegistryExtension`);改成"需显式装配 SpawnCommRegistryExtension,否则 finalize 走 warn 跳过"。
> **需整段重写(非仅改名)**:`loader.rs` 模块 `//!` 头(约 10-63 行)整段还在描述旧 `Extension`/`activate`/`configure`/`ExtensionAsPlugin` 两阶段生命周期——重写成新的单阶段 `Plugin::register`(load_plugins→r.middleware(AepBridge))。
> 其余 stale:`plugin.rs:16`(过渡期/ExtensionAsPlugin 已不存在)、`loader.rs` `loaded_count` doc 提 `configure`、3 个 `// INPUT:` 头(tool_lock_registry/provider_registry/spawn_comm_registry 仍写 `{Extension, ExtensionContext}`)。

更新 AGENTS.md:

`alva-agent-core` 行：`Extension trait`→`Plugin trait`，方法列表改 `name/register/finalize`；
`HostAPI/ExtensionContext`→`Registrar`；删 Event/`ExtensionBridgeMiddleware` 相关描述；
`alva-agent-extension-builtin` 行的 11 个 wrapper 描述更新（删 7 个纯壳）。

- [ ] **Step 2: 修 traits.rs 注释**

把 `ContextHooks` 的 "8-hook" 注释改为 "7 lifecycle hooks + name()"（与 AGENTS.md 一致）。

- [ ] **Step 3: 全门禁**

Run:
```bash
cargo test --workspace
cargo check -p alva-host-wasm --target wasm32-unknown-unknown
bash scripts/ci-check-deps.sh
```
Expected: 全 PASS（Rule 17 边界未破——新类型都在 agent-core SDK 层，未引入 app/host 依赖）。

- [ ] **Step 4: 标记 spec 完成**

把 spec 头部状态改为「已实现」。

- [ ] **Step 5: Commit**

```bash
git add AGENTS.md crates/alva-kernel-abi/src/scope/context/traits.rs docs/superpowers/specs/2026-06-14-plugin-middleware-unification-design.md
git commit -m "docs: update AGENTS.md + traits comment for Plugin/Registrar; mark spec done"
```

---

## 收益核对（完成后应达成）

- 概念 4 → 3；删 `events.rs` + `bridge.rs` + `on/on_as/emit`。
- 删 7 个纯中间件壳 + tool-group/bus-publish 壳收敛进 `register()`（~173 行样板）。
- Extension 6 方法/2 句柄 → Plugin 3 方法/1 句柄；消除 SystemContext/SubprocessLoader 跨阶段 clone。
- tool 入口 2 → 1（+ 可选 `finalize` 晚期发现）。
- provide-only 装配顺序无关；AEP 经 middleware；全程 wasm32-clean + Rule 17 不破。
- 为方案 C（声明式 `capability!`）与子项目①（Preset 分档）留干净底座。
