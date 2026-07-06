# PR-11 执行手册:agent_session 行为下沉 kernel-abi → kernel-core

> **目的**:把 `crates/alva-kernel-abi/src/agent_session.rs` 里的**具体实现**下沉到
> kernel-core(L2),让 kernel-abi(L1)回到"纯契约 + 值类型"。这是 SDK 1.0 前该还的
> 分层债(诊断 2026-07-02 PR-11)。**纯结构搬运,零行为变化**——靠现有测试护航。
> **在干净的新会话里一次性执行**(迁移是原子的,中间态必编译红)。
>
> **前置(已完成)**:D-3 listener 泄漏修复已单独 commit(`f0524ed`),与本迁移解耦,不受影响。
>
> **破坏面已探明**:所有消费者 crate **都已依赖 kernel-core**(security/agent-core/agent-context/
> app-*/host-*/engine-adapter-alva/loader 全 YES),**零新增依赖**。仅 import 路径变更。

---

## 一、边界(留 abi vs 迁 core)

以 `f0524ed` 后的行号为准(D-3 已并入,行号可能微移,按符号名定位)。

**留 kernel-abi(纯契约 + 值类型):**
- `SessionError`、`EventEmitter` + `EmitterKind` + `ComponentDescriptor`
- `SessionEvent` + `SessionMessage` + `impl SessionEvent`(构造器)
- `EventQuery`、`EventMatch`
- `AgentSession` trait(含默认方法 `messages_since` / `subscribe_events` 的默认实现——它们只用 trait 自身的方法,不碰具体类型)
- `SessionEventStream` type alias
- `SessionEventListener` trait(**含 D-3 加的 `is_active` 默认方法**)
- `ScopedSession`(trait-object 包装器,只用 `Arc<dyn AgentSession>`,不依赖具体实现)

**迁 kernel-core(行为实现):**
- `InMemoryAgentSession`(struct + 全部 impl:`new`/`with_id`/`with_parent`/`restore_events`/
  `classify_message` + `impl AgentSession` + `impl Default`)
- `ListenableInMemorySession`(struct + `impl`(subscribe/broadcast)+ `impl AgentSession` + `impl Default`)
- `ChannelListener`(struct + `impl SessionEventListener`)
- 内部 helpers:`event_matches`、`safe_truncate`、`make_preview`
- **整个 `mod tests`**——逐一核对后,几乎所有测试都构造 `InMemoryAgentSession` / `ListenableInMemorySession`
  或其 helper（`user_msg`/`make_test_listener`），全部随实现迁 core。若发现个别只测
  `SessionEvent`/`EventQuery` 纯值类型的用例,留一份在 abi。

---

## 二、最小破坏面策略:kernel-core 的 agent_session 模块 re-export 契约

关键技巧:kernel-core 新建 `agent_session` 模块,顶部 **`pub use alva_kernel_abi::agent_session::{全部契约类型}`**。
这样消费者的混合 import（如 `use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession}`）
**只需把 crate 名 `alva_kernel_abi` → `alva_kernel_core`,符号列表原样不动**——`AgentSession` 从
core re-export 拿，`InMemoryAgentSession` 从 core 本地拿。纯 sed 级改动，无需拆分每个 import。

kernel-core → kernel-abi 是既有依赖（L2→L1），re-export L1 类型完全合法。

---

## 三、分步执行

### 步骤 1：建 `crates/alva-kernel-core/src/agent_session.rs`
1. 从 kernel-abi 的 agent_session.rs **剪切**第一节列出的"迁 core"全部代码块 + `mod tests`。
2. 文件顶部：
   ```rust
   // Concrete AgentSession implementations. The contract (AgentSession /
   // SessionEvent / EventQuery / … traits + value types) lives in
   // alva-kernel-abi; these are the in-memory backends that implement it.
   // Re-export the contract so consumers get everything from one path.
   pub use alva_kernel_abi::agent_session::{
       AgentSession, ComponentDescriptor, EmitterKind, EventEmitter, EventMatch, EventQuery,
       ScopedSession, SessionError, SessionEvent, SessionEventListener, SessionEventStream,
       SessionMessage,
   };
   use alva_kernel_abi::{AgentMessage, MessageRole};
   use std::collections::VecDeque;
   use std::sync::atomic::{AtomicU64, Ordering};
   use tokio::sync::RwLock;
   use async_trait::async_trait;
   ```
3. **陷阱**：subscribe_events 里 `use futures_util::stream::{self, StreamExt}` → 改成
   `use futures::stream::{self, StreamExt}`（kernel-core 依赖的是 `futures`，不是 `futures-util`）。
4. `InMemoryAgentSession` 的私有字段被 `ListenableInMemorySession::append` 直接访问
   （`self.inner.events` / `self.inner.seq_counter`）——两者迁到**同一模块**后私有访问仍成立，无需改可见性。

### 步骤 2：`crates/alva-kernel-core/src/lib.rs`
- 加 `pub mod agent_session;`
- 顶层 re-export（对齐 kernel-abi 原来的顶层导出习惯，方便消费者）：
  ```rust
  pub use agent_session::{InMemoryAgentSession, ListenableInMemorySession};
  ```

