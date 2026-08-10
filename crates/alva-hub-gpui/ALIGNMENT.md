# GPUI 版 1:1 功能对齐清单

> 对齐原则（用户）：**功能、位置、操作路径必须一致**；UI 样式因组件库不同可微差。
> 对照源：React 版 App.tsx（Tauri 版，保留运行作参考）。

## React 版功能/位置清单（对齐基准）

### A. Sidebar（左栏）
| # | 功能 | 位置/操作 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| A1 | 品牌区（ALVA Hub + "One project. Every engine."） | 顶部 | SidebarHeader "ALVA Hub" + footer tagline | 位置微差（tagline 在 footer） |
| A2 | Workspace 面板：显示路径 + **Choose workspace** 按钮 | 品牌下 | 菜单项占位 | **缺**（需文件对话框） |
| A3 | Sessions：**搜索框** + **New session** + 会话列表 | 面板 | 菜单项（读 store ✓） | **缺搜索/新建/相对时间/结果** |
| A4 | Engines：readiness 卡片（版本/认证/stream/能力/reasons）+ **Refresh** + repair | 面板 | 静态菜单项 | **缺探测/刷新/修复** |
| A5 | Fallback chain：目标列表（model/provider/authorize/排序）+ **Save** | 面板（折叠） | 静态菜单项 | **缺编辑/保存** |
| A6 | Team：worker 列表（lifecycle/isolated）+ dispatch + 生命周期按钮 + **End run** | 面板（折叠） | 静态菜单项 | **缺** |
| A7 | 折叠机制（fallback/team 可收起，展开态 accent） | 面板头 | SidebarMenu click_to_toggle ✓ | 微对齐 |

### B. Topbar（顶栏）
| # | 功能 | 位置 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| B1 | Engine 选择（alva/claude/codex） | 左 | Select ✓ | ✅ |
| B2 | Permission 选择（3 档） | Engine 右 | Select ✓ | ✅ |
| B3 | Details toggle（开 drawer） | 右 | 无 | **缺** |
| B4 | status pill（running/completed/…/Ready） | 最右 | Ready pill ✓ | 动态状态缺（P4 后） |

### C. Main（主区）
| # | 功能 | 位置 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| C1 | Timeline 卡片（completed/failed/fallback/worker/permission 5 态） | topbar 下 | 空状态占位 | **缺**（P4 事件流） |
| C2 | fallback 卡片控件：**Stop / Switch back / View handoff** | 卡片内 | 无 | **缺** |
| C3 | permission 卡片：**Allow / Deny** | 卡片内 | 无 | **缺** |
| C4 | Empty state（图标+标题+描述+会话数） | 居中 | ✓（已加计数） | ✅ |
| C5 | Composer：输入 + **Run with {engine}** 按钮 | 底部 | Input + Button ✓ | 按钮随引擎文案缺 |

### D. Details drawer（详情抽屉）
| # | 功能 | 位置 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| D1 | Branches / Attempts / Handoffs / Memory / Skills / MCP / Engine diagnostics | 右抽屉 | 无 | **缺**（sheet 组件） |

### E. 审批组
| # | 功能 | 位置 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| E1 | 聚合审批卡片（N workers + Allow all / Deny all） | 主区 | 无 | **缺**（dialog） |

## 对齐批次（施工顺序）

- **批 1（结构）**：Sidebar 面板结构对齐（A2-A6 面板化 + 折叠）+ Topbar B3 Details + 位置
- **批 2（执行）**：C1-C3 真实 timeline + composer Run → run_persisted（P4）
- **批 3（补全）**：A3 搜索/新建、A4 readiness 探测、A5 policy 编辑、A6 team、D1 drawer、E1 approval（P5）

## 验收方式

- 每个批次后：GPUI 版跑起来，与 React 版（dev server）逐项对照功能/位置/操作
- 样式差异允许（组件库不同），**操作路径与信息展示必须一致**

## 对齐状态矩阵（逐项核查，2026-08-10）

