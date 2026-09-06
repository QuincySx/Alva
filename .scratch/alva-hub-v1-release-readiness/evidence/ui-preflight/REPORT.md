# Hub UI Release Preflight — 2026-08-30

> 结论：浏览器 fixture 预检已完成，G6 尚未通过。发现 1 个 release blocker、4 个中等级键盘/语义/响应式问题和 1 个低等级目标尺寸问题。最终签名包与 VoiceOver 验收仍待 Ticket 02/05/06。

## 范围与证据级别

- 目标：`crates/alva-hub-desktop/web` 当前 React UI；
- 浏览器：本机 Google Chrome，由 Playwright 通过真实 DOM/Accessibility tree 驱动；
- 数据：`?demo=1` 与 `?ui-lab=1`，不调用真实 Engine、不修改真实 Hub DB；
- 视口：1440×900、1280×800、860×600；
- 模式：中文、英文、dark、`prefers-reduced-motion: reduce`；
- packaged observation：本地 `ALVA Hub.app` 启动，但本轮无法取得 AX window，单列为验证阻塞，不冒充通过；
- 证据级别：本报告属于 A 级浏览器证据和一次未完成的 P 级 packaged observation。

## 已通过

| 项目 | 结果 |
|---|---|
| 七页面族 × desktop/minimum 扫描 | 无横向页面溢出、无重复 ID、无可重复 console/page/request error |
| 新会话 Workspace gate | `选择项目` 可用键盘激活，gate 关闭，composer 启用 |
| Model picker | Enter 打开并聚焦搜索；860×600 内不越界；Escape 一次关闭并恢复焦点 |
| UI Lab Popover | Enter 打开；Escape 一次关闭并恢复触发点 |
| Tabs | ArrowRight 同步移动 roving focus、selected state 与 panel |
| Settings switch | Space 切换 `aria-checked`，焦点保留 |
| 中文 → English | 自定义 Select 可用键盘操作；`html[lang]` 更新为 `en-US`；无页面横向溢出 |
| Focus visible | 首个键盘焦点显示 2px focus outline |
| Reduced motion | Model picker 只保留 background/color/opacity 的 120ms 状态反馈，无空间位移动效 |
| 视觉基线 | 既有 11 个 Playwright visual scenarios 保持 green |

## 发现

### UI-PF-01 · High · 窄屏 Workbench 缺少 overlay 焦点合同

复现：860×600，聚焦“打开工作台”并按 Enter。

- trigger 打开后被卸载，焦点落到 `body`；
- 背景 sidebar/main 未设 inert，Tab 首先进入背景“详情”、tool call、fallback 和审批控件；
- Escape 不关闭 overlay；
- 用“关闭工作台”按钮关闭后，焦点仍落到 `body`，没有返回稳定触发点。

影响：键盘用户无法可靠进入或退出 Workbench，阻塞 G6。

代码证据：`src/ui/layout.tsx` 的 `AppFrame` 只给关闭态 Workbench 设置 inert，并仅实现 scrim click dismissal；没有打开态 focus transfer、背景 inert、Escape 或 focus restore。

截图：[workbench-overlay-860.png](./workbench-overlay-860.png)

### UI-PF-02 · Medium · Dialog 需要按两次 Escape

UI Lab 共享 Dialog 打开后，初始焦点位于关闭 IconButton。第一次 Escape 只关闭其 Tooltip，Dialog 保持打开；第二次 Escape 才关闭 Dialog 并恢复触发点。

根因边界：`src/ui/overlays.tsx` 的 `Dialog.Close` 包装共享 `IconButton`，而 `src/ui/button.tsx` 的每个 `IconButton` 都拥有独立 Tooltip Root。

截图：[dialog-open-860.png](./dialog-open-860.png)

### UI-PF-03 · Medium · 会话 Context Menu 没有键盘入口

侧边栏 session 支持 pointer right-click，但聚焦同一 session 后按标准 `Shift+F10` 不会出现 `menu`。焦点仍留在 session 按钮。

代码证据：`src/components/hub-sidebar.tsx` 仅使用 Radix `ContextMenuTrigger`，没有 keyboard fallback。

### UI-PF-04 · Medium · Handoff 展开按钮缺少状态语义

“查看交接信息”可用 Enter 展开，目标和已完成工作内容出现、焦点保留，但 trigger 始终没有 `aria-expanded` / `aria-controls`。

代码证据：`src/components/hub-timeline.tsx` 使用普通 `Button` 切换 `openHandoffs[index]`，没有公开 disclosure state。

截图：[handoff-open-860.png](./handoff-open-860.png)

### UI-PF-05 · Medium · 最小支持宽度下 Settings 文案被截断

860px 是 Tauri 配置的最小窗口宽度。此宽度下中英文均发生可见截断：

- `编辑资料` / `Edit Profile`；
- `浅色` / `Light`；
- `深色` / `Dark`；
- `跟随系统` / `System`。

DOM 测量确认上述 label 的 `scrollWidth > clientWidth`，并被 `text-overflow: ellipsis` 隐藏。页面自身无横向溢出，但关键操作文案不完整。

截图：[settings-english-860.png](./settings-english-860.png)

### UI-PF-06 · Low · Workbench tab close target 只有 18×18

可见的 `hub-workbench__tab-close` 实测 18×18px，小于 24×24 的最低 target 参考。共享 17×17 Checkbox 位于可点击 label 内，不计入此缺陷。

代码证据：`src/styles/hub-components.css` 将 tab close 的 width/height 固定为 18px。

## Packaged observation 阻塞

`ALVA Hub.app` 可以启动且 bundle id 为 `com.alva.hub`，但 Orca 在 Accessibility 与 Screenshot 权限均 granted 时仍无法读取 AX window；CG window 枚举只返回一个 124×137、无法截图的 window。该现象不能直接证明产品的 VoiceOver 缺陷，也不能作为 packaged UX 通过证据。

后续必须在最终签名 artifact 上用 VoiceOver/manual smoke 或可观察的 acceptance build 复验，才能关闭 G6。

## 修复与复验顺序

1. Workbench overlay focus/dismissal/background inert；
2. Dialog close button Tooltip 与 Escape 协调；
3. Context menu keyboard entry 与 Handoff disclosure semantics；
4. Settings 860px 布局和 tab-close target；
5. 重跑本报告矩阵、63 Vitest、11 visual scenarios；
6. 最终签名 `.app` 做 VoiceOver、键盘和 focus smoke。

