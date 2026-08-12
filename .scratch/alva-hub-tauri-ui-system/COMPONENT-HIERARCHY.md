# Component Hierarchy

## 五层模型

```text
L0 Tokens
  color · type · space · radius · shadow · size · layer · motion
      ↓
L1 Controls
  Button · IconButton · Input · TextArea · Select · Checkbox · Switch
  Badge · Tooltip · Menu · Popover · Dialog · Disclosure · Tabs
      ↓
L2 Regions and Surfaces
  AppFrame · Sidebar · MainPane · Workbench · ContentFrame
  ScrollRegion · DockRegion · WorkbenchPage · PanelSection
  Notice · StatusListItem · EmptyState
      ↓
L3 Domain Components
  HubSidebar · HubTopbar · HubTimeline · HubActivityRow · HubComposer
  ModelPicker · SessionDetails · EngineReadinessPanel
  FallbackPolicyPanel · TeamPanel · AutomationForm · ExtensionCard
      ↓
L4 Pages
  Conversation · Automations · Plugins/Skills · Settings · Search
```

页面只能组合 L3/L2；领域组件只能组合 L2/L1；基础控件只能依赖 L0。低层不得反向依赖业务层。

## App Shell 组件树

```text
AppFrame
├─ HubSidebar
│  ├─ Brand
│  ├─ PrimaryNavigation
│  ├─ SearchField
│  ├─ ProjectGroup
│  │  └─ HubSessionItem[]
│  └─ WorkspaceAccountFooter
├─ MainPane
│  ├─ HubTopbar
│  └─ ContentFrame
│     ├─ ScrollRegion
│     │  ├─ HubTimeline
│     │  │  └─ HubActivityRow[]
│     │  └─ SessionDetails
│     └─ DockRegion
│        ├─ Notice[]
│        └─ HubComposer
└─ Workbench
   ├─ WorkbenchTabBar
   │  ├─ OpenToolTab[]
   │  ├─ AddTabMenu
   │  └─ CloseWorkbench
   └─ WorkbenchPage [active tool only]
```

## 区域职责

| 组件 | 拥有 | 不拥有 |
|---|---|---|
| `AppFrame` | 三栏关系、断点、全局层级 | 页面内容宽度 |
| `ContentFrame` | 主阅读宽度、水平 gutter | 子卡片宽度 |
| `ScrollRegion` | 当前区域唯一滚动 | 底部停靠内容 |
| `DockRegion` | Composer、审批和必须持续可见的状态 | 长内容滚动 |
| `Workbench` | 右侧整体宽度、覆盖/停靠模式、动态 tab 生命周期 | tab 内部卡片样式 |
| `WorkbenchPage` | tab 页唯一滚动和页面 padding | 第二层滚动容器 |
| `PanelSection` | 标题、说明、meta、action、body spacing | 页面级宽度 |
| `Popover/Dialog/Sheet` | portal、focus、碰撞、关闭、层级 | 业务数据 |

## 领域组件整理

| 组件 | 保留 | 应拆出 | 默认信息层级 |
|---|---|---|---|
| `HubTimeline` | 单一聊天阅读轴 | `HubActivityRow`、`PermissionNotice`、`FallbackNotice` | 摘要可见，执行明细折叠 |
| `HubComposer` | 输入与会话控制 | `ComposerToolbar`、`ComposerStatus` | 当前项目/Agent/模型可见，内部 ID 隐藏 |
| `ModelPicker` | 引擎与模型选择 | `EngineTabs`、`ModelRouteItem`、`ModelDetail` | 可用/不可用与原因可见，账单/provider 折叠 |
| `SessionDetails` | 用户主动打开的会话详情 | `MemorySection`、`HandoffSection`、`DiagnosticsSection` | Memory 可见，其余折叠 |
| `EngineReadinessPanel` | 可用性与修复 | `EngineStatusItem` | 状态和下一步可见，版本/日志折叠 |
| `FallbackPolicyPanel` | 顺序和授权 | `FallbackTargetItem` | 引擎、顺序、授权可见，provider/model 折叠 |
| `TeamPanel` | Worker 和任务 | `WorkerCard`、`TaskRow`、`TeamComposer` | 当前 worker/task 可见，claims/证据按需展开 |

## 禁止越级

- 页面不得设置 Popover/Dialog 的像素宽高；使用 overlay size variant。
- 领域组件不得声明 Button/Input/Tab 的 hover、active、focus、disabled。
- 业务 CSS 不得选择 `.ui-button`、原生 `button/input/select/textarea` 修改基础行为。
- 同一个视觉区域不得同时拥有两个 `overflow: auto`。
- 不为单一页面复制 `PanelSection`、`Notice`、`StatusListItem` 的边框和间距。