> ✅=已对齐 ⏳=部分（结构有/内容缺） ❌=未对齐
> 样式差异允许（组件库不同），功能/位置/操作必须一致。

### 窗口 / 布局
| 项 | React | GPUI | 状态 |
|---|---|---|---|
| 主窗口（两栏 sidebar+main） | ✓ | ✓ | ✅ |
| 窗口尺寸/最小尺寸 | 1180×760 min 860×600 | WindowOptions 默认 | ⏳ 需对齐 |
| Details 抽屉 | ✓ 右滑出 | 按钮在，内容未接 | ❌ |

### Sidebar
| 项 | React | GPUI | 状态 |
|---|---|---|---|
| 品牌区+tagline | ✓ | ✓ | ✅ |
| Workspace：路径+Choose 按钮 | ✓ | 菜单项+目录选择器 | ⏳ 位置/交互形态不同 |
| Sessions 空态 | ✓ "Hub runs will appear here." | "(no sessions yet)" | ⏳ 文案不同 |
| Sessions 数据态 | ✓ 名称/引擎/状态/相对时间/结果 | 名称·引擎·状态 | ⏳ 缺相对时间/结果 |
| Sessions 搜索过滤 | ✓ 即时过滤+计数 | ❌ | ❌ |
| New session 按钮 | ✓ | ❌ | ❌ |
| Engines readiness 卡片 | ✓ 版本/认证/stream/能力/reasons+Refresh+修复 | 静态占位 | ❌ |
| Fallback 链编辑 | ✓ model/provider/authorize/排序+Save | 静态占位 | ❌ |
| Team（worker/调度/生命周期） | ✓ | 静态占位 | ❌ |
| 折叠（fallback/team） | ✓ aria-expanded | SidebarMenu 折叠 | ⏳ 需验证交互 |

### Topbar
| 项 | React | GPUI | 状态 |
|---|---|---|---|
| Engine 选择 | ✓ | ✓ | ✅ |
| Permission 选择 | ✓ | ✓ | ✅ |
| status pill 动态（running/completed/…） | ✓ | 静态 Ready | ❌ |
| Details 按钮 | ✓ | ✓ | ✅（内容未接） |

### Main
| 项 | React | GPUI | 状态 |
|---|---|---|---|
| 空态（图标/标题/描述/会话数） | ✓ | ✓ | ✅ |
| Timeline 卡片 5 态 | ✓ | ❌ | ❌ |
| fallback 卡片控件（Stop/Switch back/View handoff） | ✓ | ❌ | ❌ |
| permission Allow/Deny | ✓ | ❌ | ❌ |
| Composer 输入 | ✓ | ✓ | ✅ |
| Run with {engine} | ✓ | 静态 "Run with alva" | ⏳ 文案不随引擎 |
| 提交→状态更新 | ✓ | ❌ | ❌ |

### 逻辑（行为）
| 项 | React/后端 | GPUI | 状态 |
|---|---|---|---|
| run_persisted（真实引擎执行） | ✓ 139 测试 | 后端迁入✓，UI 未接线 | ❌ |
| 引擎切换/fallback 链 | ✓ | 未接线 | ❌ |
| 审批路由/分组 | ✓ | 未接线 | ❌ |
| 团队调度/资源声明 | ✓ | 未接线 | ❌ |
| worktree 隔离 | ✓ | 后端迁入✓，UI 未接 | ❌ |
| 崩溃恢复 | ✓ | 后端迁入✓ | ✅（store 层） |
| 会话持久化/重启恢复 | ✓ | 后端迁入✓ | ✅（store 层） |

### 结论
- **结构骨架（布局/选择器/空态）约 1/3 对齐**；**功能与逻辑（timeline 事件流、
  fallback/审批/团队/worktree UI、真实执行）绝大部分 ❌**——批 2（执行流）+ 批 3
  （完整功能）尚未做。
- 后端逻辑层（store/run/claims/…139 测试）已 100% 迁入 GPUI 版——**逻辑可用，只差 UI 接线**。
- 诚实结论：**没有每一个都 OK**，批 2/批 3 是主要工作量。
