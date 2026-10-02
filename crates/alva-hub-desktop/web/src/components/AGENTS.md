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
- 原文为惰性 React 文本/安全 Markdown，不执行 HTML。版本化消息模块仅由编译进 Hub 的可信 renderer 消费有界数据，失败/未知类型保留原文；工具默认读首块，不用折叠隐藏事实。
- 搜索/复制覆盖所有已载入条目（包括窗口外消息）及工具预览，不声称搜索了分页全文。IME Enter 不提交，Shift Enter 换行，busy 时仍保留 Stop。
- 文件每 Turn 最多 8 个、单文件 20 MiB/总计 40 MiB，传用户明确选择的原字节，不将上传内容自动拼入模型 prompt。图片/视频本地 Blob 预览不是模型输入，关闭回收 URL；处理清单明确本次上传未压缩/转码/抽帧/分析。
- 次级页面动作必须消费真实 callback 或明确 disabled；Plugins 只管理真实内置 ALVA capability/MCP，不声称存在第三方包安装目录或 Installed 状态。Files/Text 列按所在容器宽度自动排列，浏览器操作可换行，不独立定义页面断点。

## 业务域清单
| 文件 | 职责 |
|---|---|
| chat-transcript.tsx / chat-transcript.test.tsx | 中央有界消息与 execution renderer、窗口外搜索跳转、复制、失败原因及安全链接回归 |
| safe-markdown.tsx | 助手安全 Markdown、表格/引用/代码与原生外链，用户消息保持原文 |
| copy-text-button.tsx | 显式复制、成功/失败反馈与卸载清理 |
| conversation-start.tsx / conversation-start.test.tsx | 已选目录的起步草稿与不可用 route Settings 入口，不自动执行 |
| input-attachments.tsx / input-attachments.test.tsx | 原字节选择/拖放、限额/草稿保留、会话归属惰性预览、媒体 URL 回收与处理清单 |
| message-modules.tsx / message-modules.test.tsx | 可信 renderer registry/provider、alva.display v1 表格/图表/图示、校验/错误隔离和原文 fallback |
| hub-timeline.tsx | 工具/审批/失败/fallback 卡与动作 |
| tool-content.tsx / tool-content.test.tsx | 有界全文分页、失败重试、隔离迟到响应及回归 |
| hub-sidebar.tsx / hub-topbar.tsx | 项目/会话导航和标题动作 |
| hub-composer.tsx / model-picker.tsx | 输入和运行时/model route 选择 |
| hub-composer.test.tsx | IME/换行/普通 Enter、busy Stop 与未接通附件按钮禁用回归 |
| startup-gate.tsx / workspace-gate.tsx | 对话/项目入口与工作树选择 |
| provider-settings.tsx / engine-readiness-panel.tsx | Provider 配置和引擎就绪状态 |
| engine-inspector.tsx / session-details.tsx | 诊断记录与产品会话详情 |
| workbench-tools.tsx | 工作台页组合 |
| workspace-files.tsx / workspace-files.test.tsx | 真实文件目录导航、受限只读预览、刷新重试与会话隔离回归；按 session/workspace key 重新挂载 |
| workspace-git.tsx / workspace-git.test.tsx | 真实 worktree staged/unstaged/untracked 和有界 diff；revision/会话隔离异步结果，无 Git 写入动作 |
| workspace-terminal.tsx / workspace-terminal.test.tsx | 用户直接操作的逐行 PTY；显式启动、生命周期清理、不模拟 HTTP 执行，后台轮询不清掉输入失败提示 |
| workspace-browser.tsx / workspace-browser.test.tsx | 独立临时用户浏览器的原生导航控件；显式打开、真实 loading/慢加载提示、关闭重开、迟到打开回收；调用者按 session key 隔离，HTTP 不支持 |
| fallback-policy-panel.tsx / team-panel.tsx | fallback 和团队操作 |
| product-pages.tsx | 设置、真实内置插件/MCP 与搜索、自动化 re-export、外观/Inspector/项目 Skills；第三方 AEP 安装明确未支持 |
| tool-settings.tsx / tool-settings.test.tsx | catalog capability 开关、MCP 禁用优先 CRUD/宿主授权测试、失败草稿/revision/焦点与迟到回复回归 |
| workspace-skills.tsx / workspace-skills.test.tsx | session-scoped 项目技能文件目录、源文档创建/编辑、确认归档/恢复、只读来源/失败草稿保留、迟到回复与焦点回归 |
| automations.tsx / automations.test.tsx | 真实本地 interval 配置/状态、暂停创建/编辑、显式账户计费启用、Pause/删除确认、焦点恢复与会话导航；无 cron daemon 或默认执行 |
| *.test.tsx | 确定性组件交互与协议回归 |
