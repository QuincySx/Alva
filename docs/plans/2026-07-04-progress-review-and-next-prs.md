# 诊断进展评审 + 修正版 PR 队列(2026-07-04)

> **日期**:2026-07-04
> **基于**:分支 `fix/subagent-spawn-deadlock-cancel` @ `3f8a881` + 工作区未提交改动
> **方法**:对 07-02 两份诊断的全部关键断言在当前 HEAD 逐行复核;两条死锁/取消回归测试实测通过(0.06s)
> **关联**:[架构诊断](./2026-07-02-arch-diagnosis-tech-debt-roadmap.md) · [wasm/sandbox 可行性](./2026-07-02-subagent-wasm-sandbox-feasibility.md)

---

## 一、进展记分卡

| 项 | 状态 | 当前 HEAD 证据 |
|---|---|---|
| **PR-1** Coordinator 破死锁 | ✅ 完整落地 | `2ede227`:scheduler 零锁分支+空 ticket、宏支持 `"coordinator"`、回归测试断言完整 4 轮链路,实测通过 |
| **PR-2** cancel + sleeper | 🟡 半完成 | cancel ✅(`3f8a881`,watch-channel latch 覆盖竞态);**sleeper ❌**:`agent_spawn.rs:624` 仍 `sleeper: None`,`run_child.rs:46-50` 明确 None→NoopSleeper→超时永不触发 |
| **PR-3** seq 原子性 | ❌ 未动 | `agent_session.rs` `append()` 的 `fetch_add` 在 write lock 外;`:1159` 测试仍 sort-before-assert |
| **PR-4** SQLite 原子计数 | ❌ 未动(**紧迫度下调**) | usage 走通用 `update()` 的 JSON-blob read-modify-write(`registry.rs:228,239-281`);但 SELECT+UPDATE 全程持同一 `conn.lock()`(`:238`),**单进程内实际不丢更新**——只有跨进程/多实例共库才会。`UPDATE x=x+?` 仍是正确终态,但可排后 |
| **PR-5** poison 锁 | ❌ 基本未动 | extension-builtin 仍 12 处裸 `.lock().unwrap()`(原记 16) |
| **PR-6** CI 防火墙 | ❌ 未动 | `ci-check-deps.sh:71` 仍查不存在的 `alva-provider`、`:96` 查不存在的 `alva-app`,Rule 17 无 `--all-features` |
| **PR-7** 托管 CI | ❌ 未动 | 无 `.github/workflows/` |
| **PR-8** 死代码 | ❌ 未动 | `AgentRuntimeBuilder` 仍在,3 个 engine crate 仍在 workspace |
| **CLI 四层测试** | 🟡 预备工作有实质进展,四层本体未开始 | 已做的"一波":把纯 helper 从循环里抽出并测厚(全 crate 431 个单测,如 `parse_approval_input` ×10、`format_print_mode_error` ×5),repl 纯命令走 `CommandRegistry::execute`(有 10+24 个测试)——方向正确。未做:`ui/app.rs`(1668 行)零测试;`event_handler.rs:100` 仍硬编码 `print!`;dev-deps 仅 `tempfile`、无 `tests/`;无 `ALVA_CONFIG_DIR`(config 目录硬编码 `~/.alva`,`main.rs:97`);有状态 slash 命令(`/quit /clear /model /fork /resume` 等)仍内联在 `repl.rs:255-401` |
| 子 agent 空 middleware(wasm 文档问题 2) | ❌ 仍在 | `run_child.rs:96` `middleware: MiddlewareStack::new()` — 子 agent 跑危险工具不经 HITL |
| run.rs wasm panic(wasm 文档问题 1) | ❌ 仍在 | `run.rs:722` `std::time::Instant::now()`、`:833` `SystemTime::now()`(`:608` 用 web_time 对照) |

**总评:第一波 5 个 PR 完成 1.5 个,挑最致命的先做、质量高,顺序正确。**

## 二、修复质量复核(本分支)

- **PR-1 形状干净,可直接作为后续 ExecutionMode 扩展的范本**。宏报错信息同步更新是加分项。
- **PR-2 的 commit 序列会让读者以为 KILL-1b 已修完——实际墙钟超时仍是死的**。headless 场景没人按取消键,失控子 agent 依旧只能跑到 max_iterations。
- **工作区未提交改动**(timeout 提升为 `SubAgentPlugin::new(depth, timeout)` 注入参数):方向对、唯一调用方 `components.rs:335` 无破坏、测试通过。**但新注释 "enforced by the injected sleeper" 当前是假的**——必须先补 sleeper 或改注释,不能带着错误注释合并。

## 三、修正版 PR 队列(两份文档合流,按依赖序)

