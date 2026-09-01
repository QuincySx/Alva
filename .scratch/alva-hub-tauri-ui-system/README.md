# ALVA Hub Tauri UI System Roadmap

> 2026-08-12：本任务集取代旧 GPUI UI 迁移任务。产品入口以
> `crates/alva-hub-desktop` 的 Tauri + React 实现为准。
>
> 2026-08-30：任务 00–12 已完成并通过最终视觉、测试与 Tauri package
> 验收；后续发布工作转入 `../alva-hub-v1-release-readiness/`。

## 目标

先把页面区域、组件职责和动效语义固定下来，再拼业务页面。禁止页面自己决定弹层宽度、滚动层、停靠位置或基础控件状态。正式决策见 `docs/adr/0006-hub-tauri-ui-system.md`。

参考基线：

- Cindy 本地截图：`.scratch/cindy-ui-reference/screenshots/`，67 个交互状态；
- Cindy 逐状态线稿：`.scratch/cindy-ui-reference/linework/`，67/67；
- Cindy 页面总线稿：`.scratch/cindy-ui-reference/wireframes/`，7 个页面族；
- 动效原则：`emilkowalski/skills` 的 `emil-design-eng`、`animate`、`review-animations`、`improve-animations` 与 `find-animation-opportunities`。

## 不可变规则

1. 组件依赖固定为 `Tokens → Controls → Regions/Surfaces → Domain → Pages` 五层；页面组合时按 `AppShell → Region → Surface → Control` 收口。
2. 页面只能组合公开 `ui/` 组件，不直接导入 Radix，不创建原生交互控件。
3. `tokens.css` 拥有颜色、尺寸、层级、时长和 easing；页面不得声明新的全局常量。
4. `primitives.css` 拥有控件状态和动效；业务 CSS 只允许组合布局。
5. 一个区域只有一个滚动拥有者。外层与内层不得同时 `overflow: auto`。
6. 高频导航和键盘操作不做进入/退出动画，只保留颜色反馈。
7. 动画必须回答“为什么需要”；只为反馈、空间关系、状态解释或避免突变服务。
8. 常规 UI 动画不超过 300 ms；只动画 `transform` 和 `opacity`。
9. 所有移动动效提供 `prefers-reduced-motion` 降级，保留必要的颜色和透明度反馈。
10. 每个页面状态必须与同名 Cindy 原图和线稿并排验收。

## 里程碑

| 里程碑 | 任务 | 可见结果 |
|---|---|---|
| M0 蓝图冻结 | 00–02 | reference/current/target 基线与七张统一总层架完成 |
| M1 组件底座 | 03–06 | Token、动效、控件、区域和 UI Lab 可独立验收 |
| M2 Chat Shell | 07–08 | 新会话、已有会话、Composer、模型菜单完整对齐 |
| M3 Product Pages | 09–11 | 右侧工作台、自动化、插件/技能和设置共享同一体系 |
| M4 验收 | 12 | 67 个状态、真实业务、多窗口、键盘和减弱动效全部通过 |

## 执行顺序

`00 → 01 → 02 → 03 → 04 → 05 → 06 → 07 → 08 → 09 → 10 → 11 → 12`

基础组件完成前不得直接大改页面；页面验收完成前不得删除旧对照资料。

## 启动时审计结论（历史）

下表记录 2026-08-12 规划启动时的差距，用于解释任务来源，不再代表当前实现状态。
当前完成证据以 `TASKS.md`、Ticket 12 与 `crates/alva-hub-desktop/ACCEPTANCE.md`
为准。

| 项目 | 当前状态 | 处理 |
|---|---|---|
| 业务层原生 `button/input/select/textarea` | 0 | 保持 CI 检查 |
| 业务层直接 Radix import | 0 | 保持 UI facade 边界 |
| 页面区域层级 | 已开始统一，尚未完整验收 | 任务 02、05、07、09 |
| 动效 token | 只有 fast/normal，无统一曲线 | 任务 03 |
| Popover/Dialog 动效 | 使用 keyframes，退出含 `ease-in` | 任务 04 |
| Disclosure | 动画 height，触发布局；不可平滑反转 | 任务 04 |
| 高频 Nav/Tab | 存在 active scale | 任务 03、04 |
| Tooltip 连续浏览 | Provider 有 skip delay，CSS 未区分 instant | 任务 04 |
| reduced motion | 有基础规则，仍需逐组件验收 | 任务 12 |
| 组件目录 | 仍描述 GPUI，已过期 | 本任务集取代 |

## 交付文档

- `SCREEN-MAP.md`：67 个交互状态归入 7 个页面族；
- `COMPONENT-HIERARCHY.md`：五层组件树、职责和禁止越级规则；
- `COMPONENT-REVIEW.md`：现有组件逐项归层、问题、动效决策和验收标准；
- `MOTION-SPEC.md`：按频率、目的、方向和组件类型定义动效；
- `TASKS.md`：任务状态、依赖和每一步的可见交付；
- `docs/adr/0006-hub-tauri-ui-system.md`：已接受的 UI 架构决策；
- `docs/ALVA-HUB-UI-GLOSSARY.md`：组件、区域、动效和验收词汇；
- `docs/ALVA-HUB-UI-WIREFRAMES.md`：七张总层架的重画规范；
- `issues/00-12`：可直接实施和验收的任务。
