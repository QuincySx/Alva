# 15 EngineDescriptor + 协商模块上移

- 级别：P2
- 影响：新增第五个引擎能成功注册进 Hub 内核，但在产品层会被四道写死引擎名的 `match` 依次拒绝，用户看到的是「unsupported Hub engine」；而且每一处这样的判断都被 Tauri 与 GPUI 两个前端各复制了一份，其中两份已经开始漂移。
- 状态：TODO

## 根因

**契约里的每一个缺口，都会在产品层重现为一次字符串比较；而每一次字符串比较，都会被按前端复制粘贴一遍。**

## 证据

### A. 量化：字面量与复制粘贴

**统计方法**（可复现）：对 `crates/alva-hub-{desktop,gpui,server,cli}/src` 下每个 `.rs` 文件，从第一行 `#[cfg(test)]` 处截断（只留生产段），再数 `"alva"` / `"claude"` / `"codex"` / `"pi"` / `"claude-print"` 五个字符串字面量的出现次数。

实测结果：**195 处 / 25 个文件**。

| crate | 处数 |
|---|---|
| `alva-hub-desktop` | 104 |
| `alva-hub-gpui` | 76 |
| `alva-hub-cli` | 12 |
| `alva-hub-server` | 3 |

Top 文件：`desktop/readiness.rs` 30、`desktop/providers.rs` 25、`gpui/readiness.rs` 20、`desktop/permissions.rs` 14、`gpui/permissions.rs` 14、`gpui/ui/app.rs` 12、`desktop/repair.rs` 11、`desktop/provider_plane/route_planner.rs` 11、`gpui/repair.rs` 10。

复制粘贴的实测：

- `cmp crates/alva-hub-desktop/src/permissions.rs crates/alva-hub-gpui/src/permissions.rs` **退出码 0（逐字节相同）**；两文件各 874 行，其中生产段是 1–412 行（`#[cfg(test)]` 在 `:413`）。
- `diff crates/alva-hub-desktop/src/readiness.rs crates/alva-hub-gpui/src/readiness.rs | wc -l` → **263**（1178 行 vs 1037 行，已经漂移）。
- `diff crates/alva-hub-desktop/src/repair.rs crates/alva-hub-gpui/src/repair.rs | wc -l` → **25**（418 行 vs 412 行）。
- 背景：`crates/alva-hub-gpui/Cargo.toml:14` 的注释自陈「Backend core — migrated verbatim from alva-hub-desktop (no tauri)」，且 gpui **不**依赖 `alva-hub-desktop`，所以整个 store 也是一份 7786 行的复制（`crates/alva-hub-gpui/src/store.rs`）。

### B. 抽象泄漏：产品层直接 use adapter 的厂商枚举

- `crates/alva-hub-desktop/src/permissions.rs:8-9`：
  ```rust
  use alva_engine_adapter_claude::ClaudePrintPermissionMode;
  use alva_engine_adapter_codex::CodexSandboxMode;
  ```
- `crates/alva-hub-desktop/src/permissions.rs:62-70` —— `pub enum EngineEnforcement` 的变体名写死厂商：`InteractiveCallbacks` / `ClaudePrintMode(..)` / `CodexSandbox(..)` / `PiReadOnly` / `PiUnrestricted`。
- `crates/alva-hub-desktop/src/permissions.rs:87-162` —— `enforcement_for` 是 12 个 `(引擎名字面量, PermissionProfile)` 元组的 `match`，兜底在 `:158-161`：
  ```rust
  (other, _) => Err(format!(
      "unsupported Hub engine '{other}'; expected alva, claude, codex, or pi"
  ))
  ```

### C. 决定性证据：第五个引擎注册成功，然后死在产品层四道 match

第五个引擎可以成功进入 `HubRuntime::register_engine`（`crates/alva-hub-core/src/runtime.rs:240-254` 只查重名，不查引擎名），随后依次撞上：