**本分支收尾(合并前必做):**

| PR | 内容 | 要点 |
|---|---|---|
| **PR-2b** `fix(app-core): inject real sleeper so sub-agent timeout fires` | `SubAgentPlugin` 增收 `Arc<dyn Sleeper>`,`components.rs` 装配时传 `TokioSleeper`(`alva-host-native/src/sleeper.rs:20` 现成) | ⚠️ `agent_spawn.rs` 不得直接 import host-native——spawn 核心机制是 wasm-clean 的(wasm 文档已证),经插件注入保持解耦。测试:子 sleep 5min、timeout 设 100ms,断言返回 `timed out` 错误 |

**紧随其后(新增,安全级):**

| PR | 内容 | 要点 |
|---|---|---|
| **PR-2c** `fix(kernel-core): sub-agents run the security middleware stack` | `run_child.rs:96` 空栈 → 构造最小子栈(Security + LoopDetection + ToolTimeout) | 不建议整栈继承父 middleware(部分 middleware 有 per-run 状态);最小子栈方案在未来 reverse-RPC 宿主执行方案下自然退役,不冲突。回归测试:子 agent 调 `execute_shell` 在 `ask` 模式下必须产生审批请求 |

**第一波剩余(顺序调整):** PR-3(seq 临界区 + 拆 `:1159` sort-before-assert 假断言,数据正确性优先级最高)→ PR-5(12 处裸锁:`read_url.rs:173,273`、`services/task.rs` ×5、`services/team.rs` ×5,改 `unwrap_or_else(|e| e.into_inner())`)→ PR-4(**降序**:实测单进程内被 conn mutex 串行化,只影响多实例共库场景)。

**第二波(与第一波并行):** PR-6(删死规则 + `--all-features` + 连带迁移 builtin→browser 违规)→ PR-7(GitHub Actions:fmt/clippy/test/ci-check-deps/wasm-check/mock-suite + llvm-cov 基线)。

**新增小 PR(半小时级,建议搭车第二波):**

| PR | 内容 |
|---|---|
| **PR-w1** `fix(kernel-core): web_time in run.rs hot path` | `run.rs:722/:833` 改 `web_time`(对照 `:608`);同 PR 给 CI 补一个真正在 wasm runtime 里执行 `run_stub_agent` 的冒烟测试(现有 `_wasm_smoke_probe` 只编译不执行,这正是本回归漏网的原因) |

**第三波(PR-9..13)与 3 年路线图:维持 07-02 文档原样,无需修订**——本轮核实未发现使其失效的变化。

## 四、CLI 测试:现状与执行指导

四层方案(07-02 文档第五节)**全部未开始**,方案本身经今日复核仍然成立。执行纪律:

1. **第 1 层先行(今天就能写)**:`ui/app.rs` 的 `TuiApp` 仍是纯状态机且零测试(1668 行 0 个 `cfg(test)`;入口 `on_key` `:455`、`handle_agent_event` `:1305`、`on_key_approval` `:732`)——构造 `TuiApp::new()` → 喂 `AgentEvent` 序列 → 断言状态。零重构、零新依赖。
2. **第 2 层的唯一重构是 writer seam**:`event_handler.rs:100-109` 与 `output.rs` 全部 `print_*` 改收 `&mut dyn Write`;dev-deps 补 `alva-test` + `futures`。**写 CLI 测试前先把 app-core 的重复 harness 沉淀进 `alva-test`**——实测重复度比原诊断更高:`collect_events` 已有 **4 份拷贝**(agent_capabilities/e2e_agent_test/e2e_http_test/e2e_tool_coverage),agent-builder helper 4 种分叉,`tool_use_message`/`ran_tool` 各 2 份;CLI 一加入就是第 5 份。顺手补上 `alva-test/src/lib.rs` 头注释里提到但**不存在**的 `assertions` 模块,正好作为这些断言 helper 的家。
3. **第 3 层黄金用例控制在 10-15 个**:前置一个 `ALVA_CONFIG_DIR`(或 `ALVA_HOME`)目录覆写——目前只有单值覆写(`ALVA_API_KEY`/`ALVA_MODEL`/`ALVA_BASE_URL`/`ALVA_PROVIDER_KIND`,`main.rs:108-123`),config/history/checkpoints 目录仍硬编码 `~/.alva`(`main.rs:97`、`repl.rs:118`、`checkpoint.rs:31`)三处同源,加一个覆写点三处同吃。然后 wiremock 假 provider + `assert_cmd`。
4. **第 4 层已成一半**:纯命令已走 `CommandRegistry::execute`(`repl.rs:442`,可测且已有 34 个测试);剩下的是把 `:255-401` 内联的有状态 slash 命令(`/quit /clear /setup /plan /model /fork /resume /sessions` 等)与 `!shell`(`:418`)迁进同一 dispatch 形状。
5. Model Eval Suite 保持原位——它测模型能力(`failure_owner` 归因:none/runtime/tool/model/assertion),CLI 测试测 I/O 契约,层级互补不合并。
6. **`alva-app-debug` 修的是 tests 不是 src**:20 处 `thread::sleep(100ms)` + 硬编码端口 19230-19255 都在 `tests/integration.rs`;src 干净且已支持 `.port(0)`(`builder.rs:214` 单测就在用)——把 integration 测试改成 `.port(0)` + 就绪轮询即可,半天内完成。

