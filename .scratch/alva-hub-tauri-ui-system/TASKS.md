# Execution Tasks

## 当前状态

规划、领域访谈和架构决策已完成，实施任务尚未启动。旧的未提交 UI 代码保留原状，进入任务 00 后先建立当前截图基线，再逐项决定迁入、重写或删除。

| ID | 任务 | 依赖 | 状态 | 可见交付 |
|---|---|---|---|---|
| 00 | 冻结参考图与当前界面 | 无 | 待开始 | 67 个状态的 reference/linework/current 三联索引 |
| 01 | 重画七张总层架 | 00 | 待开始 | 统一比例并标注组件、宽度、Dock、Scroll 与 Overlay 锚点 |
| 02 | 固化整体区域与缩放关系 | 01 | 待开始 | Sidebar/Main/Workbench、滚动和 Dock 在各窗口尺寸统一 |
| 03 | 建立 Token、密度与动效规则 | 00 | 待开始 | 深色语义 token、default/compact、统一时长曲线和静态检查 |
| 04 | 打磨基础控件与弹层 | 02、03 | 待开始 | 全部 L1 状态、弹层尺寸、键盘和可中断动画可独立验收 |
| 05 | 建立区域与 Surface 目录 | 02、04 | 待开始 | L2 积木覆盖七个页面族的重复结构 |
| 06 | 建立 UI Lab 与 Playwright | 03–05 | 待开始 | 确定性 fixture、组件状态矩阵、固定视口截图与差异图 |
| 07 | 重建 Chat Shell 与时间线 | 05、06 | 待开始 | 聊天、事件、审批、fallback 共用单一阅读轴 |
| 08 | 对齐 Composer 与模型菜单 | 04、06、07 | 待开始 | 新对话中实时选择 Agent、模型、权限和上下文 |
| 09 | 完成右侧 Workbench | 05–07 | 待开始 | Files/Review/Agent/Inspector/Tasks/Usage/Browser/Terminal 统一标签体系 |
| 10 | 完成自动化与插件/技能页 | 05、06 | 待开始 | 卡片、列表、详情和创建流程复用同一积木并接通真实业务 |
| 11 | 完成设置、搜索与上下文菜单 | 04–06 | 待开始 | 设置体系、全局搜索和项目/会话操作完整统一 |
| 12 | 完成视觉、动效与真实业务验收 | 07–11 | 待开始 | 67/67 状态、真实 store/Tauri、多窗口、键鼠触屏与 reduced-motion 全部通过 |

## 并行关系

```text
00 ─┬─ 01 ── 02 ─┬─ 04 ── 05 ── 06 ─┬─ 07 ── 08
    └─ 03 ───────┘                   ├─ 09
                                     ├─ 10
                                     └─ 11
                                        ↓
                                       12
```

01 和 03 可在基线冻结后并行；04 必须同时服从区域与 Token/动效规范；任何业务页面进入重建前，UI Lab 必须能单独展示其基础组件和区域状态。07–11 完成后统一进入 12，不允许以单页看起来接近为理由跳过真实业务或全状态验收。

## 每个任务的完成门槛

- 改动前保存同状态截图，改动后保存对应 current 截图；
- 组件 review 使用 `Before | After | Why`，不接受只写“更漂亮”；
- 每个组件覆盖 default/hover/active/focus/disabled/loading/open/close 中适用状态；
- 每个 overlay 检查位置、宽度、碰撞、滚动、键盘和快速反向；
- 每个 region 明确宽度、滚动、停靠和缩放所有权；
- default 与 compact 两档密度均由组件 variant 提供，页面不手写高度；
- 中英文 fixture 不破版，正文与控件满足 WCAG AA；
- 每个页面同时通过确定性 fixture 和真实 store/Tauri 流程；
- TypeScript、测试、Vite build 和 Tauri 构建按任务逐步验证；
- 任务 12 通过前不删除参考图、不宣告 UI 全部完成。
# Delivery status

- [x] Preserve 67 screenshots and 67 state lineworks
- [x] Redraw seven Tauri aggregate hierarchy SVGs
- [x] Establish Tokens → Controls → Regions → Domain → Pages
- [x] Add deterministic UI Lab
- [x] Add seven aggregate, model-picker, and narrow-Workbench browser baselines
- [x] Rebuild Chat, Composer, ModelPicker, Workbench, Automations, Extensions, Settings, Search, and ContextMenu paths
- [x] Pass 44 Vitest tests and Playwright visual regression
- [x] Native Tauri smoke on the packaged WebView
