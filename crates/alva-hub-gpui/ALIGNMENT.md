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
