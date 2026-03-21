# srow-app/src
> srow-app crate 源码根目录

## 地位
srow-agent 项目的桌面 GUI 应用源码，基于 GPUI 框架构建，通过 `engine_bridge` 连接 `srow_core` AI 引擎。

## 逻辑
- `main.rs`（binary 入口）：初始化 tracing、创建 4 个共享 model Entity、设置全局主题、打开主窗口（RootView）、配置菜单和快捷键。
- `lib.rs`：声明并导出 6 个子模块。
- 数据流向：`InputBox` -> `EngineBridge::send_message` -> 后台 `AgentEngine` -> `EngineEvent` channel -> 前台分派至 `ChatModel` / `AgentModel` -> 订阅者 View 重绘。
- 主题系统：`SettingsModel` 持有 `ThemeMode`，`main.rs` 同步到 `ActiveThemeMode` 全局，所有 View 通过 `Theme::for_appearance()` 读取。

## 约束
- 仅支持 macOS（GPUI-CE 的平台限制）。
- 窗口固定初始尺寸 1280x800。
- 无多窗口支持（窗口关闭即退出应用）。
- Cmd-Q 全局快捷键退出。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| main | `main.rs` | 应用入口：初始化 GPUI、model、窗口、菜单 |
| lib | `lib.rs` | crate 根库：导出所有子模块 |
| error | `error.rs` | 统一错误类型 SrowError |
| theme | `theme.rs` | 主题解析：ActiveThemeMode 全局 + Theme 语义颜色 |
| types | `types/` | 领域数据结构（Workspace, Session, Message, AgentStatus） |
| models | `models/` | GPUI 响应式模型（Workspace, Chat, Agent, Settings） |
| engine_bridge | `engine_bridge/` | UI 与 srow_core AgentEngine 的适配层 |
| views | `views/` | 三栏 UI 视图（SidePanel, ChatPanel, AgentPanel） |