1. `crates/alva-hub-desktop/src/permissions.rs:158-161` —— `enforcement_for` 兜底 `Err("unsupported Hub engine")`。
2. `crates/alva-hub-desktop/src/store/branch.rs:46-89` —— `EngineTarget::for_engine` 的 `match engine_id`，兜底 `:72-76` 同一句错误。
3. `crates/alva-hub-desktop/src/readiness.rs:526-567` —— `quick_veto_for_target` 的 `match engine_id`，兜底 `:565`：`other => Some(format!("engine '{other}' is not supported by Hub"))`。
4. `crates/alva-hub-desktop/src/alva.rs:89-119` —— `build_engine_for_target_with_repository` 的 `match (engine_id, plan.enforcement)`，兜底 `:115-118`。

（gpui 侧同构：`permissions.rs:87`、`store.rs:58`、`readiness.rs`、`alva.rs:57`。）

**因此 `docs/adr/0007-agent-hub-abstraction-alignment.md:51` 的「so any engine backend can already be registered」在产品路径上不成立** —— 本工单要求一并订正 ADR-0007 Finding 2 的措辞：类型层可注册 ≠ 产品层可用。

### D. 其它同型 per-engine 事实（本该由引擎自己声明）

- **max_turns**：`crates/alva-hub-cli/src/main.rs:111-116` 与 `:505-511` 硬编码「Codex exec does not expose a compatible --max-turns control」并直接拒绝命令行；而 `crates/alva-engine-runtime/src/request.rs:34` 就有 `pub max_turns: Option<u32>` —— **请求侧建模了、应答侧没建模**，产品只好替引擎回答「你支不支持」。
- **provider 协议**：`crates/alva-hub-desktop/src/provider_plane/route_planner.rs:9-13` 五张常量表（`CLAUDE_SUBSCRIPTION_ENGINES` / `CODEX_SUBSCRIPTION_ENGINES` / `ANTHROPIC_ENGINES` / `OPENAI_RESPONSES_ENGINES` / `OPENAI_CHAT_ENGINES`）。
- **fast mode**：同文件 `:137-141` —— `options.supports_fast_mode &= engine_id == "codex" && matches!(input.provider_kind, "openai-responses" | "codex-subscription")`。
- **自定义 HTTP 头**：`crates/alva-hub-desktop/src/providers.rs:622` —— `if engine_id != "alva" && !provider.custom_headers.is_empty()`。
- **模型白名单**：`crates/alva-hub-desktop/src/providers.rs:754` —— `!matches!(engine.as_str(), "alva" | "claude" | "codex" | "pi")`。
- **inspection**：`engine_id == "alva"` 三处、跨两种语言：`crates/alva-hub-gpui/src/ui/inspector.rs:53`、`crates/alva-hub-desktop/src/inspection.rs:131`、`crates/alva-hub-desktop/web/src/components/engine-inspector.tsx:66,71`。
- **provider 装配**：`crates/alva-hub-desktop/src/alva.rs:32` —— `let provider_config = if engine_id == "alva" { .. }`。

### E. 正确形状仓里已经存在

- `crates/alva-engine-runtime/src/inspector.rs:23-30` —— `InspectionCapabilities { level, historical, live, raw_records, recursive_children, export_jsonl_bundle }`，配 `enum InspectionLevel { Full, Summary, Unsupported }`（`:15-20`）和 `full()` / `unsupported()` 构造器（`:32-54`）。**6 个维度 + 一个三态枚举，比裸 bool 严谨得多。**
- `crates/alva-hub-gpui/src/alva.rs:21-24` —— `pub struct EngineBuild { runtime: Arc<dyn EngineRuntime>, inspector: Option<Arc<dyn EngineInspector>> }`。这就是「可选能力显式建模」的正确写法，**只差把它带过 `register_engine` 这道门**。

### F. `RuntimeCapabilities` 的装饰位已经在说谎

