# alva-agent-core
> Agent-layer core: Plugin/Registrar assembly, AgentBuilder, Agent facade, MockToolFs.

## Role
`alva-agent-core` holds the pure SDK-level agent assembly machinery: plugins
register tools, phase contributions, middleware, bus capabilities, commands,
and system prompt fragments through a `Registrar`; `AgentBuilder` runs the
plugin lifecycle and produces an `Agent` backed by
`alva-kernel-core::run_agent`. It also contains
`MockToolFs`, a lightweight in-memory `ToolFs` test fixture.

## Public Surface
Re-exported from `src/lib.rs`:
- `Agent` — SDK-level runnable facade；`run()` 保持 executor-neutral unbounded 兼容，native `run_bounded()` 提供生产背压。
- `AgentAssemblySnapshot` / `PluginAssemblySnapshot` — build-time plugin,
  phase, middleware, tool, command, and prompt contribution snapshots used by
  harnesses and CLIs for observability.
- `AgentBuilder` — SDK-level builder that assembles model, session, plugins,
  tools, middleware, bus, and context config.
- `extension::Plugin` — self-contained capability bundle with compatible
  `register()` / `finalize()` hooks and fallible `try_register()` /
  `try_finalize()` production seams.
- `extension::Registrar` — setup handle passed to plugins for registering
  tools, phase contributions, middleware, bus capabilities, system-prompt
  fragments, and commands.
- `alva_kernel_abi::{Phase, PhaseEffect}` — kernel-owned stable runtime
  timeline vocabulary and effect categories.
- `extension::{PhaseContribution, PhaseOrder}` — agent-core assembly
  descriptors targeting the kernel phase vocabulary. `Registrar::phase(...)`
  records metadata-only contributions.
- `extension::PhaseHandler` — executable phase contribution registered through
  `Registrar::phase_handler(...)`. Agent-core compiles it into the current
  middleware stack while kernel-native phase execution is pending.
- `extension::LateContext` — read context for late tool discovery and
  cross-plugin wiring after all `register()` calls finish.
- `PluginHost` — runtime container for plugin-registered middleware,
  command metadata, prompt fragments, and cancellation binding.
- `RegisteredCommand` — plugin-registered slash-command descriptor.
- `MockToolFs` — in-memory `ToolFs` implementation for tests.

## Dependency Policy
- Only `alva-kernel-abi` and `alva-kernel-core`.
- NO protocol crates, NO LLM providers, NO `tokio` process/fs, NO persistence.
- Compiles cleanly for `wasm32-unknown-unknown` (part of the CI wasm invariant).

## Bus Assembly Rule
- `AgentBuilder::with_bus_writer(...)` is the normal external-bus path when
  plugins are registered. Plugins receive a `Registrar` and may publish typed
  capabilities through `Registrar::provide(...)`, which requires the writer for
  that same bus.
- `AgentBuilder::with_bus(...)` is handle-only and therefore read-only. It is
  only valid for plugin-less assembly; `build()` rejects handle-only bus usage
  when plugins are present so capability registration cannot disappear into a
  throwaway bus.

## Module Map
| Name | Path | Role |
|------|------|------|
| `agent.rs` | `src/agent.rs` | Runnable `Agent` facade、兼容 unbounded 与 native bounded 事件入口、assembly snapshot |
| `agent_builder.rs` | `src/agent_builder.rs` | Plugin lifecycle, SDK-level assembly, and build metadata capture |
| `extension/mod.rs` | `src/extension/mod.rs` | Plugin system public re-exports |
| `extension/plugin.rs` | `src/extension/plugin.rs` | `Plugin` trait (`register`/`finalize` compatibility hooks + fallible `try_register`/`try_finalize`) |
| `extension/phase.rs` | `src/extension/phase.rs` | Stable phase contribution descriptors |
| `extension/registrar.rs` | `src/extension/registrar.rs` | `Registrar` and `LateContext` |
| `extension/host.rs` | `src/extension/host.rs` | `PluginHost`: runtime container for middleware, commands, prompt fragments, cancellation |
| `mock_fs.rs` | `src/mock_fs.rs` | `MockToolFs` in-memory test fixture |

## Where Things Do NOT Live
- Built-in tool implementations → `alva-agent-extension-builtin`.
- `LocalToolFs` native adapter → `alva-agent-extension-builtin`.
- `BaseAgent` and session wiring → still in `alva-app-core` (historical; planned for a later refactor).

## 迁入模块（内核边界重构 R05/R08）

- `src/agent_session.rs`：InMemoryAgentSession / ListenableInMemorySession 内存会话后端（自 kernel-core 迁入）。R25/R26 重写投递路径：`BroadcastCore` 是唯一投递实现——追加者 commit→notify 段无 await 点，专属 driver task 负责唤醒与投递（typed append 也会唤醒空闲 driver）；真实 abort（`JoinError::is_cancelled`）后已提交事件仍可达既有实时订阅。R46 事实核对（描述真实实现，替代旧承诺）：**只有 `clear`** 递增广播代际并把全部游标重置到新代际 seq 1，`restore`/`rollback` 直接委托 inner、不触碰代际；clear 不取 delivery_lock。R38：inline 投递（无 Tokio runtime/wasm）持 delivery_lock 调用监听者回调，回调内重入追加同一会话经线程局部 `DELIVERY_ACTIVE` 守卫直接返回、由外层循环继续取批投递（重入方不再等待自己持有的锁）；native driver 路径的追加仍走 driver。R39：driver 持 `BroadcastCore` 强引用运行，最后一个会话句柄 Drop 置位 `closed` 并唤醒 driver——最终一轮投递取尽已提交事件后置 `driver_exited` 退出，不残留泊车任务；置位后队列冻结（在途 append 必持有句柄），因此最终投递不吞事件。同模块的 `CellBackedSessions`（R27）把 kernel-abi 的具体 `StoreSessionCell` 包装成 `Arc<dyn PersistentSessions>` 能力——装配层经它向内核下发"当前会话"，内核只见 dyn trait。
- `src/tool_lock.rs`：跨 Agent 工具资源锁表 + `ToolLockMiddleware`（自 kernel-core 迁入）。经 bus 可选发现；子 agent 栈须显式挂中间件并接线 bus 与 workspace（configure 不触发的路径用 `with_bus`/`with_workspace`，R11）。
- 会话切换（R10/R15）：`swap_session` 在 state 锁内原子完成 flush → 日志/SharedCell 更新；压缩中间件、工具上下文、session_handle 读取同一 cell，总是指向当前会话。
