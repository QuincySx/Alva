# alva-engine-adapter-alva
> Alva Hub 的 canonical app-layer adapter：每个 engine session 构造完整 BaseAgent。

## 地位
它实现轻量 `alva-engine-runtime::EngineRuntime` contract，但本身属于 Hub/app 产品层，允许依赖 `alva-app-core`。每个 native engine session 由 `BaseAgentFactory` 独立构造，保留 BaseAgent 的默认 plugin、security、memory、context 和自定义 harness 配方。

## 逻辑
1. `BaseAgentFactory` 在新 session 时构造一个独立 `BaseAgent`，返回 `BuiltAgent`（可选 `approval_rx`）。
2. **`CatalogBaseAgentFactory`（Hub 产品默认）**：`apply_components` 全量 catalog + `ApprovalPlugin` + Skills/MCP 双源路径 + AGENTS.md 注入，与 CLI/Tauri 深度对齐。
3. **`DefaultBaseAgentFactory`**：仅 default plugins，供单测与极简场景。
3.1. Hub 用 `with_toggles` + `with_managed_mcp` 注入下一轮能力；显式空列表不回退全局 MCP，受管模式关闭外部 subprocess loader。MCP 含 meta-tool 加入 HITL，子 Agent 复用 SecurityGuard；其他 caller 保留 catalog discovery 兼容路径。
4. `execute()` 映射 `AgentEvent` → `RuntimeEvent`；ToolExecutionStart 用 monotonic clock 计时并在 ToolEnd 保留 duration；`approval_rx` 由 session 持有，每 turn 暂借给当前事件流做 multiplex，结束后归还，避免旧 stream 长驻或截获新 turn 审批。
5. `EngineInspector` 直接读取同一个 AgentSession 的 typed record snapshot/live tail，不复制第二份 Inspector 日志。
6. `SessionRepository` 是唯一持久化源：typed records 原子写入 SQLite，infallible legacy append 失败会标脏并由 `flush()` 事务重建，敏感字段写盘前脱敏，大载荷转内容寻址 zstd blob。
7. 同进程 idle BaseAgent 优先；running session 拒绝并发 execute。
8. 每轮独立 cancellation token：`cancel()` 在 BaseAgent 尚未组装完成时也可取消；消费者丢弃 stream 会中断 pending factory/MCP 组装，不在后台继续调用模型。运行期取消/consumer 关闭转交已启动的 BaseAgent 并 drain 到真实终态/flush 后释放 running，resume 不继承前一轮取消；`respond_permission()` 仍路由对应 BaseAgent，修改 tool input 尚不支持。
9. adapter 从 `alva-app-core::session_projection` 重导出 `build_run_record` / `RunRecord`，供 L7 Hub 在不绕过 adapter 依赖边界的前提下投影稳定 Inspector DTO。

## 约束
- 不得构造裸 `AgentState` 或直接调用 `run_agent`；必须通过 BaseAgent factory。
- 一个 engine session 只拥有一个 BaseAgent，不能用 `swap_session` 共享 agent 支撑并发。
- 这是 app/product 层 crate，不能被 SDK/kernel/agent crate 反向依赖；Hub core 只依赖 `alva-engine-runtime` contract。
- **`execute` 的每一条出口都必须发 `Error{recoverable:false}` + `Completed`**，静默 `return` / `break` 一律禁止。消费者无法区分「静默 EOF」与「引擎失联」，Hub 会替你合成兜底并可能触发付费 fallback。两条曾经违规的路径（session map 回写落空、agent 事件通道提前关闭）已各自带回归测试。
- **`respond_permission` 必须消费 `BaseAgent::resolve_permission` 的返回值**：`false` 表示该审批已不在 pending（引擎已超时并 Block 了工具），必须回 `RuntimeError::PermissionNotFound`。返回 `Ok(())` 会让产品层把一条 `Allowed …` 写进会话审计轨迹，而那个工具实际从未执行。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| AlvaAdapter | adapter.rs | EngineRuntime + EngineInspector：session 生命周期、审批路由、typed snapshot/live tail |
| BaseAgentFactory / BuiltAgent | factory.rs | 每 session 组装；Catalog vs Default 两档 |
| SessionRepository | session_repository.rs | engine-native SQLite AgentSession records、atomic batch、失败脏标记 + flush 事务修复、resume/live tail |
| EventMapper | mapping.rs | AgentEvent → RuntimeEvent；Completed 聚合最后一条助手消息的全部文本块；ToolEnd 保留真实执行 duration |
| lib.rs | lib.rs | 重导出 adapter、factory、repository Inspector 与稳定 run projection API |
