# ALVA Hub ASCII UI Wireframe Specification & Master Index

> 本目录是基于 Cindy 0.1.44 / 0.1.45 本地交互审计与 ALVA Hub 架构规约重新绘制的 **高精度 ASCII 线稿库**。
> 取代原有粗糙的 OpenCV 边缘过滤图（Sobel Lineart）与散乱 SVG，作为后续 Alva Hub UI（Tauri / GPUI）开发、审查与验收的 **唯一标准线稿依据**。

---

## 1. 结构化专题线稿 (Thematic Aggregate Wireframes)

| 编号 | 模块分类 | 包含核心状态与弹层 | 对应专题线稿 |
|---|---|---|---|
| **01** | **聊天主界面与时间线** | 桌面三栏架构、单阅读轴时间线、工具活动行（折叠/展开）、代码审查卡片、计划浮层、长对话长图分享选区 | [01-chat-shell-and-timeline.md](./01-chat-shell-and-timeline.md) |
| **02** | **新会话与输入区体系** | 空白新会话首屏、快速开始模板、项目/纯对话切换浮层、Composer "+" 操作菜单、Goal 自主目标弹窗、权限选择、模型选择器 | [02-new-conversation-and-composer.md](./02-new-conversation-and-composer.md) |
| **03** | **右侧多标签工作台** | 多标签 TabBar、代码审查与 Diff 视图、添加标签菜单、系统与进程资源用量表、Subagent 树状态、独立窗口化工作台 | [03-right-workbench.md](./03-right-workbench.md) |
| **04** | **自动化任务系统** | 自动化任务列表、模板推荐网格、创建/编辑自动化表单、Cron 及相对时间调度器 | [04-automation.md](./04-automation.md) |
| **05** | **插件市场与技能管理** | 插件/技能双标签首页、已安装/推荐插件卡片、插件详情/配置/权限弹窗、本地技能库管理 | [05-plugins-and-skills.md](./05-plugins-and-skills.md) |
| **06** | **系统设置与配置中心** | 设置双栏导航、通用与外观设置、Worker并发控制、模型供应商管理与排序、自定义端点表单、远程 SSH 主机、IM 机器人通道 | [06-settings-and-account.md](./06-settings-and-account.md) |
| **07** | **上下文菜单与全局浮层** | 项目右键菜单、会话右键菜单、侧栏全局搜索与多维筛选、侧栏整理弹层、版本更新公告弹窗 | [07-context-menus-and-overlays.md](./07-context-menus-and-overlays.md) |

---

## 2. 全量 43 个状态 1:1 独立线稿 (1:1 Granular State Wireframes)

> 针对 `/Users/smallraw/.cache/cindy-deep-dive/screenshots/original/` 的全部 43 张真实状态原图，我们提供了 **1:1 独立精细线稿**，详见：
> 📋 **[ALL-43-STATES-CATALOG.md](./ALL-43-STATES-CATALOG.md)** (含全部 001 ~ 043 单态文件与原图对照)

- 状态 001~005：更新公告、聊天主视图、侧栏整理、项目右键、任务选中态
- 状态 006~008：自动化列表、模板编辑器、Cron 选择器
- 状态 009~018：设置总览、外观排版、Worker调度、模型供应商排序与余额、添加供应商、自定义端点
- 状态 019~023：内置工具与 MCP、远程 SSH 主机列表及连接表单
- 状态 024~030：计划进度、权限模式、模型选择器、Composer `+` 菜单、会话右键、长图分享选区与全选
- 状态 031~035：右栏工作台展开、进程资源用量表、独立新窗口 (`#/sidebar-window`)、Subagent 空态、添加标签
- 状态 036~039：新建空白草稿、纯对话/项目选择器、Goal 自主目标草稿与高级限制
- 状态 040~043：编辑资料、微信 IM 门禁、侧栏全局搜索输入、多维筛选排序菜单

---

## 2. 空间布局与几何规约 (Spatial Layout & Geometry)

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│  AppFrame (Width: ≥ 1360px 宽屏桌面模式)                                                         │
├──────────────────────────────┬────────────────────────────────────┬──────────────────────────────┤
│ [HubSidebar]                 │ [MainContent]                      │ [Workbench (Docked)]         │
│ 宽度: 248px (固定)           │ 宽度: 自适应                       │ 宽度: 520px (420px~800px可调)│
│ 内容:                        │ 阅读轴最大宽: 820px                │ 内容:                        │
│ - 品牌与全局操作             │ 内容:                              │ - 多标签条 (TabBar)          │
│ - 项目分组与会话树           │ - TopBar 会话标头                  │ - 工具页 (Review/Term/Tree)  │
│ - 底部账户与设置入口         │ - Timeline (单阅读轴滚动)          │ - 底部独立/最大化控制        │
│                              │ - DockRegion (常驻底栏/Composer)   │                              │
└──────────────────────────────┴────────────────────────────────────┴──────────────────────────────┘
```

### 响应式视口层级规约 (Responsive Viewport Rules)

1. **宽屏桌面 (≥ 1360 px)**：三栏并存（`Sidebar` + `MainContent` + 停靠式 `Workbench`）。
2. **标准桌面 (1024 px ~ 1359 px)**：`Sidebar` 紧凑模式；`Workbench` 仍可停靠，压缩主阅读轴两侧空白。
3. **平板与窄屏 (760 px ~ 1023 px)**：`Workbench` 变为浮层抽屉（Overlay Sheet），不挤压主聊天流。
4. **紧凑窗口 (< 760 px)**：`Sidebar` 与 `Workbench` 均为抽屉；`Composer` 采用双行折叠控制。

---

## 3. 字符与符号规范 (ASCII Notation & Design Tokens)

- **边框与容器**：使用规范 Unicode Box 字符（`┌─┐`、`│ │`、`└─┘`、`├──┤`、`┬`、`┴`、`┼`）。
- **滚动标识**：`[SCROLL]` 标注页面唯一的滚动宿主容器。
- **常驻停靠**：`[DOCK]` 标注底部悬停或常驻区域（如 `Composer`、分享底栏）。
- **浮层弹窗**：`[OVERLAY]` / `[DIALOG]` 标注通过 Portal 弹出的临时菜单、Popovers 与模态框。
- **状态指示器**：
  - `[●]` 选中 / 激活 / 运行中
  - `[○]` 待执行 / 未选中
  - `[✓]` 已完成 / 校验通过
  - `[!]` 警告 / 错误
  - `▼` / `▶` 折叠展开标记