## 五、下一步(单句版)

**本分支补 PR-2b(sleeper)后合并 → PR-2c(子 agent 安全栈)→ PR-3 → PR-6/7 并行上 CI → CLI 第 1 层 TuiApp 测试开工。**

---

## 六、执行记录(2026-07-04 当日落地,分支 `fix/subagent-spawn-deadlock-cancel`)

全部 TDD(RED 实测失败 → GREEN),每步带回归测试:

| Commit | 内容 | 备注 |
|---|---|---|
| `5842731` | **`CancellationToken::child()`**(kernel-abi):层级 token,父→子单向传播 | PR-2b 落地时暴露的连带 bug:`run_child` 超时分支 `cancel.cancel()` 在 PR-2a 的对等克隆语义下会**把父运行一起取消**;child() 修复该交互。9 条新不变量测试 |
| `78af1d9` | **PR-2b sleeper 注入**:`SubAgentPlugin`/`AgentSpawnTool` 收 `Arc<dyn Sleeper>`,`apply_components` 传 `TokioSleeper`;900s 提升为 `ComponentContext::subagent_timeout` | 回归 `subagent_timeout_fires`:修复前子 agent 无界运行(外层超时失败),修复后 ~250ms 退出且父级第 3 轮调用携带 "timed out" 工具结果 |
| `74465cc` | **PR-2c 子 agent 安全栈**:`ChildAgentParams.middleware` + `SecurityMiddleware::from_shared`(包 bus 上父 guard 同一把 `Arc<Mutex>`,mode/allow-always 全树同步);headless 无审批处理器时 fail-closed | 回归 `subagent_dangerous_tool_goes_through_hitl`:修复前 approvals seen = [](子 shell 裸跑),修复后必产生审批请求 |
| `06f452d` | 修既有坏测试 `e2e_plan_mode_blocks_writes`(裸 builder 从未装 permission 组件,`set_permission_mode` fail-closed 后一直挂,无 CI 所以没人发现) | "CI 盲区"的活标本 |
| `b65733f` | **PR-3 seq 原子性**:4 个赋值点(InMemory/Listenable × append/append_message)全部移进 events 写锁临界区;message 路径双锁同临界区(锁序 events→messages) | 假断言测试重写为多线程 runtime + 严格存储序断言:修复前 3/3 RED,修复后 5/5 GREEN。dev-deps 加 rt-multi-thread(仅测试) |
| `766dfc1` | **PR-5 poison-safe 锁**:16 处 + builder.rs host 锁,`unwrap_or_else(\|e\| e.into_inner())` | 回归 `store_survives_a_poisoned_lock`。注意 services 模块是 feature 门控的(`--features task,team`) |
| `fadaf4d` | **PR-6 依赖防火墙修复**:Rule 12→`alva-llm-provider`、Rule 16→builtin 边界(原两条查不存在的 crate 永远假 OK)、全脚本 `--all-features -e normal`、过滤器全 `alva-*`;**BrowserPlugin 迁入 `alva-app-extension-browser`**,builtin 删 browser feature | 修好的防火墙先精确抓到 builtin→browser(RED),迁移后 PASSED 含 wasm32 集合 |
| `63fef5a` | `cargo fmt --all`(清既有漂移,让 CI 能 gate fmt) | 6 文件 |
| (本条之后) | **PR-7 GitHub Actions**:fmt(gate)+ clippy(report-only 待清仓)+ workspace 测试(除 tauri/app-debug)+ 防火墙/wasm + llvm-cov 基线(无门槛) | tauri 需系统依赖单列;app-debug 待端口 0 改造后回归 |

**尚余(按优先序)**:CLI 第 1 层 TuiApp 状态机测试(`ui/app.rs` 1668 行零测试)→ 第 2 层 writer seam + harness 沉淀 alva-test(`collect_events` 已 4 份拷贝)→ PR-w1(run.rs:722/:833 web_time,半小时)→ app-debug 集成测试去 sleep/固定端口 → PR-4(降级)→ 第三波结构收敛。
