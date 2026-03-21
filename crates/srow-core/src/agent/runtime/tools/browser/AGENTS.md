# agent/runtime/tools/browser
> 浏览器自动化工具集 —— 基于 CDP（Chrome DevTools Protocol）

## 地位
提供 7 个浏览器操作工具，通过 `BrowserManager` 管理 Chrome 实例生命周期，所有工具共享同一管理器实例。

## 逻辑
`BrowserManager` 维护 Chrome 实例池（按 ID 索引），处理启动/停止/导航/标签管理。各 tool 通过 `SharedBrowserManager`（`Arc<Mutex<BrowserManager>>`）获取引用。

## 约束
- 依赖 chromiumoxide crate
- 每个 Chrome 实例需要后台 tokio task 驱动 CDP 事件循环
- 截图最大支持 10MB

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块、pub use 再导出 |
| browser_manager | `browser_manager.rs` | BrowserManager、BrowserInstance、TabInfo、SharedBrowserManager |
| browser_start | `browser_start.rs` | BrowserStartTool：启动 Chrome 实例 |
| browser_stop | `browser_stop.rs` | BrowserStopTool：关闭 Chrome 实例 |
| browser_navigate | `browser_navigate.rs` | BrowserNavigateTool：导航到 URL |
| browser_action | `browser_action.rs` | BrowserActionTool：click/type/press/scroll 交互 |
| browser_snapshot | `browser_snapshot.rs` | BrowserSnapshotTool：提取页面内容（text/html/readability） |
| browser_screenshot | `browser_screenshot.rs` | BrowserScreenshotTool：页面截图 |
| browser_status | `browser_status.rs` | BrowserStatusTool：查询浏览器状态 |
