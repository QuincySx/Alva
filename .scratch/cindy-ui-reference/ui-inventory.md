# Cindy 0.1.45 UI Inventory

## Scope and evidence

- Live target: Cindy macOS 0.1.45, installed at /Applications/Cindy.app.
- Baseline: 0.1.44 reference set in .scratch/cindy-ui-reference (67 screenshots, 67 cropped frames, 67 linework frames, 7 SVG wireframes).
- Delta capture: 43 unique live UI states. Each state has a JPEG capture, a normalized PNG, a deterministic same-name CIEdges linework PNG, AX summary, trigger, prerequisites, and branches in manifest.jsonl.
- Safety boundary: read-only traversal. No messages sent, tasks created, accounts changed, credentials entered, permissions accepted, processes terminated, files shared, or destructive actions executed.

## Product shell

The persistent shell has four structural regions:

1. Sidebar navigation: New, Automation, Plugins, global search, Projects, Conversations, account/settings, mobile download, release notes.
2. Task header and timeline: title, task menu, branch, messages, tool/activity rows, generated change cards, review/undo affordances.
3. Composer: target/project selector, attachment/action menu, permission mode, agent/engine, model/reasoning, voice, send, plan chip, question navigation.
4. Optional right workbench: tab strip, tool surface, detach, maximize, collapse.

Window-level routes observed: #/cc-agent/new, #/cc-agent/:id, #/automations, #/settings and #/sidebar-window.

## Surface inventory

### Chat, task, plan, and sharing

- Main timeline preserves activity rows, assistant messages, user messages, changed-file cards, Undo and Review. The current task exposes a 2/6 plan chip and a default previous-question navigation button.
- Task title menu in 0.1.45: Pin, Rename, Move to project, Copy task link, Open in new window, Export, Archive, Delete. The former split-open command is absent.
- Plan popover exposes ordered steps and current progress. A running breathing dot and failed-plan reconciliation require an active/failing plan and were not manufactured.
- Share as image enters a selection mode without exporting. Every eligible message gets a checkbox; footer exposes Select all, selection count, Cancel, Download locally, and Copy as image. A long task selected 23 messages.
- Existing changed-file UI exposes precise Undo and Review, but rollback/rewind was not invoked.

### New task and composer

- New task defaults to Conversation, Codex, GPT-5.6-Sol High, Auto approve, an empty “Hi Cindy!” composer, and four quick starts.
- The target picker contains Conversation and local Projects. It does not show a phone/mobile device entry in 0.1.45.
- Composer action menu: add file/image/video, New Goal, Plan mode, Collaboration mode, installed TapTap Maker plugin, read-only referenced directories, Add directory, keyword search.
- New Goal warns that it autonomously continues until achieved/blocked/stopped and may consume many tokens. Advanced limits are Max turns, Token budget, and No-progress turns (default 3); limits affect only the current goal.

### Right workbench

- Restored/preserved tabs observed: Review, SVG preview, Subagent, Background tasks, Resource usage, a generic New tab, Terminal.
- Add-tab catalog exposed Files, Review, Subagent, Background tasks, Browser, Terminal. Orca Workers and iOS Simulator were not exposed in this task/context.
- Subagent empty state says runtime-created Subagents persist in the tab.
- Resource usage is a sortable Process/CPU/Memory/PID table divided into Agent and app processes. Terminate Process remains disabled until selection and was never invoked.
- Detach creates a dedicated #/sidebar-window with Merge back. The populated resource table appeared after a short load, confirming the 0.1.45 preheated detached-window path. Maximize was visible but not used.

### Automation

- Empty list and template gallery; editor supports AI/script, automatic/manual, one-time/cron, new-per-run/continuous/bound task, silent run, Feishu/WeCom notifications, preflight prompt, project/worktree/engine/model.
- Cron picker supports Cron or relative schedules, minutes/hourly/daily/weekdays/weekly/monthly and time-of-day.
- Creation was not submitted.

### Settings and account

- General: theme/mode/local themes, UI/code fonts and sizes, sidebar card mode, panel reset, language, notifications, keep-awake, background click behavior, Cmd+Enter, Worker soft/hard/idle limits, Agent resource tiers/concurrency/priority/toolchain parallelism, Git savepoints, LSP beta, logout, account deletion.
- Profile editor: avatar and name, cross-device sync notice, Save/Cancel. No value was changed or saved.
- Providers: seven configured sources with drag ordering. Provider/model context menus, balance/usage/recharge, model visibility and categories.
- Add Provider catalog and Custom Endpoint form: template, auth type, runtime, base URL/exact path, API key, models, headers, test connection/get models/save. No credential entered and no connection test.
- Tools/MCP: built-in tool toggles and remote HTTP/SSE MCP form with name, URL, Bearer token and headers. No server saved.
- Remote: same-account devices, local machine, SSH hosts, reload ~/.ssh/config, key management, Add host. Draft fields include alias/host/user/port, ssh-agent/private key, key pinning and Agent proxy. No host connected or saved.
- IM Bots: WeChat, WeCom, Feishu/Lark and DingTalk. Expanding unconnected personal WeChat shows new-chat agent/model/permission, work directory, Tencent authorization notice and Connect WeChat. Connection and contacts remain gated.

