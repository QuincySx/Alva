# 16 流契约一致性 harness + 违约处置动词

- 级别：P2
- 影响：给 pi 或 claude-print 改一条事件映射导致漏发/多发终结事件，`cargo test --workspace` 会全绿；这个回归只在真机跑该引擎时以 CLI 的一行 stderr 出现，Tauri 与 GPUI 下完全静默，用户看到的是会话永远转圈或提前收尾。
- 状态：TODO

## 先说要肯定的部分

`stream_contract` 是整个 seam 里**唯一在生产路径上有强制力**的东西，本工单不动它的设计：

- `EngineRuntime::execute` 的全仓唯一生产调用点是 `crates/alva-hub-core/src/runtime.rs:483`（其余 `.execute(` 命中全在 `#[cfg(test)]` 段内或属于 rusqlite 的同名方法）。
- `StreamContract::observe` 挂在 `crates/alva-hub-core/src/runtime.rs:517`，位于转发循环的第一条语句。

所以**每个 adapter 的每一条事件都真的过了契约**。它有骨头，缺的是牙。

## 根因

**契约测试的位置和实际会出错的位置不重合。**

## 证据

### A. 生产实现是 5 个，不是 4 个

`rg -n 'impl EngineRuntime for' crates/ --glob '*.rs'` 的生产命中：

- `crates/alva-engine-adapter-alva/src/adapter.rs:88` —— `AlvaAdapter`
- `crates/alva-engine-adapter-claude/src/adapter.rs:234` —— `ClaudeAdapter`
- `crates/alva-engine-adapter-claude/src/print_adapter.rs:92` —— **`ClaudePrintAdapter`**（Hub 产品在 `"claude"` 这个 id 下实际构建的就是它，见 `crates/alva-hub-desktop/src/alva.rs:97-100`）
- `crates/alva-engine-adapter-codex/src/adapter.rs:55` —— `CodexAdapter`
- `crates/alva-engine-adapter-pi/src/adapter.rs:43` —— `PiAdapter`

其余 22 处命中全部落在 `#[cfg(test)]` 内（hub-core / hub-desktop / hub-gpui 的测试替身）。

### B. 契约断言的分布严重倾斜

`rg -c 'validate_event_stream|check_soft' <file>` 实测：

| 文件 | 命中行数 |
|---|---|
| `crates/alva-engine-adapter-alva/src/adapter.rs` | 3（1 处 `use` + 2 条断言） |
| `crates/alva-engine-adapter-alva/src/mapping.rs` | 2（1 `use` + 1 断言） |
| `crates/alva-engine-adapter-claude/src/mapping.rs` | 6（3 `use` + 3 断言） |
| `crates/alva-engine-adapter-codex/src/mapping.rs` | 4（2 `use` + 2 断言） |
| `crates/alva-engine-adapter-claude/src/adapter.rs` | **0** |
| `crates/alva-engine-adapter-claude/src/print_adapter.rs` | **0** |
| `crates/alva-engine-adapter-codex/src/adapter.rs` | **0** |
| `crates/alva-engine-adapter-pi/src/adapter.rs` | **0** |
| `crates/alva-engine-adapter-pi/src/mapping.rs` | **0** |

**pi 与 claude-print 两个生产 adapter 零契约断言。** 更进一步：`crates/alva-engine-adapter-pi/src/adapter.rs` 里 `rg -n '#\[cfg\(test\)\]'` 零命中 —— 该文件**没有测试模块**，整个 pi crate 的测试只在 `mapping.rs:206` 与 `process.rs:186` 两处。

### C. 有断言的那几处只测 mapper，测不到真正会出错的地方

- `crates/alva-engine-adapter-claude/src/mapping.rs:713,761,816` 与 `crates/alva-engine-adapter-codex/src/mapping.rs:276,332` 的测试都是「构造 `EventMapper` → 手喂 vendor 帧 → 收集 `Vec<RuntimeEvent>` → `validate_event_stream`」。它们验证的是**映射函数**。
- 而漏发 `Completed`、fatal 之后继续发事件这类违约，发生在 adapter 的 `tokio::select!` 分支与早退路径上：`crates/alva-engine-adapter-pi/src/adapter.rs:82`、`crates/alva-engine-adapter-claude/src/adapter.rs:302`、`crates/alva-engine-adapter-claude/src/print_adapter.rs:131`、`crates/alva-engine-adapter-codex/src/adapter.rs:100`、`crates/alva-engine-adapter-alva/src/adapter.rs:276`；以及 spawn 失败的早退分支（如 `crates/alva-engine-adapter-pi/src/adapter.rs:64-76`）。
- **只有 alva 一个 adapter 在 `execute()` 端到端跑过契约**：`crates/alva-engine-adapter-alva/src/adapter.rs:559`（首轮）与 `:633`（resume 轮），断言的输入是 `adapter.execute(..).collect().await` 的结果。

