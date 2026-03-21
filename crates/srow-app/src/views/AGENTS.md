# views
> GPUI 视图层：三栏布局 + 各面板组件

## 地位
在 `srow-app` crate 中承担所有 UI 渲染职责，将 `models` 层的状态映射为 GPUI 可视元素。

## 逻辑
- `RootView` 是窗口的根视图，采用 flex-row 三栏布局：
  - 左栏 `SidePanel`（220px 固定宽度）：导航 + 会话树
  - 中栏 `ChatPanel`（flex-1）：消息列表 + 输入框
  - 右栏 `AgentPanel`（280px 固定宽度）：状态/设置/预览标签页
- 所有面板通过 GPUI Entity 引用共享 model，model 事件驱动 UI 重绘。
- `InputBox` 是唯一调用 `EngineBridge` 的组件，形成 View -> Bridge -> Engine 的单向数据流。

## 约束
- 所有视图使用 `Theme::for_appearance()` 解析当前主题颜色。
- 面板间无直接通信，全部经由共享 model 间接协调。
- GPUI 不支持 CSS/HTML；布局使用 `div()` builder API。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| RootView | `root_view.rs` | 窗口根视图，三栏 flex 布局 |
| agent_panel | `agent_panel/` | 右侧面板：Agent 状态 + 设置 + 预览 |
| chat_panel | `chat_panel/` | 中央面板：消息列表 + 输入框 |
| settings_panel | `settings_panel/` | 设置表单（被 agent_panel 内嵌） |
| side_panel | `side_panel/` | 左侧导航：新建按钮 + 工作区/会话树 |
| mod | `mod.rs` | 桶模块，re-export RootView |
