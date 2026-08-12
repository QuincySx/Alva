# 02 Region Layout Contract

## 目标

让所有页面服从统一 AppFrame 和区域归属，不再逐组件修宽度。

## 工作

- 固化 `AppFrame/ContentFrame/ScrollRegion/DockRegion/WorkbenchPage`；
- 实现唯一滚动拥有者检测；
- 实现四档断点和 Workbench 停靠/覆盖行为；
- 统一 main content、settings content、dialog form 的宽度 variant；
- 审计当前未提交布局改动并逐项迁入、重写或删除。

## 验收

- Timeline、Notice、Composer 使用同一内容轴；
- 打开 Workbench 后 Main 整体收缩且内部不错位；
- 任一页面不存在嵌套双滚动；
- 四档窗口截图无覆盖、截断和横向滚动。
