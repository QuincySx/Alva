# Hub domain components
> 消费产品 DTO，组合公共 ui Controls/Regions/Surfaces。

## 地位
位于 Pages 与公共 `ui/` 之间，不直接操作 adapter、数据库或宿主文件系统。

## 逻辑
页面传入状态与动作；对话使用一个有界窗口，工具正文通过 session-scoped bridge 按需读页。

## 约束
- 从 `../ui` 使用基础控件；遵循上级 AGENTS 及 UI glossary/wireframes/motion。
- `ToolContent` 由调用者按 content_id 加 key；每个挂载实例只保留当前块，翻页替换正文，不累积全文。
- 同会话/内容/页的请求匹配后才展示，卸载与会话切换丢弃迟到结果；失败可重试，无全文引用的旧记录明确标注摘要。
- 正文仅为 React 文本节点，不执行 HTML；默认读首块，不用工具级折叠隐藏事实。

## 业务域清单
| 文件 | 职责 |
|---|---|
| chat-transcript.tsx | 中央有界消息与 execution renderer |
| hub-timeline.tsx | 工具/审批/失败/fallback 卡与动作 |
| tool-content.tsx / tool-content.test.tsx | 有界全文分页、失败重试、隔离迟到响应及回归 |
| hub-sidebar.tsx / hub-topbar.tsx | 项目/会话导航和标题动作 |
| hub-composer.tsx / model-picker.tsx | 输入和运行时/model route 选择 |
| startup-gate.tsx / workspace-gate.tsx | 对话/项目入口与工作树选择 |
| provider-settings.tsx / engine-readiness-panel.tsx | Provider 配置和引擎就绪状态 |
| engine-inspector.tsx / session-details.tsx | 诊断记录与产品会话详情 |
| workbench-tools.tsx | 工作台页组合 |
| workspace-files.tsx / workspace-files.test.tsx | 真实文件目录导航、受限只读预览、刷新重试与会话隔离回归；按 session/workspace key 重新挂载 |
| fallback-policy-panel.tsx / team-panel.tsx | fallback 和团队操作 |
| product-pages.tsx | 设置、插件、自动化等次级页面 |
| *.test.tsx | 确定性组件交互与协议回归 |
