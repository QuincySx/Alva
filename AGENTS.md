# Srow Agent

> Rust 实现的 AI Agent 平台，包含 GPUI 桌面应用和核心引擎库

> **⚠ 本项目采用分形文档协议，必须严格遵守 [FRACTAL-DOCS.md](./FRACTAL-DOCS.md) 中定义的三层文档规范。**

---

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| 桌面应用 | `crates/srow-app/` | GPUI 桌面 GUI，提供聊天、Agent 管理、设置等交互界面 |
| AI 交互层 | `crates/srow-ai/` | 框架无关的 AI 交互逻辑：Chat、Transport、Completion、ObjectGeneration |
| 核心引擎 | `crates/srow-core/` | Agent 引擎核心库：Provider V4、运行时、工具、MCP、技能系统、安全、持久化 |
| 工作区配置 | `Cargo.toml` | Rust workspace 配置，管理三个 crate 成员 |

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
