# side_panel
> 左侧导航栏：新建会话按钮 + 工作区/会话树

## 地位
在 `views` 中作为左侧固定宽度导航列，管理 workspace 和 session 的浏览与选择。

## 逻辑
- `SidePanel` 顶部渲染 "+ New" 按钮，点击创建临时 GlobalSession 并立即选中。
- 下方嵌入 `SidebarTree` Entity 渲染工作区树。
- `SidebarTree` 订阅 `WorkspaceModel` 事件重绘，读取 `sidebar_items` 和 `selected_session_id`。
- GlobalSession 单行可点击，选中态高亮。
- Workspace 行带折叠箭头（▶/▼），点击调用 `toggle_workspace`；展开后显示子 session 列表（缩进）。

## 约束
- 新建 session 的 ID 基于时间戳，仅存在于内存。
- 不支持重命名、删除 session/workspace。
- Workspace 文件夹图标使用 Unicode emoji（\u{1F4C1}）。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| SidePanel | `side_panel.rs` | 左侧面板容器：New 按钮 + SidebarTree |
| SidebarTree | `sidebar_tree.rs` | 可滚动树视图，渲染 workspace 折叠/展开和 session 选择高亮 |
| mod | `mod.rs` | 桶模块，re-export SidePanel |