### D. 缺的不是机制，是接线

`crates/alva-engine-adapter-claude/src/print_adapter.rs:337` 起的测试模块已经有完整的 fixture 进程机制：`fake_claude(&executable, "<shell 脚本>")` 写一个假 CLI，再 `adapter.execute(RuntimeRequest::new("test").with_cwd(..))` 端到端跑（`:403,452,488,496`）。其中 `:388` 的 `cancel_emits_one_fatal_error_then_completed` **正是「中途 kill」剧本**，但它手写断言而不是把事件序列喂给契约。**器材齐了，只差把契约挂上去。**

### E. 违约计数器指向一个不存在的动作

- `crates/alva-hub-core/src/runtime.rs:510-513` 的注释明说取舍：「Hard violations are logged; Hub still forwards events so adapters stay debuggable」。**这个取舍是对的，本工单不推翻它。**
- 但计数结果只有一个消费者：`rg -n 'stream_hard_violations' crates/ --glob '*.rs'` 的产品侧命中只有 `crates/alva-hub-cli/src/main.rs:867-878`，打一行 stderr。desktop / gpui / server **零引用**。
- 所以 `crates/alva-hub-core/src/runtime.rs:723-737` 的 `record_hard_violation` 在旗舰产品上是一个写进去就没人读的字段 —— **计数器指向一个不存在的动作**。

## 复现

1. 在 `crates/alva-engine-adapter-pi/src/adapter.rs:120`（`Ok(None) => break` 那一支）临时改成 `Ok(None) => return`，从而跳过 `:141-145` 合成 `Completed` 的那段。这模拟「给 pi 加一条新映射时不慎绕过了终结路径」。
2. 跑 `cargo test --workspace`。
3. 观察到：**全绿**。pi crate 的两个测试模块（`mapping.rs` / `process.rs`）都不经过 `execute()`。
4. 期望：至少一条测试红。
5. 补充观察：把同一个改动放到真机上跑一轮 pi，`alva-hub` CLI 会打印 `alva-hub: stream contract hard violations: 1`（`crates/alva-hub-cli/src/main.rs:867-878`）；同一轮在 Tauri 或 GPUI 下**没有任何提示**（证据 E）。

## 工作

### 第一批 —— 在 `alva-engine-runtime` 导出 conformance harness

- `crates/alva-engine-runtime/Cargo.toml` 新增 `[features] test-support = []`。
- 新增 `crates/alva-engine-runtime/src/conformance.rs`，`#[cfg(feature = "test-support")]` 导出：
  ```rust
  pub enum Scenario { Normal, SpawnFailure, KilledMidRun, EofWithoutResult, OutputAfterFatal }
  pub async fn assert_conforms<E: EngineRuntime>(engine: &E, request: RuntimeRequest, scenario: Scenario);
  ```
  内部：收集 `execute()` 的完整事件序列 → `validate_event_stream` → `StreamContract::check_soft` → 再加每个剧本自己的额外不变量（例如 `SpawnFailure` 必须恰好是 `Error{recoverable:false}` + `Completed{result:None}` 两条）。
- 五个标准剧本对应 `crates/alva-engine-runtime/src/AGENTS.md` 约束段已经写死的规则，不新增语义。

### 第二批 —— 五个 adapter 各接一遍

- `claude-print`：直接复用 `print_adapter.rs:337` 起已有的 `fake_claude` fixture，把 `:388,437,473` 三条测试改为经由 harness 断言，并补齐缺的剧本。**这一批成本最低，先做。**
- `codex` / `claude`：照 `fake_claude` 的形状加各自的 fixture 脚本。
- `pi`：`crates/alva-engine-adapter-pi/src/adapter.rs` 从零建 `#[cfg(test)]` 模块 —— 这是覆盖缺口最大的一个。
- `alva`：把 `adapter.rs:559,633` 两条既有断言改为调 harness，并补齐五个剧本中缺的那几个。