### Search and navigation

- Sidebar global search expands to “Search title or content…” and keeps the task tree visible.
- Filter menu: relevance ordering, status, project, Agent, recent activity.
- Sidebar continuously exposes mobile download and release-note controls; the 0.1.45 release-notes overlay was captured.

## Screenshot manifest

| ID | Page/state | Trigger | Visible controls / evidence |
|---:|---|---|---|
| 001 | 更新公告-v0.1.45 | 启动 Cindy；首屏自动展示更新公告 | 更新公告、版本说明正文、我知道了 |
| 002 | 任务对话-主视图 | 关闭更新公告后进入当前任务 | 左侧导航/项目任务树、对话消息、改动摘要、计划进度、消息操作、输入区、模型/权限、任务成本与配额 |
| 003 | 侧边栏整理弹层 | 点击侧边栏「整理」 | 状态/Agent/最近活跃/分组/排序/项目筛选 |
| 004 | 项目更多操作菜单 | 点击项目 alva-agent 的「更多操作」 | 项目上下文菜单 |
| 005 | 侧栏任务条目-选中态 | 点击当前任务条目（菜单未展开） | 选中任务条目、时间、更多、归档；主任务内容保持可见。 |
| 006 | 自动化-列表页 | 点击左侧「自动化」 | 自动化列表/搜索/创建自动化/筛选/空态或已有规则 |
| 007 | 自动化-模板编辑器 | 点击「夜间自愈测试」模板 | 标题/Prompt/运行目录/模型/cron/隔离 worktree/保存取消等表单控件 |
| 008 | 自动化-Cron选择器 | 点击计划「Cron · Daily at 02:00」 | Cron/相对间隔；Minutes/Hourly/Daily/Weekdays/Weekly/Monthly；小时分钟/系统时间选择器 |
| 009 | 设置-0.1.45总览 | 点击用户头像/设置入口 | 设置侧栏目录与当前通用页；账户/外观/更新等 |
| 010 | 设置-通用中段-0.1.45增量 | 在通用设置内容区向下滚动一页 | 侧边栏卡片模式/面板布局/语言/通知/应用行为/消息输入等 |
| 011 | 设置-通用底部-0.1.45增量 | 通用设置继续向下滚动 | 协同 Worker 上限/空闲释放；Agent 资源档位/并发/优先级/限核；Git 安全保存点；LSP Beta；退出登录 |
| 012 | 设置-模型供应商-0.1.45 | 点击设置侧栏「模型供应商」 | 供应商目录、连接状态、默认模型/可见模型、添加供应商入口 |
| 013 | 设置-供应商更多操作菜单 | 点击 Cindy AI 供应商「更多操作」 | 供应商级管理菜单 |
| 014 | 设置-模型更多操作菜单 | 点击 Kimi K3 模型行「更多操作」 | 模型级操作菜单 |
| 015 | 设置-模型供应商-余额加载完成 | 等待供应商余额异步加载完成 | Cindy AI 可用余额、查看用量、余额充值、模型列表 |
| 016 | 设置-添加供应商弹层 | 点击「添加供应商」并等待弹层展开 | 三步流程指示、搜索、xAI 订阅授权、API/自托管供应商目录、自定义端点、取消 |
| 017 | 设置-自定义供应商连接表单 | 点击「自定义端点」 | 供应商名称/协议/Base URL/API Key/模型等连接表单 |
| 018 | 设置-自定义供应商模板菜单 | 点击「从模板快速填充」 | 常见兼容供应商模板列表 |
| 019 | 设置-工具与MCP | 点击设置侧栏「工具」 | 内置工具、MCP 服务列表、添加/编辑入口 |
| 020 | 设置-添加MCP服务器表单 | 点击「添加 MCP 服务器」 | 名称/传输类型/URL/Headers/保存取消等 |
| 021 | 设置-远程连接 | 关闭 MCP 表单后点击「远程连接」 | SSH 主机列表/添加主机/连接说明 |
| 022 | 设置-SSH主机列表 | 点击「SSH 远程主机」 | 已配置主机卡片、连接状态、更多动作、新增主机 |
| 023 | 设置-添加SSH主机表单 | 点击 SSH 主机「添加」 | 别名/主机/端口/用户名/认证方式/密钥/密码/代理/测试/保存 |
| 024 | 任务-计划浮层-0.1.45 | 点击 Composer 上方「第 2 / 6 步」 | 计划步骤列表、完成/执行中/等待/失败状态与关闭 |
| 025 | 任务-权限模式菜单 | 点击 Composer「自动审批」 | 默认权限/自动审批/完全访问及说明 |
| 026 | 任务-模型选择器-0.1.45 | 点击 Composer 当前模型 | 模型搜索、Agent/供应商分组、模型上下文、推理强度、Fast 模式等 |
| 027 | 任务-Composer操作菜单 | 点击 Composer「+」操作菜单 | 附件/文件夹/额外目录/Goal/计划/协作/插件等 |
| 028 | 任务标题-更多操作菜单-0.1.45 | 点击任务标题右侧“更多操作” | 置顶；重命名；移动到项目；复制任务链接；在新窗口打开；导出任务；归档；删除。0.1.45 未出现“分栏打开”。 |
| 029 | 任务-分享选区-0.1.45 | 点击一条消息的“分享为图片” | 每条消息出现复选框；底栏含全选、已选数量、取消、下载到本地、复制为图片；默认选中触发消息。 |
| 030 | 任务-长对话分享全选-0.1.45 | 在分享选区点击“全选” | 23 条可分享消息全部选中；底栏显示“已选 23 条消息”，提供下载到本地/复制为图片。 |
| 031 | 右栏-多标签工作台-0.1.45 | 点击右侧“展开” | 标签：审查、SVG 文件、Subagent、后台任务、资源用量、新标签、终端；每标签可关闭；底部支持在新窗口打开、最大化、收起。 |
| 032 | 右栏-资源用量表-0.1.45 | 切换到已存在的“资源用量”标签 | 表格按进程/CPU/内存/PID排序；分 Agent 进程与应用进程；右侧为进程详情空态；未选择时“终止进程”禁用。 |
| 033 | 资源用量-独立窗口-0.1.45 | 点击右栏底部“在新窗口中打开侧边栏” | 独立 sidebar-window 含完整标签组和资源用量表；顶部有“合并回主窗口”；资源数据已在打开后快速呈现。 |
| 034 | 右栏-subagent空态-0.1.45 | 在独立工作台切换到 Subagent 标签 | 空态文案：本任务还没有 Subagent；运行时创建的 Subagent 会持续保留在这里。 |
| 035 | 右栏-添加标签菜单-0.1.45 | 点击“添加标签” | 可添加：文件浏览器、浏览器、终端；审查/Subagent/后台任务标记已打开。当前菜单未显示 Orca Workers、iOS Simulator 或资源用量（资源用量已作为现有标签）。 |
| 036 | 新建任务-空白草稿-0.1.45 | 点击侧栏顶部“新建” | 协同模式默认“对话”；Hi Cindy 输入框；操作菜单、权限、引擎、模型、语音、发送；快速开始四项。新建页未显示手机/远程设备入口。 |
| 037 | 新建任务-对话或项目选择器-0.1.45 | 点击新建页顶部“协同模式”选择器 | 浮层按“对话”与“项目”分组；可选对话或本地项目，支持从列表移除、选择其他项目文件夹。未出现手机/远程设备项。 |
| 038 | goal-新建目标草稿 | 操作菜单→新建目标 | 说明持续推进语义及 token 消耗提示；目标描述输入；高级设置；取消；未填写时开始禁用。 |
| 039 | goal-高级限制 | 点击“高级设置” | 仅当前目标生效：最大轮数（不限）、Token 预算（不限）、无进展轮数（默认 3）；取消/开始。 |
| 040 | 设置-编辑个人资料 | 点击“编辑名字和头像” | 头像预览与“更换头像”；名字输入含当前 Wang；说明名字和头像保存在账号中并跨设备同步；保存/取消。 |
| 041 | 设置-im个人微信连接门 | 展开个人微信卡片（未连接） | 新对话配置含 Agent/模型/权限；新对话工作目录；连接说明：私聊消息进入本机 Cindy，腾讯授权网页可能显示 OpenClaw；最终“连接微信”按钮。 |
| 042 | 侧栏-全局搜索输入 | 点击侧栏搜索 | 侧栏顶部搜索框“搜索标题或内容…”；筛选/排序按钮；下方任务树保持可见。 |
| 043 | 侧栏-搜索筛选排序菜单 | 点击搜索旁筛选按钮 | 筛选维度：排序（相关度）、状态（全部）、项目（所有项目）、Agent（全部）、最近活跃（全部）。 |

## Asset layout

- Originals: /Users/smallraw/.cache/cindy-deep-dive/screenshots/original/
- Linework source: /Users/smallraw/.cache/cindy-deep-dive/screenshots/linework/
- Delivery alias (lineart): /Users/smallraw/.cache/cindy-deep-dive/screenshots/lineart/
- Machine-readable manifest: /Users/smallraw/.cache/cindy-deep-dive/ui/manifest.jsonl