### 步骤 3：`crates/alva-kernel-abi/src/agent_session.rs`（剥离后）
- 删掉已迁走的所有代码块与 `mod tests`（迁走的部分）。
- 保留契约/值类型 + `ScopedSession` + 两个 trait（含 `is_active` 默认方法）+ 若有的纯值类型测试。
- 确认剥离后 abi 的 agent_session.rs 不再 `use` 任何仅具体实现需要的东西
  （如 `futures_util::stream`、`VecDeque`、`AtomicU64` 若仅被具体实现用则删 import）。

### 步骤 4：`crates/alva-kernel-abi/src/lib.rs`
- 从 `pub use agent_session::{...}` 块**删除** `InMemoryAgentSession, ListenableInMemorySession`
  两个符号（其余契约类型保留）。ChannelListener 原本非 pub，无需处理。

### 步骤 5：改消费者 import（约 15 处，含测试/示例）
用这个命令定位全部，逐一把 `alva_kernel_abi::agent_session::` → `alva_kernel_core::agent_session::`
（**仅**改含 `InMemoryAgentSession` / `ListenableInMemorySession` / `ChannelListener` 的行；纯契约行不必动，
但改了也对——core re-export 了契约）：
```
grep -rn "InMemoryAgentSession\|ListenableInMemorySession\|ChannelListener" crates --include="*.rs" \
  | grep "alva_kernel_abi::agent_session"
```
已知生产 import 点（起点，运行上面命令拿全量）：
- `crates/alva-kernel-core/src/run_child.rs:10` — kernel-core 内部：改成 `crate::agent_session::{...}`
- `crates/alva-agent-core/src/agent_builder.rs:10`
- `crates/alva-host-native/src/builder.rs:8`
- `crates/alva-host-wasm/src/agent.rs:26`
- security（约 15 引用，import 点应少数几个）、app-core、app-tauri、app-cli、
  engine-adapter-alva、agent-context、app-extension-loader（tests/examples）各自的 import 点。

顶层路径用法（`alva_kernel_abi::InMemoryAgentSession` 不带 `agent_session::` 段）：**grep 确认无人使用**，
但迁移后若有编译错误按同法改成 `alva_kernel_core::InMemoryAgentSession`。

### 步骤 6：kernel-core 自己的测试目录
`crates/alva-kernel-core/tests/*.rs`（integration/middleware_order/input_committed/session_skeleton/
tool_batch_coordinator）与 `examples/middleware_basic.rs` 里的
`use alva_kernel_abi::agent_session::{...InMemoryAgentSession...}` → `alva_kernel_core::agent_session::{...}`
（它们是外部 crate 视角，用 crate 名而非 `crate::`）。

---

## 四、验证清单（全绿才算完成）
```
cargo test -p alva-kernel-abi          # 契约 + 剥离后残留测试
cargo test -p alva-kernel-core         # 迁移来的 session 测试 + 既有
cargo test --workspace --exclude alva-app-tauri
cargo test -p alva-app-tauri
cargo check --target wasm32-unknown-unknown -p alva-host-wasm   # ★ 迁移代码必须保持 wasm-clean
./scripts/ci-check-deps.sh             # 依赖边界 + wasm32 集合
cargo fmt --all --check
```
**wasm 验证是硬门槛**：迁移的 session 代码用 uuid(v4，wasm 需 js feature——kernel-abi 已配，确认
kernel-core 同样配了 wasm 段的 `getrandom/js`)、tokio sync、futures stream，这些 kernel-core 作为
wasm-clean crate 已在用；但迁入后必须重新过一遍 wasm check。

---

## 五、已知陷阱汇总
1. `futures_util::stream` → `futures::stream`（crate 名不同）。
2. `ListenableInMemorySession` 访问 `InMemoryAgentSession` 私有字段——同模块迁移后仍成立，勿改字段可见性。
3. `ScopedSession` 留 abi（trait-object，不依赖具体实现）——别误迁。
4. `AgentSession` trait 的默认方法 `messages_since`/`subscribe_events` 留 abi（只用 trait 自身方法）。
5. tests mod 里的 `ForwardToSession`（`listenable_session_nested_forward` 内联定义）随测试迁 core。
6. wasm：kernel-core 的 Cargo.toml 若无 `[target.'cfg(target_family="wasm")'.dependencies]` 的
   `getrandom/js`，迁入 uuid v4 后 wasm 会因 RNG 缺失编译失败——照 kernel-abi 的 wasm 段补上。
7. 迁移是**一个原子 commit**：建 core 模块 + 剥 abi + 改全部 import 一起提交，中间态编译红属正常。

---

## 六、预期 commit
```
refactor(kernel): sink AgentSession implementations from abi (contract) to core

InMemoryAgentSession / ListenableInMemorySession / ChannelListener + their
tests move from alva-kernel-abi (L1, pure contract) to alva-kernel-core
(L2). kernel-abi keeps the AgentSession / SessionEventListener traits, the
value types (SessionEvent / EventQuery / …), and ScopedSession. Pure
structural move, no behavior change — the migrated test suite is the proof.

kernel-core's agent_session module re-exports the contract types, so
consumers only swap the crate name in their imports (alva_kernel_abi →
alva_kernel_core); no import lists were split and no crate gained a new
dependency (every consumer already depended on kernel-core). Restores the
L1 "pure contract" invariant ahead of SDK 1.0.
```
