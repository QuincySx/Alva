# alva-app
> GPUI 桌面 GUI 应用 crate — srow-agent 项目的用户界面

## 地位
srow-agent 项目的前端 crate，提供基于 GPUI-CE 的原生桌面应用，用户通过该界面与 AI Agent 交互。依赖 `alva-app-core` 提供引擎能力。

## 逻辑
- **架构**：Model-View-Bridge 三层结构。
  - `types/`：纯数据结构，无框架依赖（除 AgentStatusKind 的颜色方法）。
  - `models/`：GPUI Entity 响应式状态，EventEmitter 驱动。
  - `views/`：GPUI Render 视图，订阅 model 事件自动重绘。
  - `engine_bridge/`：将 `alva_app_core::AgentEngine` 适配到 GPUI 异步模型。
- **启动流程**：`main.rs` 创建 4 个共享 model -> 打开窗口 -> RootView 三栏布局 -> 用户输入 -> InputBox 调用 EngineBridge -> 后台 AgentEngine 运行 -> EngineEvent 流转回 ChatModel/AgentModel -> View 重绘。
- **持久化**：仅 `AppSettings` 序列化到 `~/.srow/settings.json`，其余数据为内存态。

## 约束
- 目标平台：macOS（GPUI-CE 限制）。
- UI 框架：GPUI-CE 0.3（社区版），非 HTML/CSS，使用 Rust builder API。
- 二进制产物名：`srow-agent`。
- 依赖 `alva-app-core`（本地路径 `../alva-app-core`）。
- 异步运行时：bridge 层每次请求创建独立 tokio Runtime。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Cargo.toml | `Cargo.toml` | crate 元数据和依赖声明 |
| src | `src/` | 全部源码：入口、库、类型、模型、视图、引擎桥接 |
