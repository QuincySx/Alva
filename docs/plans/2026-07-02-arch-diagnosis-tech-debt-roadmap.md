# Alva-Agent 架构诊断报告（2026-07-02，基于 main @ fe4559c）

> **日期**：2026-07-02
> **基于提交**：`fe4559c14bc791b72c990b6a3baa927f869b069b`（main，短 hash `fe4559c`）
> **诊断方法**：4 路并行探查（架构健康 / 高危遗留 / CLI 深挖 / 测试与 CI 基建）
> + 对最致命结论（KILL-1 死锁链路等）的逐行人工复核
> **状态**：待评审

---

## 目录

- [TL;DR](#tldr)
- [一、整体架构诊断](#一整体架构诊断)
- [二、技术债务排行](#二技术债务排行)
- [三、超危险遗留深挖：KILL-1 死锁 + 重构计划与 PR 方案](#三超危险遗留深挖kill-1-死锁--重构计划与-pr-方案)
- [四、3 年迁移路线图](#四3-年迁移路线图)
- [五、CLI 测试方案：具体指导意见](#五cli-测试方案具体指导意见)
- [附录：诊断方法与证据等级](#附录诊断方法与证据等级)

---

## TL;DR

**架构骨架是健康的，但护栏已经和它守护的代码脱钩了。** 分层纪律（kernel 零全局可变状态、SDK/app 边界、plugin-only 扩展）是真实存在且大体被遵守的。真正的危险集中在四处：**① 一个默认配置下必现的进程级死锁（子 agent 跑 shell 即挂死，已逐行证实）；② CI 依赖防火墙有结构性盲区且仓库根本没有托管 CI；③ 约 4000 行"看起来活着"的死代码/双实现在稀释地图与地形的对应关系；④ CLI 的三大交互面（headless、TUI、REPL）恰好是零测试覆盖区。** 好消息：这些都可以用一串小 PR 收敛，且已有的 Model Eval Suite 是很好的地基。

## 一、整体架构诊断

### 做对了的（承重墙，别动）

- **kernel 纯度是真的**：`alva-kernel-bus/abi/core` 三个 crate 生产区零进程级可变全局状态；全 workspace 无 `static mut`/`lazy_static`。
- **插件化契约执行到位**：ad-hoc setter 全部砍掉、同名替换契约、`components::apply_components` 让 CLI/Tauri/测试走同一装配路径——这是防复制粘贴漂移的正确设计。
- **wasm 探针**（`alva-host-wasm::_wasm_smoke_probe`）是聪明的编译期护栏。
- **Model Eval Suite** 的 mock/real 双轨 + `failure_owner` 归因，是同类项目里少见的成熟设计。

### 系统性病灶（贯穿所有发现的一条线）

四份独立探查殊途同归指向同一件事：**强制机制在名义上存在，实际上失效**——

| 护栏 | 名义 | 实际 |
|---|---|---|
| `ci-check-deps.sh` | "最重要的分层约束" | Rule 12/16 检查**不存在的 crate**（`alva-provider`、`alva-app`），永远空过；Rule 17 不带 `--all-features`，看不到 `alva-agent-extension-builtin → alva-app-extension-browser` 这条被 `alva-app-core` 打开 feature 后**每次真实构建都激活**的 SDK→app 违规（均已亲自证实，`ci-check-deps.sh:71,96,136`） |
| 托管 CI | — | **不存在**。无 `.github/workflows/`，test/clippy/fmt/coverage 全靠手动 |
| `#[deprecated]` | 标记退役 | `AgentRuntimeBuilder` 500 行 deprecated 代码靠自己的测试 + 1 个 example 续命，无真实调用者 |
| AGENTS.md（"唯一真相源"） | 追踪现实 | crate 数写 30（实际 32）、`alva-llm-wire`/`alva-app-gateway` 在层级表缺失、`engine-adapter-alva` 的接线描述是错的、说 ARCHITECTURE.md 讲"三仓库"（实际内容早已不同） |
| 并发测试 | 守护竞态 | `agent_session.rs` 唯一的并发测试**先 sort 再断言**，把要验证的顺序性 sort 没了 |

单看每条都小，合起来就是：**"CI 绿 = 没事"这个心智模型已经不成立**。这是干净架构随时间侵蚀的标准路径。

## 二、技术债务排行

按 危险度 × 恶化趋势 × 修复杠杆 排序：

| # | 债务 | 等级 | 位置 | 一句话 |
|---|---|---|---|---|
| 1 | **KILL-1：默认配置死锁** | 🔴 崩溃级 | `tool_batch.rs:87` + `scheduler.rs:336` + `agent_spawn.rs:597-618` | 子 agent 跑一条 shell → 同 task 上 read-then-write 同一把 tokio RwLock → 全会话永久挂死 |
| 2 | **KILL-1b：取消不传播** | 🔴 | `agent_spawn.rs:609` | 子 agent 拿的是 `CancellationToken::new()` 断连 token，父取消后子树空转到 max_iterations；`sleeper: None` 连超时也废了 |
| 3 | **CI 防火墙盲区 + 无托管 CI** | 🔴 制度级 | `scripts/ci-check-deps.sh` | 虚假安全感比没有护栏更糟 |
| 4 | **agent_session.rs 违反 ABI 纯度 + seq 竞态（D-1）** | 🔴 | `alva-kernel-abi/src/agent_session.rs:671-679` | "纯契约层"里住着 ~640 行行为实现；并发 append 时 seq 与落盘顺序可背离 → `tool_result` 排到 `tool_use` 前，直接喂坏 LLM 上下文 |
| 5 | **compaction 双实现** | 🟠 分叉级 | `agent-context/middleware.rs`（775 行）vs `compact.rs+auto_compact.rs`（751 行） | 同一职责两种范式并存，接线在两层，每次策略演进付双倍成本，且无机制阻止两者同时挂到一个 agent 上 |
| 6 | **Tauri↔CLI 漂移孪生** | 🟠 安全相关 | `tauri/agent.rs:1078` vs `cli/event_handler.rs:225` | 权限决定解析词汇和 fallback 都不同（硬 Err vs 静默拒绝）；**漂移的 Tauri 侧恰好 0 测试**；provider 构造 5 份近似拷贝 |
| 7 | **死代码群** | 🟠 认知级 | engine 3 crate（零消费者）、`AgentRuntimeBuilder`（500 行）、`register_builtin_tools`、`outcome.rs` 状态机（零生产消费者）、`alva-app-gateway`（半成品） | ~4000+ 行"看着活着"的代码在稀释"哪些是承重墙"的判断 |
| 8 | **数据正确性三连**（D-2/D-3/D-6） | 🟠 | `sqlite_session/registry.rs`、`agent_session.rs:984`、extension-builtin 16 处 | SQLite 后端 record_usage 非原子丢更新；listener 只增不减泄漏；16 处裸 `.lock().unwrap()` 一次 panic 永久 poison 整个 task/team 子系统 |
| 9 | **session_projection 延时炸弹** | 🟡→🔴 | `session_projection.rs:136` 的 TODO | 子 agent 双重投影今天不炸**仅因**真实子事件还没内联；谁接 Phase 3 linkage 谁引爆（turns 和 token 双重计数） |
| 10 | **测试基建欠账** | 🟡 | 全仓 | 无覆盖率工具；e2e harness 三处复制粘贴未沉淀进 `alva-test`；`alva-app-debug` 21 处固定 sleep + 硬编码端口 19230-19245 |
| 11 | **bus 逼近自定退役阈值** | 🟡 | 15/20 caps | `kernel-abi` 已挂 5 个 cap；BUS-RULES 自己写了 Cap>20 整层治理作废 |
| 12 | **文档漂移** | 🟡 | AGENTS.md / CLAUDE.md / BUS-RULES | 数字、层级表、接线描述多处与代码不符 |

## 三、超危险遗留深挖：KILL-1 死锁 + 重构计划与 PR 方案

### 死锁链路（已逐行核实的四个环节）

1. `alva-kernel-core/src/tool_batch.rs:87-100`——每个工具执行前 `registry.acquire(...)`，guard 绑到 `_lock_guards`，**存活横跨整个工具执行的 `.await`**；
2. `AgentSpawnTool`（`agent_spawn.rs:229`）的 `#[tool(...)]` **没有覆写 `execution_mode`** → 默认 `Parallel` → `scheduler.rs:336` 先拿 `global.read_owned()`；
3. `agent_spawn.rs:597-618`——子 agent 是**同一 task 内联 `.await`**（非 spawn），且 `bus: ctx.bus().cloned()` 共享同一个 `ToolLockRegistry`；
4. 子 agent 里的 `execute_shell` 声明 `execution_mode = "serial-global"`（`execute_shell.rs:170`）→ 请求同一把锁的 `write_owned()`。

同一 task 已持 read guard、又在同一 RwLock 上等 write：write 等 read 释放，read 的持有者正被这个 write 阻塞。**死锁。** 而且 `tool-lock` 和 `sub-agents` 两个组件都是 `default_on: true`（`components.rs:144,169`）——这不是边缘配置，是默认路径。放大因素：热路径用的是 `acquire` 而非已存在但无人调用的 `acquire_with_timeout`（`scheduler.rs:393`）；KILL-1b 把 900s 子 scope 超时也废了，所以连自愈都没有。

### PR 系列方案（按依赖序，小步提交）

**第一波：止血（1-2 周内全部可完成）**

| PR | 内容 | 关键改动 | 必须带的测试 |
|---|---|---|---|
| **PR-1** `fix(kernel-abi/app-core): coordinator execution mode breaks spawn deadlock` | kernel-abi 新增 `ExecutionMode::Coordinator`（不获取任何锁——语义：该工具本身只编排、由内部工具各自持锁）；`AgentSpawnTool` 声明之 | `scheduler.rs`（新分支）、`alva-macros`（接受 `"coordinator"`）、`agent_spawn.rs` | **死锁回归测试**：mock model 编排"父调 agent 工具→子调 execute_shell"，整体包 `tokio::time::timeout(30s)`——修复前必挂、修复后必过。用现成的 `build_agent_with_responses` 模式即可 |
| **PR-2** `fix(app-core): propagate parent cancel + real sleeper into spawned sub-agents` | `agent_spawn.rs:609` 改为从 `ctx.cancel_token()` 派生 child token；`sleeper: None` 改注入真实 sleeper 使子超时生效 | `agent_spawn.rs` | 取消父后断言子在 N 轮内停；子超时生效测试 |
| **PR-3** `fix(kernel-abi): atomic seq+append in InMemorySession` | seq 分配与 events/messages 写入放进同一临界区；**同时修掉 `concurrent_append_preserves_monotonic_seq` 里 sort-before-assert 的假断言** | `agent_session.rs:671-679,1136` | 并发 append 断言真实存储顺序 == seq 顺序 |
| **PR-4** `fix(tauri): atomic record_usage via SQL UPDATE` | `SqliteSessionRegistry` override `record_usage`/`record_active_ms` 为 `UPDATE ... SET x = x + ?` | `sqlite_session/registry.rs` | 并发 100 次 record，总数精确 |
| **PR-5** `fix(builtin): poison-safe locks` | 16 处 `.lock().unwrap()` → `.unwrap_or_else(\|e\| e.into_inner())`（对齐 app-core 已有的 18 处防御式写法）+ `base_agent/builder.rs:262` 同修 | `services/task.rs`、`services/team.rs`、`read_url.rs` | 机械改动，现有测试护航 |

**第二波：修护栏（与第一波可并行）**

| PR | 内容 |
|---|---|
| **PR-6** `fix(ci): repair dependency firewall` | 删 Rule 12/16 两条死规则；Rule 17 加 `--all-features`；grep 过滤器补上 `alva-kernel-\|alva-host-\|alva-llm-\|alva-macros`；把 `alva-app-gateway` 纳入规则。**注意：`--all-features` 会立刻暴露 builtin→browser 违规**，所以本 PR 需连带把 `BrowserPlugin` wrapper 从 `alva-agent-extension-builtin` 挪进 `alva-app-extension-browser` 自己，SDK crate 删掉这条 optional dep |
| **PR-7** `feat(ci): GitHub Actions pipeline` | fmt + clippy + `cargo test`（按 crate 分 job，tauri/browser 叶子隔离）+ `ci-check-deps.sh` + wasm check + `mock_capability_suite`。加 `cargo-llvm-cov` 出基线报告（先不设门槛，只可见化） |
| **PR-8** `chore: delete dead assembly paths` | 删 `AgentRuntimeBuilder`/`AgentRuntime`/`with_standard_agent_stack` + `runtime_basic.rs` example + `register_builtin_tools` shim。engine 3 crate 先从 workspace members 摘除、代码移入 `attic/` 分支 |

**第三波：结构收敛（每个都是独立里程碑）**

| PR | 内容 |
|---|---|
| **PR-9** `refactor(security/app): single permission-decision parser` | `parse_decision`（Tauri）与 `parse_approval_input`（CLI）合并为 `alva-agent-security` 里的一个函数，统一词汇表和 fallback 策略（建议：未知输入→显式 Err），CLI 12 个既有测试迁移护航 |
| **PR-10** `refactor(app-core): single provider factory` | 5 份 `match kind → Arc::new(Provider)` 收敛到 `registry.rs:64` 的 canonical 版本；默认 base_url 表只留一份 |
| **PR-11** `refactor(kernel): move session backends out of ABI` | `agent_session.rs` 里 InMemorySession 两后端 + listener/live-tail 子系统（~380 行，`520-1010`）迁往 `kernel-core`；ABI 回归纯 trait+值类型。同 PR 修 D-3 listener 泄漏（subscribe 返回 guard，drop 时反注册） |
| **PR-12** `refactor(context): unify compaction` | 分阶段：先让 `CompactionMiddleware` 内部委托 hook 路径的 `compact.rs` 实现（消灭逻辑双份），再决定 middleware 壳是否保留为纯接线层。建议单独写设计文档 |
| **PR-13** `refactor(tauri): split agent.rs + fix D-4/D-7` | 1954 行 God-file 按职责拆 ~9 模块；`send_message` 的 swap+prompt 加临界区；`respond_approval` 先校验后摘 pending |

## 四、3 年迁移路线图

**现在 → 3 个月：止血与护栏**（第一、二波 PR）
退出标准：死锁回归测试在 CI 常绿；`--all-features` 下 Rule 17 通过；每个 PR 自动跑 fmt/clippy/test/mock-suite；llvm-cov 基线可见。

**3 → 12 个月：收敛与瘦身**（第三波 PR + ARCHITECTURE.md 自己点名的压力点）
- compaction 归一、ABI 提纯、Tauri/CLI 孪生去重全部落地；
- 启动 `alva-app-core` 拆分（16 个 workspace 依赖的"上帝装配器"；ARCHITECTURE.md 已列为 "main future split candidate"）：facade / component catalog / app-plugins / session projection 至少拆出 session 一块；
- **engine 层裁决**：3 个 crate 零消费者。要么 Tauri/CLI 真的接上 `EngineRuntime`（如果多后端是真实产品需求），要么删。给 6 个月缓刑期，到期无消费者即出仓；
- `outcome.rs` 同理：EvaluationPlugin 接线或删除；
- 文档修复纳入 PR 模板检查项（改了 crate 结构必须动 AGENTS.md）。

**第 2 年：SDK 1.0 与生态**
- SDK 层（kernel-* + agent-core + 能力库）宣布 semver 稳定，破坏性变更走 RFC；ABI 提纯（PR-11）**必须赶在 1.0 前完成**——`agent_session.rs` 里的行为实现一旦被第三方依赖就再也改不动了；
- AEP loader + Python/JS SDK 从"能用"到"有第三方真用"：补协议一致性测试套件；
- bus 治理复检：若 caps 逼近 20，按 BUS-RULES 自己的退役条款升级治理（分域 bus 或 cap 分组）；
- 覆盖率从"可见"升级为"门槛"（新增代码行覆盖 ≥ 70% 的 ratchet 规则，只升不降）。

**第 3 年：结构性拆分决策点**
- "三仓库（alva-sandbox / alva-agent / alva-app）"旧愿景 vs 现实单仓——触发条件是 SDK 有了独立发版节奏的外部消费者。有 → SDK 出仓独立发布；没有 → 保持单仓，删掉文档里的三仓叙事；
- wasm host 从"编译期探针"到真实产品面（如果浏览器端 agent 是方向）。

## 五、CLI 测试方案：具体指导意见

### 现状一句话

431 个文件内单测把**纯逻辑单元**（命令、markdown、vim mode、picker、审批解析）测得很厚，但**三个真正的交互面零覆盖**：`run_print_mode`（headless 主循环）、`TuiApp`（1668 行 TUI）、`repl.rs`（750 行 REPL 循环）。dev-deps 只有 `tempfile`——连 `alva-test` 都没引。风险和覆盖恰好互补错位。

### 怎么改：四层方案，按 ROI 排序

**第 1 层（零基建成本）：TuiApp 状态机测试**
`TuiApp` 已经是无终端依赖的纯状态机——`on_key(KeyEvent) -> KeyAction`（`app.rs:455`）和 `handle_agent_event`（`app.rs:1305`）只改内存状态。直接写：构造 `TuiApp::new()` → 喂 `AgentEvent` 序列（TextDelta 流、tool 执行、审批请求、MessageEnd）→ 断言对话状态/streaming 缓冲/审批队列。**不需要任何重构**，这是全 CLI 最便宜的高价值覆盖。

**第 2 层（一个小重构解锁）：`run_print_mode`/`run_prompt` 进程内集成测试**
两个函数已经接收 `&BaseAgent`——种子很好，缺两样：
1. `alva-app-cli/Cargo.toml` 加 dev-deps：`alva-test`、`futures`；
2. **输出 writer seam**：`event_handler.rs` 和 `output.rs` 目前硬编码 `print!`/stdout（`event_handler.rs:100-109`）。把签名改成收 `&mut dyn Write`（生产传 `io::stdout().lock()`），这是唯一必要的重构。

然后照抄 `alva-app-core/tests/e2e_tool_coverage.rs:37` 的 `build_agent_with_responses` 模式：`MockLanguageModel::with_response(...)` 排脚本 → 直接构建 `BaseAgent`（绕过 `agent_setup::build_agent`，避开真实 provider 构造）→ 调 `run_print_mode` → 断言 writer 内容 + 返回码。重点用例：多轮 tool 执行时 stdout 只出 TextDelta、`AgentEnd{error}` 退出码 1、审批 fail-closed 拒绝有 stderr 提示、token 计数正确。

**第 3 层（真实契约层）：spawned-binary 端到端测试**
`-p` 模式的**真正契约**是 stdin→stdout→exit-code + 权限模式语义，只有跑真二进制才能覆盖 `main.rs` 里的 config 解析、`--permission-mode` 校验、非 TTY 分支。需要两块基建：
1. **config 隔离**：`alva-app-core/src/config.rs:81` 硬读 `dirs::home_dir()/.alva/config.json`，无覆写。加 `ALVA_CONFIG_DIR` 环境变量覆写；
2. **假 provider**：起一个 wiremock 假 OpenAI-compatible server，测试 config 的 `base_url` 指过去。

然后 `tests/print_mode.rs` 用 `assert_cmd`：
- `alva -p "hi"` → 0 + 正确 stdout；
- 空 prompt → 1；无 config 且非 TTY → 1；
- `--permission-mode ask` 下模型请求 shell → 拒绝提示不挂起（回归 fail-closed 行为）；
- `accept-shell` 无沙箱 → 拒绝启动（`main.rs:203-213`）。
这一层每个用例贵（编译+进程），**控制在 10~15 个黄金用例**，只测契约不测细节。

**第 4 层（先重构后测试）：REPL**
`repl.rs:231-520` 的 slash 分发内联在 `read_line` 循环里、耦合 reedline——当前不可测。做法：把分发逻辑提取成 `fn dispatch(line: &str, ctx: &mut ReplCtx) -> ReplAction`（参照已有的 `CommandRegistry::execute` 模式），循环体只剩"读行→dispatch→执行 action"。提取后分发逻辑按第 1 层方式测；循环本体留 2-3 个 pty 冒烟测试或干脆不测（薄壳）。

### 配套动作

- **把三处复制粘贴的 e2e harness 沉淀进 `alva-test`**：`collect_events` / `tool_use_message` / `build_agent_with_responses` / 自动审批 spawn 循环，目前在 app-core 三个测试文件里各存一份，CLI 一加入就是第四份。先抽壳再写 CLI 测试。
- **Model Eval Suite 保持原位不动**。它测"模型会不会用工具"（app-core 层），CLI 测试测"二进制的 I/O 契约"——互补层级，不要合并。
- **顺手修 `alva-app-debug` 的 21 处 `thread::sleep(100ms)` + 硬编码端口 19230-19245**——改端口 0 + 就绪轮询，否则它会是上 CI 后第一个 flaky 源。
- 执行顺序：TuiApp 测试（第 1 层）→ writer seam + 进程内测试（第 2 层）→ `ALVA_CONFIG_DIR` + 黄金用例（第 3 层）→ REPL 提取（第 4 层）。第 1、2 层合计约 2-3 天工作量。

---

## 附录：诊断方法与证据等级

- 4 路并行探查：架构健康（依赖图/Rule 17/bus/legacy 路径）、高危遗留（session/并发/全局状态/unwrap）、CLI 深挖（模块地图/测试盘点/-p 契约/可测性障碍）、测试与 CI 基建。
- **已逐行人工复核（最高证据等级）**：KILL-1 死锁四环节链路、KILL-1b 断连 token、ci-check-deps.sh 三处盲区（Rule 12/16 死规则、Rule 17 无 --all-features）、execute_shell serial-global 声明、AgentSpawnTool 无 execution_mode 覆写。
- 其余发现来自探查报告，均带 file:line 引用，行号对应 main @ fe4559c。
