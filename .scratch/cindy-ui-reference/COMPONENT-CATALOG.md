# ALVA Hub 组件目录

本目录从 Cindy 的 67 张逐状态线稿抽取，不从现有 Hub 页面反推。基础交互优先使用 `gpui-component`，只有带 ALVA 业务语义的组合才进入 Hub 领域组件。

## 一、基础组件

| 交互结构 | 线稿状态 | gpui-component | Hub 使用约束 |
|---|---|---|---|
| 三栏主壳 | 01、34、35 | `DockArea`、`Panel` | 左导航、中央会话、右工作台均由 Dock 管理 |
| 左侧导航 | 01、02、34 | `Sidebar`、`SidebarMenu`、`SidebarMenuItem` | 项目、会话和一级入口不得手搓点击行 |
| 页面设置 | 15–28 | `Settings`、`SettingPage`、`SettingGroup`、`SettingItem`、`SettingField` | 禁止自建设置左栏和设置行 |
| 页面标签 | 11、60 | `TabBar`、`Tab` | 插件/技能等平级页面切换 |
| 右栏标签 | 35–48 | `DockArea` 内置 TabPanel | 支持关闭、排序、宽度和持久化 |
| 输入与搜索 | 02–07、14、51、65 | `Input`、`InputState`、`Select`、`Combobox` | 所有输入状态由 Entity 持有 |
| 菜单 | 03–07、10、36、53–58、61 | `DropdownMenu`、`PopupMenu`、`ContextMenuExt`、`Popover` | 项目、会话、模型、权限和新增标签统一菜单系统 |
| 主次操作 | 全部 | `Button`、`ButtonGroup` | 图标按钮必须带 `tooltip` |
| 状态表达 | 34、35、41、42、49、50 | `Tag`、`Badge`、`Spinner`、`Skeleton` | 使用语义变体，不硬编码状态颜色 |
| 折叠详情 | 49、50 | `Collapsible`、`Accordion` | 工具活动默认折叠，原地展开 |
| 对话框 | 09、13、64 | `Dialog`、`AlertDialog` | 自动化、插件详情和危险确认 |
| 侧栏抽屉 | 13、17、25 | `Sheet` | 较轻的详情和配置，不替代右侧工作台 |
| 表单 | 09、13、64 | `v_form`、`field` | 自动化和插件配置共享表单间距 |
| 文件与 Agent 树 | 37–39 | `Tree`、`List` | 文件树和递归 Agent 树使用虚拟化状态组件 |
| 终端/浏览器容器 | 43–48 | Dock Panel | 生命周期属于 HubSession，关闭右栏不终止进程 |

## 二、Hub 领域组件

| 组件 | 代码 | 组合基础 | 使用页面 |
|---|---|---|---|
| `HubSurfaceHeader` | `ui/components/surface.rs` | 标题排版 + `Button` action | 自动化、插件、独立页面 |
| `HubEmptyState` | `ui/components/surface.rs` | `Icon` + action | 新会话、自动化、右栏空状态 |
| `HubTemplateCard` | `ui/components/cards.rs` | `GroupBox` + `Icon` + `Tag` + `Button` | 自动化模板、快速开始 |
| `HubExtensionCard` | `ui/components/cards.rs` | `GroupBox` + `Icon` + `Tag` + `Button` | 插件、技能、引擎 |
| `HubExtensionDetail` | `ui/components/cards.rs` | `GroupBox` + `Icon` + `Tag` + `Sheet` | 插件、技能、引擎详情 |
| `HubQuickStartCard` | `ui/components/cards.rs` | `Button` + `Icon` + `Tooltip` | 新对话快捷开始 |
| `HubToolPlaceholder` | `ui/components/cards.rs` | `HubEmptyState` + `Tag` | 尚未接通的右栏标签 |
| `HubActivityRow` | `ui/components/chat.rs` | `Collapsible` + `GroupBox` + `Tag` + `Button` | 聊天工具活动、审批、fallback |
| `HubComposer` | `ui/components/chat.rs` | `GroupBox` + `Input` + `Select` + `Button` | 聊天底部 |
| `HubAutomationForm` | `ui/components/forms.rs` | `v_form` + `field` + `Input` + `Select` | 新建自动化 Dialog |
| `HubProjectPicker` | `ui/components/navigation.rs` | `DropdownMenu` + `PopupMenuItem` + 原生目录选择器 | 新会话和 Composer |
| `HubSessionItem` | `ui/components/navigation.rs` | `SidebarMenuItem` + `Tag` | 左栏会话 |
| `HubWorkbenchTabMenu` | `ui/components/navigation.rs` | `DropdownMenu` + `PopupMenuItem` + Dock | 右栏新增和切换标签 |

页面级状态提示统一使用 `gpui-component::Alert`；设置页直接使用 `Settings` 系列组件，不再经过 Hub 自建替代层。

## 三、拼装顺序

1. 主壳：Dock、Sidebar、TitleBar、StatusBar。
2. 聊天：消息、活动行、Composer、审批。
3. 右栏：动态标签菜单、文件、审查、Agent、Inspector、终端。
4. 自动化：空状态、模板、创建 Dialog、计划菜单。
5. 插件与技能：TabBar、卡片、详情 Sheet/Dialog、安装菜单。
6. 设置：组件库 Settings 全分类页面。
7. 项目与会话：Context Menu、搜索、导出、归档和删除确认。

## 四、验收门槛

- 67 张线稿全部有同名线稿文件，当前为 67/67。
- 每种交互结构先在本目录绑定基础组件，再进入页面代码。
- 设置、Dock、Sidebar、菜单、Dialog、Sheet 和 Tab 不允许自建替代品。
- 裸 `div()` 只能承担文字和局部布局；不得承担按钮、菜单、标签页、开关、输入或可折叠控件。
- 每完成一个页面，保存 Hub 同状态截图并与对应 Cindy 原图和线稿并排复核。
