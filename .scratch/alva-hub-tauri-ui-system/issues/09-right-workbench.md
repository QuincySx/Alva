# 09 Right Workbench

## 目标

按 35–48 线稿实现统一 Workbench tab 生命周期和页面骨架。

## 工作

- 可调整宽度、停靠/覆盖、关闭、排序和会话持久化；
- Files、Review、Agent、Inspector、Tasks、Usage、Browser、Terminal；
- Engines/Fallback/Team 归入适合的 tab 或设置入口；
- WorkbenchPage 唯一滚动，tab 内容不堆叠；
- Agent 显示子 Agent 关系，Inspector 显示执行调试详情；
- fixture 通过后连接真实工具状态。

## 验收

- 至少三个 tab 同时打开；
- 关闭 Workbench 不停止终端、浏览器或任务；
- Agent 与 Inspector 是独立标签；
- 窄窗口自动转覆盖式 Sheet；
- fixture 与真实工具生命周期均通过。
