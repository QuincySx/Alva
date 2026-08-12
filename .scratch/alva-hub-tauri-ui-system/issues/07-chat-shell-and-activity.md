# 07 Chat Shell and Activity

## 目标

按 01/31–34/49–52 线稿重建聊天主壳和单一阅读轴。

## 工作

- Sidebar 项目/会话层级、整行 hover 和 context action；
- User/Agent message 与 `HubActivityRow`；
- approval/fallback/handoff 进入同一时间线；
- SessionDetails 归入正文详情或 Workbench，不成为第二主页面；
- 滚动锚点、流式更新和回到底部行为；
- fixture 通过后接入真实 Hub store 和 Tauri command。

## 验收

- 所有运行事件只占一条聊天轴；
- 工具明细默认折叠，摘要、原因和下一步优先；
- 审批与 Composer 始终同轴；
- 长会话滚动时 Composer 不抖动；
- fixture 和真实会话流程均通过。
