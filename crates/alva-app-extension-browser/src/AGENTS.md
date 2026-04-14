# alva-agent-tools/src/browser
> 基于 Chrome DevTools Protocol (CDP) 的浏览器自动化工具组

## 地位
`alva-agent-tools` crate 的 browser 子模块。通过 `mod.rs` re-export 7 个浏览器工具和共享的 `BrowserManager`。所有工具仅在 `browser` feature 下编译，依赖 `chromiumoxide` crate 与 Chrome 实例通信。

## 逻辑
1. `browser_manager.rs` 管理 Chrome 实例生命周期：启动 Chrome、建立 CDP 连接、标签页管理。通过 `SharedBrowserManager`（`Arc<Mutex<BrowserManager>>`）在多个工具间共享状态。
2. 每个工具通过持有 `SharedBrowserManager` 引用访问浏览器实例，实例以字符串 ID 标识（默认 "default"）。
3. 工具调用链典型流程：`browser_start` 启动实例 -> `browser_navigate` 导航 -> `browser_action` 交互 -> `browser_snapshot` / `browser_screenshot` 提取内容 -> `browser_stop` 关闭。
4. `browser_status` 可随时查询实例状态，支持通配符 `"*"` 列出所有实例。

## 约束
- 所有工具共享同一个 `SharedBrowserManager`，实例 ID 需在工具间保持一致。
- `browser_action` 使用 CDP Input domain 直接派发鼠标/键盘事件，非 DOM API。
- `browser_screenshot` 使用 CDP Page.captureScreenshot，输出保存为文件而非内联返回。
- Chrome 实例须事先安装在系统中，`chromiumoxide` 不负责安装。
- 并发操作同一实例时由 `Mutex` 串行化，高并发场景可能成为瓶颈。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Module Root | mod.rs | 声明子模块并 re-export 全部浏览器工具和 BrowserManager |
| BrowserManager | browser_manager.rs | Chrome 实例生命周期管理、CDP 连接、标签页导航、共享访问 |
| BrowserStartTool | browser_start.rs | 启动 Chrome 实例，支持 headless 模式、用户 profile、代理配置 |
| BrowserStopTool | browser_stop.rs | 关闭 Chrome 实例并释放所有资源 |
| BrowserNavigateTool | browser_navigate.rs | 导航到 URL，等待加载完成，返回最终 URL 和标题 |
| BrowserActionTool | browser_action.rs | 页面交互：click / type / press / scroll，支持 CSS 选择器和坐标 |
| BrowserSnapshotTool | browser_snapshot.rs | 提取页面内容：text / HTML / readability（文章提取）模式 |
| BrowserScreenshotTool | browser_screenshot.rs | 页面截图：viewport / full-page / element 模式，保存为文件 |
| BrowserStatusTool | browser_status.rs | 查询浏览器实例状态：运行状态、当前 URL、标题、打开标签页 |
