# 2026-09-09 内核边界 Review 证据

## 审查范围与结论

- Diff：`git diff 5800395...6e5bf33`，历史卡 01–19 与随后一条 CI 修复，206 个文件。
- 基线：`6e5bf33`。以下四个测试在该基线实际运行并失败；不是仅由静态阅读推断。
- 后续工作入口：[TASKS.md](../TASKS.md)，本轮卡 R01–R08。原卡有实现提交不等于已全部验收。

## 可复现测试

| 卡 | 测试 | 基线实际结果 |
|---|---|---|
| R01 | `spawned_child_obeys_shared_tool_lock` | 持有 SerialGlobal 锁时，子工具的执行标记仍变为 true |
| R02 | `production_analytics_plugin_records_llm_call` | 用 AnalyticsPlugin 正式装配跑完一轮，JSONL 文件内容为 `""` |
| R03 | `concurrent_broadcast_preserves_seq_with_yielding_listener` | channel subscriber 收到 `[2, 1]`，期望 `[1, 2]` |
| R04 | `failed_run_start_still_emits_terminal_event` | 注入首条 append 失败后，事件流仅有 `[AgentStart]` |

文件是审查时保留的测试种子，尚未纳入项目测试树：

- [wiring_regressions.rs](./wiring_regressions.rs)：在 `alva-app-core` 的集成测试上下文编译；覆盖卡 R01、R02、R04。
- [broadcast_regression.rs](./broadcast_regression.rs)：在 `alva-agent-core` 的集成测试上下文编译；覆盖卡 R03。

实现者应将对应测试迁入已有的语义相关测试文件，或新建符合分形文档协议的集成测试文件。
如直接复现原始种子，可分别复制到：
`crates/alva-app-core/tests/review_wiring_regressions.rs` 和
`crates/alva-agent-core/tests/review_broadcast_regression.rs`，运行：

```sh
cargo test -p alva-app-core --test review_wiring_regressions --offline --locked
cargo test -p alva-agent-core --test review_broadcast_regression --offline --locked
```

这两个命令在未修复的基线下应失败。按卡拆分后只运行当前卡对应测试；不要因其他卡仍失败而修改它们的断言。
卡 R05 改变契约后，可以适配测试类型接线，但必须保留上述行为断言。

## 已完成验证

- `cargo test -p alva-agent-core --lib --offline --locked`：65 passed。
- `cargo test -p alva-hub-desktop --lib --test readiness_tests --offline --locked`：172 + R02 passed。
- `cargo fmt --all -- --check`：通过。
- `scripts/public-surface.sh --check`：三个 crate 快照通过。
- `scripts/ci-check-deps.sh` 的依赖边界和 bus surface 阶段通过；后续完整 wasm 检查未完成，不能标记整条脚本通过。
- 未执行完整 `cargo test --workspace`。

## 执行与交接

本次只建卡、保存证据，不修复生产代码。后续执行者先核对当前 HEAD 和工作区状态，
沿当前分支逐卡实现，不重做原卡 01–19；低优先级的 R07、R08 是设计整理，不与故障修复混为一项提交。
