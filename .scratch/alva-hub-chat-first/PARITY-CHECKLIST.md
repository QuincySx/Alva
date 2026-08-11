# GPUI Migration Parity Checklist

> 对照基线：Hub 使用 `89ed8957^` 的 Tauri/React 源码；Alva App 使用 `363a942a^` 的 Tauri/React 源码。
> 状态定义：`migrated` = 功能已存在；`partial` = 功能存在但交互/视觉/组件形态未对齐；`missing` = 未实现；`obsolete` = 新产品方向不再需要。

## 组件库覆盖现状

| 项目 | 当前证据 | 状态 |
|---|---:|---|
| `Button` | 48 处 | migrated |
| `Input` | 15 处 | migrated |
| `Select` | 6 处 | migrated |
| `Checkbox` | 5 处 | migrated |
| `Icon` | 1 处 | partial |
| `Sidebar` / `Avatar` / `Tag` / `Collapsible` / `TabBar` | 0 处 | missing |
| `Skeleton` / `Spinner` / `Tooltip` | 0 处 | missing |
| 裸 `div()` | 约 200 处 | partial |
| 手搓圆角/边框/间距组合 | 约 121 处 | partial |

结论：GPUI 技术迁移已经完成，但 gpui-component 设计系统迁移尚未完成。

## ALVA Hub

| 能力 | GPUI 位置 | 状态 | 后续任务 |
|---|---|---|---|
| 原生窗口、Root、主题、最小尺寸 | `main.rs` | migrated | 01 |
| Workspace 选择 | `ui/sidebar.rs` | migrated | 01 |
| Session 列表、搜索、恢复 | `ui/sidebar.rs`, `store.rs` | migrated | 01/03 |
| Engine readiness / repair | `ui/readiness.rs`, `readiness.rs`, `repair.rs` | migrated | 04 |
| Fallback policy 编辑 | `ui/fallback.rs` | migrated | 04 |
| TeamRun / Worker 操作 | `ui/team.rs`, `ui/team_actions.rs` | migrated | 09 |
| Timeline、attempt、handoff | `ui/timeline.rs`, `ui/details.rs` | migrated | 02–04 |
| Permission cards | `ui/approval.rs` | migrated | 04 |
| Composer 与 engine 选择 | `ui/timeline.rs`, `ui/topbar.rs` | partial | 03/05 |
| Chat-first 信息架构 | 无 | missing | 02–04 |
| Engine/model picker | 无统一模型目录 | missing | 05–07 |
| Provider 添加向导 | 无 | missing | 06 |
| Engine × model 兼容性 badge | 无 | missing | 07 |
| 嵌入模型协议 gateway | Hub 未接入 | missing | 08 |
| gpui-component Sidebar/Tag/Tab/Sheet 等 | 标准区域多数手搓 | missing | 01 |
| Light/dark、键盘、视觉 parity | 未完成打包验收 | partial | 10 |

## ALVA App

| 能力 | GPUI 位置 | 状态 | 后续任务 |
|---|---|---|---|
| 原生窗口与聊天流式响应 | `main.rs` | migrated | 01 |
| SQLite session 列表与切换 | `sqlite_session/`, `main.rs` | migrated | 01 |
| Provider settings | `main.rs` | migrated | 01 |
| 动态 provider 列表 | `main.rs` | migrated | 01 |
| Embedded gateway 控制 | `main.rs`, `service.rs` | migrated | 01 |
| Tool picker | `main.rs`, `tools.rs` | migrated | 01 |
| Approval cards | `main.rs` | migrated | 01 |
| Skills / MCP 页面 | `main.rs`, `skills.rs`, `mcp.rs` | migrated | 01 |
| Inspector 数据投影 | `main.rs` | migrated | 01 |
| 独立 Inspector window | 当前为同窗 view | partial | 10 决定保留或 obsolete |
| Resizable split | 无等价组件 | missing | 01 |
| Model picker 交互与视觉 | provider select 可用，未形成完整 picker | partial | 01/05 |
| gpui-component Sidebar/Avatar/Tag/Tab/Sheet | 标准区域多数手搓 | missing | 01 |
| 打包视觉 parity | `.app` 已能打包，未完成组件替换后复验 | partial | 10 |

## 删除旧 Tauri 的闸门

只有同时满足以下条件才允许删除旧目录：

- [ ] ticket 01 的两套 GPUI 组件基线完成；
- [ ] Hub ticket 02–09 完成；
- [ ] ALVA App 中所有 `partial/missing` 项完成或明确标记 `obsolete`；
- [ ] ticket 10 的打包、视觉、键盘和三引擎验收通过；
- [ ] 当前文档、Cargo、CI、打包脚本不再依赖旧目录；
- [ ] 用户确认最终删除。
