# alva-engine-adapter-alva
> Alva Hub 的 canonical app-layer adapter：每个 engine session 构造完整 BaseAgent。

## 地位
它实现轻量 `alva-engine-runtime::EngineRuntime` contract，但本身属于 Hub/app 产品层，允许依赖 `alva-app-core`。每个 native engine session 由 `BaseAgentFactory` 独立构造，保留 BaseAgent 的默认 plugin、security、memory、context 和自定义 harness 配方。

## 逻辑
1. `BaseAgentFactory` 在新 session 时构造一个独立 `BaseAgent`，返回 `BuiltAgent`（可选 `approval_rx`）。
2. **`CatalogBaseAgentFactory`（Hub 产品默认）**：`apply_components` 全量 catalog + `ApprovalPlugin` + Skills/MCP 双源路径 + AGENTS.md 注入，与 CLI/Tauri 深度对齐。
3. **`DefaultBaseAgentFactory`**：仅 default plugins，供单测与极简场景。
3.1. Hub 用 `with_toggles` + `with_managed_mcp` 注入下一轮能力；显式空列表不回退全局 MCP，受管模式关闭外部 subprocess loader，只注册一个渐进式 MCP gateway 和短 server prompt。meta-tool 保持 HITL，子 Agent 复用 SecurityGuard；其他 caller 保留 direct catalog 兼容路径。
4. `execute()` 映射 `AgentEvent` → `RuntimeEvent`；ToolExecutionStart 用 monotonic clock 计时并在 ToolEnd 保留 duration；`approval_rx` 由 session 持有，每 turn 暂借给当前事件流做 multiplex，结束后归还，避免旧 stream 长驻或截获新 turn 审批。
5. `EngineInspector` 直接读取同一个 AgentSession 的 typed record snapshot/live tail，不复制第二份 Inspector 日志。
6. `SessionRepository` 是唯一持久化源：typed records 原子写入 SQLite，infallible legacy append 失败会标脏并由 `flush()` 事务重建，敏感字段写盘前脱敏，大载荷转内容寻址 zstd blob。
7. 同进程 idle BaseAgent 优先；running session 拒绝并发 execute。
8. 每轮独立 cancellation token：`cancel()` 在 BaseAgent 尚未组装完成时也可取消；消费者丢弃 stream 会中断 pending factory/MCP 组装，不在后台继续调用模型。运行期取消/consumer 关闭转交已启动的 BaseAgent 并 drain 到真实终态/flush 后释放 running，resume 不继承前一轮取消；`respond_permission()` 仍路由对应 BaseAgent，修改 tool input 尚不支持。
9. adapter 从 `alva-app-core::session_projection` 重导出 `build_run_record` / `RunRecord`，供 L7 Hub 在不绕过 adapter 依赖边界的前提下投影稳定 Inspector DTO。
10. `with_context_repository` 使用 adapter/Inspector 的同一个 authority，为当前 native session 注入 `ContextRetrievalPlugin`：普通结果 >4 KiB、任何 ToolContent 图片、MCP 调用结果只向模型投影引用，记录/事件/error 原样保留；优先级在现有压缩之前。模型检索固定 session、128 records/64 MiB scan、20 hits/100-byte previews、2 KiB UTF-8 reads/16 KiB response；缺失/损坏明确失败，reasoning/typed media bytes 不暴露。compaction 引用验证来源而不层层套引用；重复 call ID 不串源，重启可读。
11. `SessionPluginFactory` 是显式产品配方：每个实际 native root ID 创建 Plugin；Hub 经它绑定已认领的 HubStore/session，补跨 branch 可见历史与按需媒体。契约/Middleware 类型从此 app adapter 重导出，不给 SDK 增加业务能力，也不自动将根产品数据授权给递归子树。`visible_context_tool_text` 复用 typed 媒体字节排除规则。

## 约束
- 不得构造裸 `AgentState` 或直接调用 `run_agent`；必须通过 BaseAgent factory。
- 一个 engine session 只拥有一个 BaseAgent，不能用 `swap_session` 共享 agent 支撑并发。
- 引用政策覆盖经 Hub factory 装配的 ALVA（含 Hub-owned Worker）与 app-core `agent` tool 的独立递归子装配：通过初始化发布的 SpawnContextPolicy 按实际子会话 ID 重绑自有检索/首个 assembly hook/请求 middleware/短提示；空 host whitelist 仍有自有 reader，父/其他会话记录不可读。子 hook 来源失败后 provider fail-closed，即使后续 hook 删除 entries；重算投影 token metadata，不改变 raw records。通信 hooks 是可信代码，不是沙箱；不新增子会话自动压缩器，不控制外部 engine。历史当前各自 native session，不冒充跨 engine 产品历史。blob 校验 SHA-256/尺寸，解压 ≤64 MiB、压缩输入 ≤65 MiB。
- 这是 app/product 层 crate，不能被 SDK/kernel/agent crate 反向依赖；Hub core 只依赖 `alva-engine-runtime` contract。
- **`execute` 的每一条出口都必须发 `Error{recoverable:false}` + `Completed`**，静默 `return` / `break` 一律禁止。消费者无法区分「静默 EOF」与「引擎失联」，Hub 会替你合成兜底并可能触发付费 fallback。两条曾经违规的路径（session map 回写落空、agent 事件通道提前关闭）已各自带回归测试。
- **`respond_permission` 必须消费 `BaseAgent::resolve_permission` 的返回值**：`false` 表示该审批已不在 pending（引擎已超时并 Block 了工具），必须回 `RuntimeError::PermissionNotFound`。返回 `Ok(())` 会让产品层把一条 `Allowed …` 写进会话审计轨迹，而那个工具实际从未执行。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| AlvaAdapter | adapter.rs | EngineRuntime + EngineInspector：session 生命周期、审批路由、typed snapshot/live tail |
| BaseAgentFactory / SessionPluginFactory / BuiltAgent | factory.rs | 每 session 组装；Catalog vs Default 两档，显式 root 产品 Plugin 配方 |
| SessionRepository | session_repository.rs | engine-native SQLite records、atomic batch/脏修复、resume/live tail 与固定会话有界检索/引用来源校验 |
| ContextRetrievalPlugin | context_retrieval.rs | 通用结果/MCP 媒体引用、固定会话耐久历史检索；递归子会话重新绑定与 assembly fail-closed，真实父/子/孙隔离和重启回归 |
| EventMapper | mapping.rs | AgentEvent → RuntimeEvent；Completed 聚合最后一条助手消息的全部文本块；ToolEnd 保留真实执行 duration |
| lib.rs | lib.rs | 重导出 adapter、factory、reference-first Plugin、repository Inspector 与稳定 run projection API |
