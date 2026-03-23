# alva-runtime

> Batteries-included agent runtime composing alva-core + alva-tools + alva-security + alva-memory with a builder API.

---

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | src/lib.rs | Composes all agent subsystems and re-exports a batteries-included API |
| AgentRuntimeBuilder | src/builder.rs | Builder pattern for constructing a fully-configured AgentRuntime with tools, middleware, and model |
| Model Init | src/init.rs | Resolves a "provider/model_id" spec string into a LanguageModel via ProviderRegistry |
| Basic Example | examples/runtime_basic.rs | Demonstrates builder API usage with a stub provider and logging middleware |
