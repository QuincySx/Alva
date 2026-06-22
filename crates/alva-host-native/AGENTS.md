# alva-host-native

> Native platform capabilities for Alva. `AgentRuntimeBuilder` is retained as a deprecated legacy runtime path; new app code uses `alva-app-core::BaseAgentBuilder`, and SDK code uses `alva-agent-core::AgentBuilder`.

---

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | src/lib.rs | Composes all agent subsystems and re-exports a batteries-included API |
| AgentRuntimeBuilder | src/builder.rs | Deprecated legacy builder for constructing `Result<AgentRuntime, AgentError>`; kept for compatibility tests and low-level native runtime experiments, not recommended for new harness code |
| Model Init | src/init.rs | Resolves a "provider/model_id" spec string into a LanguageModel via ProviderRegistry |
| Basic Example | examples/runtime_basic.rs | Legacy smoke example for the deprecated runtime builder with a stub provider and logging middleware |
