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
- 次级页面动作必须消费真实 callback 或明确 disabled；插件样例不是安装目录，不声称可用或已安装。Files/Text 列按所在容器宽度自动排列，浏览器操作可换行，不独立定义页面断点。

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
| workspace-git.tsx / workspace-git.test.tsx | 真实 worktree staged/unstaged/untracked 和有界 diff；revision/会话隔离异步结果，无 Git 写入动作 |
| workspace-terminal.tsx / workspace-terminal.test.tsx | 用户直接操作的逐行 PTY；显式启动、生命周期清理、不模拟 HTTP 执行，后台轮询不清掉输入失败提示 |
| workspace-browser.tsx / workspace-browser.test.tsx | 独立临时用户浏览器的原生导航控件；显式打开、真实 loading/慢加载提示、关闭重开、迟到打开回收；调用者按 session key 隔离，HTTP 不支持 |
| fallback-policy-panel.tsx / team-panel.tsx | fallback 和团队操作 |
| product-pages.tsx | 设置、插件样例与搜索、真实自动化 re-export；真实外观偏好/Inspector 导航/工作区 Skills，未接通能力明确不可用 |
| workspace-skills.tsx / workspace-skills.test.tsx | session-scoped 项目技能文件目录、源文档创建/编辑、确认归档/恢复、只读来源/失败草稿保留、迟到回复与焦点回归 |
| automations.tsx / automations.test.tsx | 真实本地 interval 配置/状态、暂停创建/编辑、显式账户计费启用、Pause/删除确认、焦点恢复与会话导航；无 cron daemon 或默认执行 |
| *.test.tsx | 确定性组件交互与协议回归 |
