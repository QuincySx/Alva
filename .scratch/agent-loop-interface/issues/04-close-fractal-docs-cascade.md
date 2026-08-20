# 04 — Close the fractal-docs cascade for AgentLoop

**What to build:** `bebc7d6`/`0f44bb8`/`7fd8ae0`/`9003f7f` 引入了 `AgentLoop` trait 与 `DefaultAgentLoop`（ADR-0007 的核心交付物），但没有走 `FRACTAL-DOCS.md` 规定的三级联更新：文件头三行注释、目录级 `AGENTS.md`、上级 crate `AGENTS.md`、根 `AGENTS.md` 的 crate 清单全都还停留在"kernel-core 只有 `run_agent()`"的状态。本票据把文档补齐，并顺手修一处过时注释和一句不准确的历史表述。

**Blocked by:** None — can start immediately.

**Status:** done/shipped — 经 `90e2680 docs(kernel-core): close fractal-docs cascade for AgentLoop` 全部落地并验证。

- [x] `crates/alva-kernel-core/src/run.rs:2` 的 `// OUTPUT: pub async fn run_agent()` 补上 `AgentLoop`（trait，`run.rs:970`）与 `DefaultAgentLoop`（struct，`run.rs:985`）。
- [x] `crates/alva-kernel-core/src/AGENTS.md`：「逻辑」第 1 点（`run.rs` 那条）与「业务域清单」表格里 `run.rs` 那一行，补充「`AgentLoop`/`DefaultAgentLoop` 提供可替换 driver 接口，`run_agent` 是其默认实现」。
- [x] `crates/alva-kernel-core/AGENTS.md`：「逻辑」小节里描述 `run.rs` 的那一条，补同样的信息。
- [x] 根 `AGENTS.md` 里 `alva-kernel-core`（L2 agent loop）那一行 crate 清单，补充 `AgentLoop` 可替换 loop 接口。
- [x] `crates/alva-kernel-core/src/middleware.rs:50`：`Middleware` trait 文档注释里 `wrap_*` hook 那一行还写着 `"(mutable, state is threaded through the chain)"`，但类型早已改成 `&AgentState`（共享只读引用），这句话现在跟类型矛盾——去掉 "mutable"，改成准确描述 state 如何穿过 chain（比如 "shared, threaded through the chain via reborrow"）。
- [x] 在合适的位置（`AgentLoop`/`Middleware` trait 的文档注释，或 `crates/alva-kernel-core/AGENTS.md`「约束」小节）用准确说法补一句「为什么 `&mut AgentState` 改成了 `&AgentState`」：真实原因是 `MiddlewareStack` 的洋葱式组合（`wrap_llm_call`/`wrap_tool_call` 递归调用 `next: &dyn LlmCallFn/ToolCallFn` continuation）没法干净地让一个独占借用穿过嵌套 async continuation，**不是**"object-safe 要求"——`#[async_trait] fn drive(&self, state: &mut AgentState, ...)` 本身完全 object-safe，Rust 的 object-safety 规则不关心非 `self` 参数的可变性。既有 commit message（`0f44bb8`/`7fd8ae0`）不用改写，但面向未来读者的文档要写对。
- [x] 验证：纯文档改动，跑 `git diff` 确认没碰任何 `.rs` 文件的可执行代码（只有 doc comment 行）。
