# 横向支撑 · L4.5 Engine 根契约与产品适配
> Engine 是完整 Agent harness，不是模型 Provider；Hub 只消费此层的执行、独立检查与声明契约。
## 地位
仅本目录根契约包属于 L4.5：不依赖 SDK、harness、platform/storage 或 apps。具体 adapter 在独立子包，不能反向成为 Hub core 的依赖；整个 integration/engine 容器不因此成为 SDK 层。
## 逻辑
Execute 的 context 属于执行所有者；Cancel 只是请求，真实 Completed 才收尾。Error fatal 后仅允许 Completed，工具必须配对，大结果用 qualified native reference。
ExecutionActivity独立表示native build/prepare启动前事实，必须先await耐久report再启动；ToolIntent.NoSideEffect不覆盖这些工作，Activity不冒充ToolStart/End或完成。
## 约束
不制造可恢复 native ID，不静默忽略不支持的选项。PermissionCallback/Resume 能力如实声明。模型/账户配置由显式 assembly 注入，不读取用户旧配置或凭据。
## 业务域清单
| 文件/目录 | 职责 |
|---|---|
| contract.go | 统一 Request/Event/Runtime/能力与流契约 |
| descriptor.go | 独立Runtime/Inspector装配、保守配置/权限关系/安装声明；不探测宿主或账户 |
| inspector.go | stdlib-only检查契约：bounded metadata/text/typed child/背压观察/显式JSONL，不含执行或删除 |
| contract_test.go | stream终态/配对、字段身份与typed event边界回归 |
| alva | 消费完整 L5 harness/SDK 的 Alva 产品桥接，不是轻契约或 SDK 包 |
| process | 按 Vendor 分派 Claude/Codex/Pi 的共享原生 JSONL adapter 实现，不冒充三个独立 package |

Request.RetainWorkspace is an optional nonserialized trusted host capability: native ToolControl adapters may extend the actual admitted workspace lease for explicitly application-owned background resources, releasing only after physical reap. It grants no tool permission; sealed attempts reject late retention. Opaque external engines do not receive it.

Descriptor仅声明实际Go配方：显式option-aware Alva factory与Claude print已支持bounded SystemPrompt/MaxTurns；Codex/Pi不把system role降为user文字，fast-mode未接线就不声明。未注册Inspector时明确unsupported，不借用Alva存储。Scope由可信产品branch/typed chain派生，owner/native ID不是UI授权输入。