- `crates/alva-engine-runtime/src/event.rs:111-117` —— 5 个裸 bool：`streaming` / `tool_control` / `permission_callback` / `resume` / `cancel`。
- `rg -n 'capabilities\(\)\.(streaming|tool_control|resume|cancel|permission_callback)' crates/ --glob '*.rs'` 的全部命中：`crates/alva-hub-core/src/runtime.rs:440`（resume）、`:637`（cancel）、`:659`（permission_callback），外加 `crates/alva-engine-adapter-claude/src/adapter.rs:667` 的一条测试断言。**`streaming` 与 `tool_control` 零行为消费者**，只被 `capability_labels` 转成 UI 标签（`crates/alva-hub-desktop/src/readiness.rs:251-269` 与 `crates/alva-hub-gpui/src/readiness.rs:233-251`，又是两份重复）。
- 装饰位在说谎：`crates/alva-engine-adapter-codex/src/adapter.rs:240` 声明 `streaming: false`，而 `crates/alva-engine-adapter-codex/src/mapping.rs:122-131` 实打实构造并发出 `RuntimeEvent::MessageDelta`。

## 复现

1. 写一个最小的第五引擎（`impl EngineRuntime for StubEngine`，`capabilities()` 全 true，`execute` 立刻发 `SessionStarted` + `Completed`），在 `crates/alva-hub-desktop/src` 内的测试里以 id `"gemini"` 注册。
2. `HubRuntime::register_engine("gemini", Arc::new(StubEngine))` 返回 `Ok`。
3. 调 `alva_hub_desktop::permissions::enforcement_for("gemini", PermissionProfile::WorkspaceWrite)`。
4. 观察到：`Err("unsupported Hub engine 'gemini'; expected alva, claude, codex, or pi")` —— 这条断言今天就已经存在，见 `crates/alva-hub-desktop/src/permissions.rs:672`。
5. 期望：引擎在注册时自带 permission plan 声明，产品层无需知道它叫什么名字就能判定它能否满足 profile。

## 工作

### 第一批 —— 在 `alva-engine-runtime` 定义 `EngineDescriptor`（纯新增）

- 新增 `crates/alva-engine-runtime/src/descriptor.rs`：
  ```rust
  pub struct EngineDescriptor {
      pub runtime: Arc<dyn EngineRuntime>,
      pub inspector: Option<Arc<dyn EngineInspector>>,
      pub capabilities: RuntimeCapabilities,
      pub permission_plans: Vec<PermissionPlanDecl>,
      pub config_surface: ConfigSurface,
      pub install_metadata: InstallMetadata,
  }
  ```
- `PermissionPlanDecl` 的**旋钮必须 opaque**：产品层只见 `{ profile, relation: Equivalent|Stricter, summary: String, apply: <句柄> }`，看不到 `ClaudePrintPermissionMode` / `CodexSandboxMode` 这类厂商类型。这是硬约束：`scripts/ci-check-deps.sh:88` 规定 hub-core 只许依赖 `alva-engine-runtime`，若旋钮不 opaque，第三批的上移会直接被 CI 拦下。
- `capabilities` 参照 `inspector.rs:23-54` 的形状重做：把 F 段的裸 bool 换成有语义的枚举/结构，并给 `full()` / 保守默认构造器。
- `config_surface` 承接 D 段的 provider 协议 / fast mode / 自定义头 / max_turns 支持性声明。

### 第二批 —— `register_engine` 收 descriptor

- `crates/alva-hub-core/src/runtime.rs:240-254` 改签名为 `register_engine(id, EngineDescriptor)`；内部 registry 存 descriptor。
- 保留一个 `register_engine_runtime(id, Arc<dyn EngineRuntime>)` 便捷封装（内部填保守默认 descriptor），让四个产品的现有调用点（`desktop/run.rs:138,527`、`gpui/run.rs:127,509`、`hub-cli/main.rs`）能分批迁移。
- 四个 adapter crate 各自导出自己的 descriptor 构造函数（`ClaudePrintAdapter::descriptor(config)` 等），把 B / D 段的厂商知识搬回引擎自己家。

### 第三批 —— `permissions.rs` / `readiness.rs` / `repair.rs` 上移到 `alva-hub-core`

- 三对文件（共 6 份、约 4300 行）合并成一份，放进 `alva-hub-core`；desktop 与 gpui 只 re-export 消费。
- 上移后这三个模块**不得**再出现引擎名字面量：`enforcement_for` 改为查 descriptor 的 `permission_plans`；`quick_veto_for_target` 改为查 `install_metadata`；`build_engine` 的四路 match 改为查 descriptor。
- `capability_labels` 的两份重复随之合一。