### 第三批 —— 在 AGENTS.md 写死约束

- 在 `crates/alva-engine-runtime/src/AGENTS.md` 的「约束」段增加一条：**新增 `impl EngineRuntime` 必须附带该 conformance harness 的实例化，五个剧本一个不少。**
- 同步在业务域清单表加一行 `conformance | conformance.rs | 五剧本一致性 harness（feature = test-support）`。

### 第四批 —— 给违约计数器接一个真实动词

- 在 `alva-hub-core` 增加 quarantine：单次运行的 `hard_violations` 超阈值时，把该 engine 标记为 quarantined，并从 fallback 链中剔除。
- 挂到 `docs/adr/0002-reversible-multi-engine-fallback.md` 已有的 `EngineTarget` / `FallbackPolicy` 语义上（`:23`「Each HubSession stores an ordered, user-authorized FallbackPolicy snapshot」、`:38`「An EngineTarget is tried at most once automatically per attempt cycle」）。
- **保持 `:510-513` 的转发取舍不变**：违约事件照样转发给产品，quarantine 只影响后续路由选择。
- 阈值与解除条件写进 ADR-0002 新增小节。

## 验收

- [ ] `rg -n 'test-support' crates/alva-engine-runtime/Cargo.toml` 有命中，且 `cargo build -p alva-engine-runtime` 默认不编译 `conformance.rs`。
- [ ] 五个 adapter crate 各自的 `cargo test -p <crate>` 中，都能看到五个剧本对应的测试名（例如 `conformance_normal` / `conformance_spawn_failure` / `conformance_killed_mid_run` / `conformance_eof_without_result` / `conformance_output_after_fatal`），共 25 条。
- [ ] `rg -c 'assert_conforms' crates/alva-engine-adapter-pi/src/adapter.rs crates/alva-engine-adapter-claude/src/print_adapter.rs` 两个文件都 ≥ 5（今天分别是 0 和 0）。
- [ ] **变异验证**：按「复现」第 1 步把 `pi/src/adapter.rs` 的 `Ok(None) => break` 改成 `return`，`cargo test -p alva-engine-adapter-pi` **必须红**；改回后转绿。这一条是本工单是否真的有效的唯一判据。
- [ ] `crates/alva-engine-runtime/src/AGENTS.md` 约束段含「新增 `impl EngineRuntime` 必须附带 conformance harness 实例化」字样。
- [ ] 第四批：新测试 `engine_is_quarantined_after_repeated_hard_violations`（用 `crates/alva-hub-core/src/runtime.rs:1619` 的 `ContractBreakingEngine`）通过；且同一测试断言违约事件**仍被转发**给产品（不回退 `:510-513` 的取舍）。
- [ ] `docs/adr/0002-reversible-multi-engine-fallback.md` 含 quarantine 的阈值与解除条件。
- [ ] `cargo test --workspace` 全绿；`bash scripts/ci-check-deps.sh` 全绿（`test-support` 是 dev-only feature，不得引入新的 workspace 依赖）。

## 依赖与切分

- **无前置。可随时开工，且越早越好** —— 它是 01 / 02 两张 P0 修完之后接住回归的网。本工单与 13 / 14 / 15 在文件层面零重叠（13/14 动 `hub-core/runtime.rs`，15 动产品层与 `engine-runtime` 的新文件，16 动 adapter 的测试段 + `engine-runtime` 的新 feature）。
- **对 15 的正向作用**：第二批一旦落地，15 第二批「四个 adapter 各自导出 descriptor」的改动就有了回归网。
- **切分点**：
  1. 第一批（harness 本体 + 五剧本定义）独立可合，零调用者；
  2. 第二批按 adapter 切五个 commit，**顺序建议 claude-print（复用现成 fixture，最便宜）→ codex → claude → alva（改写已有断言）→ pi（从零建测试模块，最贵但收益最大）**；每个 commit 独立可验证，中途停下也是净收益；
  3. 第三批一行文档，跟在第二批任一 commit 后；
  4. 第四批（quarantine）与前三批解耦，可独立排期 —— 它需要 ADR-0002 拍板阈值，不应阻塞 harness 落地。
