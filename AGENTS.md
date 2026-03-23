# Srow Agent

> Rust 实现的三层架构 AI Agent 平台：agent-types（类型/trait）→ agent-core（循环引擎）→ agent-graph（图编排），配合 GPUI 桌面应用

> **⚠ 本项目采用分形文档协议，必须严格遵守 [FRACTAL-DOCS.md](./FRACTAL-DOCS.md) 中定义的三层文档规范。**

---

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 基础层 | `crates/agent-types/` | 基础类型和 trait：Message、Tool、LanguageModel、StreamEvent、CancellationToken |
| 引擎层 | `crates/agent-core/` | Agent 循环引擎：双层 Loop + 6 Hooks + AgentEvent + 工具执行 |
| 图编排层 | `crates/agent-graph/` | 图执行 + 编排：StateGraph、Channel、Pregel、Session、Retry、Checkpoint、SubAgent |
| Skill 协议 | `crates/protocol-context-skill/` | Skill 系统：加载、注入、存储、渐进式加载 |
| MCP 协议 | `crates/protocol-model-context/` | Model Context Protocol 客户端：连接、工具发现、工具调用 |
| ACP 协议 | `crates/protocol-agent-client/` | Agent Client Protocol：消息类型、会话、连接、进程管理 |
| 应用核心 | `crates/srow-core/` | 应用特有逻辑：具体 Tool 实现、安全、持久化、Environment |
| 桌面应用 | `crates/srow-app/` | GPUI 桌面 GUI：Sidebar、Chat、Dialogs、Markdown 渲染 |
| 调试服务器 | `crates/srow-debug/` | AI 调试系统：HTTP API、日志捕获、视图树检查、traced! 宏 |
| 工作区配置 | `Cargo.toml` | Rust workspace，管理 9 个 crate |

## GPUI
Use when building GPUI components, custom elements, managing state/entities, working with contexts, handling events/subscriptions, async tasks, global state, actions/keybindings, focus management, layout/styling, code style conventions, or writing GPUI tests.
`docs/gpui/index.md`

## Compact Instructions 如何保留关键信息
保留优先级：
1. 架构决策，不得摘要
2. 已修改文件和关键变更
3. 验证状态，pass/fail
4. 未解决的 TODO 和回滚笔记
5. 工具输出，可删，只保留 pass/fail 结论