### 第四批 —— 订正 ADR 与文档

- `docs/adr/0007-agent-hub-abstraction-alignment.md:51` 的「so any engine backend can already be registered」改为准确表述：类型层可注册，产品层需 descriptor 才可用；并注明本工单是把这句话变成事实的工作。
- 更新 `crates/alva-engine-runtime/src/AGENTS.md`（新增 descriptor 条目 + 业务域清单一行）与 `crates/alva-hub-core/src/AGENTS.md`（协商模块的新归属）。

### 第五批 —— 处置 `RuntimeCapabilities` 的两个装饰位

二选一，需在实现时明确：

- **接上行为**：让 `streaming` 真正参与决策（例如驱动 UI 是否等待 delta），并修正 `crates/alva-engine-adapter-codex/src/adapter.rs:240` 的 `streaming: false`（它发 `MessageDelta`，应为 `true`）。
- **删掉**：从 `crates/alva-engine-runtime/src/event.rs:112-113` 移除 `streaming` / `tool_control`，同步删两份 `capability_labels` 中的对应分支。

## 验收

- [ ] `rg -n 'alva_engine_adapter_' crates/alva-hub-core/src crates/alva-hub-desktop/src/permissions.rs crates/alva-hub-gpui/src/permissions.rs` 输出为空（厂商类型不再泄漏到协商层）。
- [ ] 用 A 段的统计方法重新计数，四个产品 crate 生产段的引擎名字面量总数 **≤ 40**（从 195 降下来；剩余部分应集中在默认 provider 目录种子与 UI 显示名）。
- [ ] `crates/alva-hub-gpui/src/permissions.rs`、`readiness.rs`、`repair.rs` 三个文件不再存在（或退化为 `pub use alva_hub_core::...;` 一行）；`crates/alva-hub-desktop/src` 同。
- [ ] 新测试 `a_fifth_engine_registered_with_a_descriptor_passes_every_product_gate`：注册 id `"gemini"` 的 stub descriptor 后，`enforcement_for` / `EngineTarget::for_engine` / `quick_veto_for_target` / `build_engine` 四道门全部返回 `Ok`。
- [ ] `crates/alva-hub-desktop/src/permissions.rs:672` 那条 `assert!(error.contains("unsupported Hub engine 'gemini'"))` 被改写为「未声明 descriptor 的引擎才被拒」的新语义断言。
- [ ] `bash scripts/ci-check-deps.sh` 全绿（`:88` 的 hub-core 单依赖规则**不放宽**）。
- [ ] `docs/adr/0007-agent-hub-abstraction-alignment.md` 中不再有「any engine backend can already be registered」这句原文（`rg -n 'any engine backend can already be registered' docs/` 输出为空）。
- [ ] 第五批：`rg -n 'capabilities\(\)\.streaming|capabilities\(\)\.tool_control' crates/` 要么有真实行为消费者，要么两个字段已从 `event.rs` 删除 —— 不允许保持现状。
- [ ] `cargo test --workspace` 全绿。

## 依赖与切分

- **硬前置 1**：工单 14 必须先拍板。否则 descriptor 会被塞进一个待下线的状态机。
- **硬前置 2**：工单 04 / 05 / 06 先止血。第三批是大规模搬运，把已知 bug 一起搬上去会让归因变得极其昂贵。
- **建议前置**：工单 13 的第二批（控制面改从 `runs` 取）先落，避免 `register_engine` 在短期内连改两次签名。
- **切分点**：
  1. 第一批纯新增，零调用者，可独立合入并单独测试；
  2. 第二批用「便捷封装」保住旧调用点，让四个产品各自一个 commit 迁移；
  3. 第三批按文件切：`repair.rs`（差异 25 行，最容易）→ `permissions.rs`（逐字节相同，机械合并）→ `readiness.rs`（差异 263 行，需先逐条判定漂移是有意还是遗漏，**单独一个 commit 并附漂移清单**）；
  4. 第四批与第五批彼此独立，可随时插入。
- **不在本单范围**：`gpui/src/store.rs` 那 7786 行的 store 复制。它属于同一个失败模式，但体量与风险独立，应单开工单。
