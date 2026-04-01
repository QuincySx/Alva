# Alva

> Rust 实现的分层架构 AI Agent 平台：alva-types（基础 trait + ToolFs）→ alva-agent-core（循环引擎 + 异步 Middleware）→ alva-agent-tools/security/memory（独立功能 crate）→ alva-agent-runtime（Builder API）→ alva-engine-runtime（统一引擎接口）→ alva-app-core（Facade）→ alva-app（GPUI 桌面应用）

> **⚠ 本项目采用分形文档协议，必须严格遵守 [FRACTAL-DOCS.md](./FRACTAL-DOCS.md) 中定义的三层文档规范。**

---

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 基础层 | `crates/alva-types/` | 基础类型和 trait：ToolContext（泛化）+ LocalToolContext、Tool、LanguageModel、Provider、Message、StreamEvent |
| 协调总线 | `crates/alva-agent-bus/` | 跨层协调总线：Caps（typed 能力注册/发现）+ EventBus（typed pub/sub）+ StateCell（可观察状态）+ BusPlugin 插件体系 |
| 引擎层 | `crates/alva-agent-core/` | Agent 循环引擎：双层 Loop + AgentHooks + 异步 MiddlewareStack（洋葱模型）+ CompressionMiddleware + AgentEvent |
| 工具层 | `crates/alva-agent-tools/` | 16 个内置 Tool 实现：9 标准工具 + 7 浏览器工具（feature-gated），实现 alva_types::Tool trait |
| 安全层 | `crates/alva-agent-security/` | 安全子系统：SecurityGuard、PermissionManager、SensitivePathFilter、AuthorizedRoots、SandboxConfig |
| 记忆层 | `crates/alva-agent-memory/` | 记忆子系统：FTS + 向量混合搜索、SQLite 存储、EmbeddingProvider、文件同步 |
| 上下文管理 | `crates/alva-agent-context/` | 上下文工程：ContextStore 五层容器、Plugin 体系、SDK compose/inject 接口、DefaultContextPlugin 规则+LLM 回调 |
| 图编排层 | `crates/alva-agent-graph/` | 图执行 + 编排：StateGraph、Channel、Pregel、Session、Retry、Checkpoint、SubAgent |
| 运行时层 | `crates/alva-agent-runtime/` | Batteries-included 运行时：AgentRuntimeBuilder、model("provider/id") 统一初始化 |
| Skill 协议 | `crates/alva-protocol-skill/` | Skill 系统（独立）：加载、注入、存储、渐进式三级加载 |
| MCP 协议 | `crates/alva-protocol-mcp/` | Model Context Protocol 客户端：连接、工具发现、McpToolAdapter 桥接为 Tool trait |
| ACP 协议 | `crates/alva-protocol-acp/` | Agent Client Protocol（独立）：消息类型、会话、连接、进程管理 |
| 应用 Facade | `crates/alva-app-core/` | 薄 Facade 层：Re-export agent-* crate + 保留 skills/mcp/environment/persistence/domain |
| 桌面应用 | `crates/alva-app/` | GPUI 桌面 GUI：仅通过 alva-app-core Facade 导入，Sidebar、Chat、Markdown 渲染 |
| 调试服务器 | `crates/alva-app-debug/` | AI 调试系统：HTTP API、日志捕获、视图树检查、traced! 宏 |
| 引擎接口层 | `crates/alva-engine-runtime/` | EngineRuntime trait：execute / cancel / respond_permission / capabilities |
| Alva 引擎适配器 | `crates/alva-engine-adapter-alva/` | 本地 Agent 适配器：AgentEvent → RuntimeEvent 映射，直接 Rust 调用 |
| Claude 引擎适配器 | `crates/alva-engine-adapter-claude/` | Claude SDK 适配器：Node.js bridge + JSON-line 协议 |
| 开发工具 MCP | `crates/alva-app-devtools-mcp/` | MCP 服务器：wrapping alva-app-debug HTTP API |
| 测试工具 | `crates/alva-test/` | MockLanguageModel 等测试辅助 |
| 依赖防火墙 | `scripts/ci-check-deps.sh` | CI 自动化：强制 12 条 crate 边界规则，确保分层不被破坏 |
| 架构文档 | `docs/ARCHITECTURE.md` | 三仓库架构设计：alva-sandbox + alva-agent + alva-app |
| 工作区配置 | `Cargo.toml` | Rust workspace，管理 17 个 crate |

# 项目架构

> 详细架构设计见 [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md)

## GPUI
Use when building GPUI components, custom elements, managing state/entities, working with contexts, handling events/subscriptions, async tasks, global state, actions/keybindings, focus management, layout/styling, code style conventions, or writing GPUI tests.
`docs/gpui/index.md`

## Git Commit 规范

1. **小步提交**：每个逻辑改动单独一个 commit，不要攒多个改动一起提交。方便后续 bisect、revert 和 review。
2. **说清楚改了什么**：commit message 第一行写改动内容，用 `feat:` / `fix:` / `refactor:` / `chore:` 前缀区分类型。
3. **写清楚为什么改**：如果改动原因不是显而易见的，在 commit message body 里补充原因。格式：第一行摘要，空一行，然后写原因。

示例：

```
refactor: rename MessageInjector → PendingMessageQueue

"Injector" 是依赖注入框架术语，在 agent 消息队列场景下不直观。
PendingMessageQueue（待处理消息队列）一读就懂。
```

```
fix: remove dead Steering branch from LLM filter

Steering 消息在注入 session 前已转成 Standard，
session 里不会出现 Steering 变体，这个 match 分支永远不会命中。
```

# alva-agent-bus 防破坏规则
> Bus 是跨层协调总线，不是万能通道。本文档定义它的边界，防止退化为 God Object。
./docs/BUS-RULES.md

## Compact Instructions 如何保留关键信息
保留优先级：
1. 架构决策，不得摘要
2. 已修改文件和关键变更
3. 验证状态，pass/fail
4. 未解决的 TODO 和回滚笔记
5. 工具输出，可删，只保留 pass/fail 结论
