# alva-agent-runtime/src/middleware
> 运行时领域中间件：将领域安全组件适配为 Agent 中间件接口

## 地位
`alva-agent-runtime` 的中间件子模块。存放依赖领域 crate（如 `alva-agent-security`）的中间件实现。这些中间件不适合放在 `alva-agent-core` 中（core 不应依赖领域 crate），所以放在 runtime 层。

## 逻辑
1. `mod.rs` 声明 `security` 模块并 re-export `SecurityMiddleware`。
2. `security.rs` 将 `SecurityGuard`（来自 `alva-agent-security`）包装为 `alva-agent-core::middleware::Middleware` trait 实现，在工具调用前执行安全检查：
   - `Deny` -> 返回 MiddlewareError 阻止执行
   - `NeedHumanApproval` -> 返回 MiddlewareError 等待人工审批
   - `Allow` -> 放行

## 约束
- `SecurityMiddleware` 内部持有 `Arc<Mutex<SecurityGuard>>`，每次检查需获取锁，高并发场景注意锁竞争。
- 中间件返回 `MiddlewareError` 后，Agent 执行管线会中断当前工具调用，不会 fallback。
- 新增领域中间件应放在此目录，保持 core 的领域无关性。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Module Root | mod.rs | 声明子模块并 re-export SecurityMiddleware, CheckpointMiddleware, CompactionMiddleware, PlanModeMiddleware |
| SecurityMiddleware | security.rs | 将 SecurityGuard 适配为 Middleware trait：读取 bus ApprovalNotifier 实现交互式权限审批 |
| CheckpointMiddleware | checkpoint.rs | 写入工具执行前自动备份文件：从 bus 读取 CheckpointCallbackRef |
| CompactionMiddleware | compaction.rs | 上下文压缩：使用 bus TokenCounter 估算 token、超限时 LLM 摘要、发送 bus 事件 |
