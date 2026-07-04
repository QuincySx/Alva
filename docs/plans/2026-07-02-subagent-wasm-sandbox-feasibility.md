# 子 Agent 跨环境（wasm VM / Sandbox）可行性调研报告

> **日期**：2026-07-02
> **基于提交**：`fe4559c14bc791b72c990b6a3baa927f869b069b`（main，短 hash `fe4559c`）
> **调研方法**：3 路并行探查（wasm 可移植性 / 子 agent 跨环境宿主 / alva-sandbox
> 姊妹仓评估） + 对三个承重结论的逐行人工复核
> **状态**：待评审
> **关联文档**：[2026-07-02-arch-diagnosis-tech-debt-roadmap.md](./2026-07-02-arch-diagnosis-tech-debt-roadmap.md)

---

## 目录

- [TL;DR](#tldr)
- [一、可移植性地图（哪部分是可以的）](#一可移植性地图哪部分是可以的)
- [二、跨进程边界图（子 agent 放进沙箱/VM 时，什么能过界）](#二跨进程边界图子-agent-放进沙箱vm-时什么能过界)
- [三、实现起来的问题清单（按严重度）](#三实现起来的问题清单按严重度)
- [四、alva-sandbox 姊妹仓评估 + 拆库建议](#四alva-sandbox-姊妹仓评估--拆库建议)
- [五、待拍板的三个问题](#五待拍板的三个问题)
- [附录：诊断方法与证据等级](#附录诊断方法与证据等级)

---

## TL;DR

**方向是通的，而且比预期近——但有三个"以为有、其实没有"的坑。** 编译层面 SDK 内核已经很干净：10 个 SDK crate 实测通过 wasm32 编译，`WasmAgent` facade 能装配出最小 agent；spawn 请求侧（SpawnInput/模板/scope 配置）已经全部可序列化。真正的差距在：**① kernel 主循环里有一处漏网的 `std::time::Instant::now()`，任何 wasm agent 第一轮就 panic，CI 只编译不执行所以从没发现（已亲自证实）；② 子 agent 跑在空 middleware 栈上，SecurityMiddleware 根本不在子循环里——今天的子 agent 调危险工具就绕过 HITL 闸门（已亲自证实，与跨环境无关，本身就该修）；③ 主仓的 Seatbelt 沙箱是死代码——profile 生成了但从未施加到任何命令上。** alva-sandbox 姊妹仓设计合理但还是 PoC 骨架；拆两库的依赖方向成立、无环。

## 一、可移植性地图（哪部分是可以的）

```
              子 Agent SDK 跨环境可移植性地图
    ✅ 今天就能进   🔧 小改能进   🏗 需重构   ⛔ 不该进 / 永不进

═══════════════ L0-L2.5 SDK 内核 ═══════════════════════════════════════
 alva-kernel-bus / alva-llm-wire          ✅ 零依赖地基，实测 wasm 编译过
 alva-kernel-abi                          ✅（ToolLockRegistry 计时处 🔧）
 alva-kernel-core                         🔧 唯一阻断: run.rs:722/:833
                                             std::time → 换 web_time 即通
 alva-agent-core                          ✅
═══════════════ L3 能力库 ═══════════════════════════════════════════════
 alva-agent-{memory,security,graph}       ✅ 实测编译过
 alva-agent-context                       ✅ 编译过（wasm 版 ContextHandle 🏗）
═══════════════ L4 工具层 ═══════════════════════════════════════════════
 计划/任务/团队/config/skill/tool_search 工具  ✅ 已在 wasm gate 内
 文件工具 read/create/edit/list           🔧 已走 ToolFs，只因 LocalToolFs
                                             fallback 被 gate；去掉即可注入
                                             IndexedDbToolFs
 grep_search / find_files                 🏗 绕过 ToolFs 直用 std::fs+walkdir，
                                             且 ToolFs 缺递归 walk
 execute_shell / browser / worktree       ⛔ 语义上属于宿主/沙箱进程
═══════════════ 协议层 ═══════════════════════════════════════════════════
 alva-llm-provider                        🔧 实测编译过(reqwest→fetch)，
                                             但 SSE 流式未验证、dirs 配置失效
 alva-protocol-skill                      🔧 fs 发现可选化
 alva-protocol-mcp                        🏗 wasm 下无 transport（stdio only）
 alva-protocol-acp                        ⛔ wasm 下编译成空壳（设计即 native）
═══════════════ 装配层 ═══════════════════════════════════════════════════
 alva-host-wasm (WasmAgent)               ✅ 骨架在：WasmSleeper/InMemorySession/
                                             ToolTimeout；缺真 model（只有 Stub）
 agent_spawn（子 agent spawn 逻辑）        🔧 核心机制全建立在 wasm-clean crate 上！
                                             唯一 host 耦合 = 11 行纯函数
                                             alva_host_native::model，挪进 SDK 即解绑
 alva-app-core / host-native / 重外挂      ⛔ rusqlite(C)/tokio full/子进程，
                                             本来就不该进
```

**最重要的架构好消息**（wasm 探查实测得出）：`agent_spawn.rs` 的核心机制——`run_child_agent`、`SpawnScopeImpl`、`ContextHooksChain`、session 转发、深度限制——**已经全部建立在 wasm-clean 的 SDK crate 上**。它住在 app-core 只是历史位置，下沉的全部工作量 = 挪一个 11 行的纯函数（`alva-host-native/src/init.rs:15-26`）+ 把模板路径改成可选。

## 二、跨进程边界图（子 agent 放进沙箱/VM 时，什么能过界）

```
 父 agent（宿主进程）              ║ 进程/VM 边界 ║        子 agent（沙箱/wasm）
                                  ║             ║
 SpawnInput(task/role/tools名单)──╫─✅ 已可序列化─╫─→ spawn 请求          【现成】
 AgentTemplate(含模型凭证配置) ────╫─✅ 已可序列化─╫─→ 子侧自建 provider   【现成】
                                  ║             ║
 Arc<dyn Tool> 工具集合 ──────────╫─⛔ 活对象────╫─→ ★攻坚点：reverse-RPC
                                  ║             ║    回宿主执行 或 子侧重建
 BusHandle(SecurityGuard/锁/审批)─╫─⛔ 活对象────╫─→ 逐能力 RPC 代理
 ForwardToSession 直写父 session ─╫─🏗──────────╫─→ SessionEvent 序列化流回
 CancellationToken(靠future-drop)─╫─🏗──────────╫─→ 显式 cancel RPC
 Arc<Blackboard> @mention 协作 ───╫─🏗──────────╫─→ board 服务化(按scope_id寻址)
 ContextHooks chain ──────────────╫─⛔ 活对象────╫─→ 留宿主侧执行
```

**已有三个进程外先例，各占一角、拼起来正好**：

| 先例 | 有什么 | 缺什么 |
|---|---|---|
| `EngineRuntime` trait（目前零消费者） | **消费侧接口形状完全正确**：`execute→事件流 / cancel / respond_permission / capabilities`，claude adapter 已证明同一 trait 能跨进程 | 无父子嵌套维度；`RuntimeRequest` 没派生 Serialize |
| AEP loader（`proxy.rs`/`dispatcher.rs`） | **唯一成熟的双向 stdio transport**：pending-map ID 关联、双向 request、graceful-kill | `host/request_approval` 是 stub（"Phase 6"）；事件无流式、5s 硬超时 |
| ACP 协议 crate（已 deprecated） | **消息词汇最全**：13 入站/5 出站事件 + BootstrapPayload + RequestPermission/PermissionResponse，纯 serde | transport 绑 stdio、未接线、作者已弃用——只当类型库抄 |

**最短路径**：第一刀是纯重构——写一个 in-process `EngineRuntime` adapter 包住 `run_child_agent`，把 `agent` 工具从直调改为走 `EngineRuntime` trait。行为不变、可回归验证，但从此换任何 out-of-process/wasm adapter 都不用再动 `agent_spawn.rs`。这也顺手给死代码 engine 层找到了真实存在理由（否则按 3 年路线图它 6 个月后就该删）。

## 三、实现起来的问题清单（按严重度）

1. 🔴 **`run.rs:722` 无条件 `std::time::Instant::now()`**（已亲自证实；`:608` 就在用 `web_time`，这是回归）——最小 wasm agent 第一轮 LLM 调用就 panic。`:833` SystemTime 同类。**修复约半小时**，但暴露了更大的问题：CI 的 wasm 检查是纯编译探针（`_wasm_smoke_probe` 永不执行），需要补一个真正在 wasm runtime 里执行 `run_stub_agent` 的冒烟测试。
2. 🔴 **子 agent 空 middleware 栈**（已亲自证实，`run_child.rs:96`）——SecurityMiddleware/PlanMode/LoopDetection/ToolTimeout 在子循环全不跑，子 agent 调 `execute_shell` 等危险工具**不经 HITL 审批**。这与跨环境无关，是当下的安全缺口。值得注意：如果 tool 递交选"宿主侧执行"方案，这个缺口在跨进程版天然不复现（工具在宿主执行必过闸门）。
3. 🔴 **Seatbelt 是"生成但不施加"的死代码**——`wrap_command()` 全仓无非测试调用者；CLI `main.rs:203` 的 `is_enforced()`（= `cfg!(macos)`）基于"macOS 有沙箱"这个不成立的前提放行 `accept-shell`/`bypass`。`execute_shell` 和 `local_fs` 的 `sh -c` 实际都是裸跑。
4. 🟠 与架构诊断报告的 KILL-1/KILL-1b 同源：spawn 路径持全局读锁 + 断连 cancel token + `sleeper:None` 使 900s 子超时失效——跨环境调度做之前这三个必须先修，否则把死锁和僵尸子 agent 一起带进沙箱。
5. 🟠 wasm LLM provider 只有 Stub；reqwest→fetch 能编译但 SSE 流式未经验证、`dirs` 在 wasm 返回 None 导致配置加载静默失效（key 必须 caller 显式传入）。
6. 🟡 ToolFs 只有 5 个操作、没有递归 walk；`grep_search`/`find_files` 绕过它——文件系统虚拟化 seam 有了但不完整。
7. 🟡 文档漂移追加一条：AGENTS.md 里的 `SessionTracker` 在代码里不存在（spawn 树父子关系实际由 `BoardRegistry.parents` 承载）。

## 四、alva-sandbox 姊妹仓评估 + 拆库建议

**现状**：单 commit（2026-03-24），935 行，工作树干净。三层抽象（`Sandbox` 实例 / `SandboxAdapter` 后端 / `SandboxProvider` 工厂）+ 7 档能力模型 + env 策略/skill 注入，为 Docker/E2B/云预留了 config 字段——**设计是得体的**。但 local 后端是裸 `sh -c` 零隔离，`get/list/destroy` 全是 stub，能力 trait 零实现者，还有一个 `~` 不展开的 skill 注入 bug。定位：**PoC 级抽象 + demo 后端**，不是生产件。

**拆两库成立吗？成立，且无环**：正确方向是 `alva-agent → alva-sandbox`（agent 的执行点面向 `Sandbox` trait），反向绝不可以。真正的风险不是编译环而是**概念环**：主仓已有三套互不相通的"sandbox"语义（Seatbelt SandboxConfig / claude adapter 的 bool / ACP SandboxLevel），alva-sandbox 若引入第四套而不收敛，只会更乱。

**综合建议（三步）**：
1. **让 alva-sandbox 成为唯一的执行抽象**，主仓三套沙箱概念收敛到它之下；Seatbelt 代码迁入 `alva-sandbox-local` 作为 enforce 分支，并**真正接线到 `Command::new`**——这同时修掉问题 3（把"生成但不施加"升级为真实隔离），也让 CLI 的 `is_enforced()` 门禁改为查询 `provider.capabilities()` 而非 cfg!(macos) 硬编码。
2. **关键发现**：alva-sandbox 的 `Sandbox` trait（exec/write_file/read_file/list_dir/exists）与 kernel-abi 的 `ToolFs`（exec/read_file/write_file/list_dir/exists）**几乎同构**。这不是巧合——它们是同一个边界的两侧。做一个 `SandboxToolFs` adapter，agent 的文件/shell 工具面向 ToolFs，ToolFs 背后是任意 Sandbox 后端（local+Seatbelt / Docker / wasm 宿主回调）——两个仓库就此接上，且不需要 alva-sandbox 知道任何 agent 概念。
3. 拆库时机：等第 1、2 步在主仓内验证过一轮再拆出去（现在拆只是把 PoC 供起来）；拆完 alva-sandbox 对应 3 年路线图里"SDK 有独立外部消费者才分仓"的同一判断标准。

## 五、待拍板的三个问题

1. **tool 递交**：方案 1（子环境只推理，每次 tool call reverse-RPC 回宿主执行——安全闸门天然复用，倾向这个）还是方案 2（子环境重建工具——性能好但要重做安全层）？
2. **子 agent 空 middleware（问题 2）**：这是当初的设计意图还是缺口？不管跨不跨环境，建议先修——可以并进架构诊断报告的 PR 系列（放 PR-2 附近）。
3. **目标环境优先级**：sandbox 本地进程 → wasm VM → 远端网络，三者递进（网络传输是所有先例的净新增工作）。先做哪个决定协议层的第一版形状。

## 附录：诊断方法与证据等级

- 3 路并行探查：wasm 可移植性（含 wasm32 target 实测编译）、子 agent 跨环境宿主契约与三先例复用性、alva-sandbox 姊妹仓全源码评估。
- **已逐行人工复核（最高证据等级）**：`run.rs:722/:833` std::time 回归（`:608` 用 web_time 对照）、`run_child.rs:96` 空 MiddlewareStack、`execute_shell.rs:170` serial-global 声明与 `AgentSpawnTool` 无 execution_mode 覆写。
- 其余发现来自探查报告，均带 file:line 引用，行号对应 main @ fe4559c。
