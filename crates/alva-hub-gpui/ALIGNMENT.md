# GPUI 版 1:1 功能对齐清单

> 对齐原则（用户）：**功能、位置、操作路径必须一致**；UI 样式因组件库不同可微差。
> 对照源：React 版 App.tsx（Tauri 版，保留运行作参考）。
> 状态更新：批 1-7 已完成（结构 + 执行流 + 补全），见下方逐项矩阵。

## React 版功能/位置清单（对齐基准）

### A. Sidebar（左栏）
| # | 功能 | 位置/操作 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| A1 | 品牌区（ALVA Hub + "One project. Every engine."） | 顶部 | brand block（mark + title + tagline） | ✅ |
| A2 | Workspace 面板：显示路径 + **Choose workspace** 按钮 | 品牌下 | 面板 + 目录选择器（reset + 重载 policy/readiness） | ✅ |
| A3 | Sessions：**搜索框** + **New session** + 会话列表 | 面板 | 搜索/新建/列表/相对时间/结果/active 高亮/空态 | ✅ |
| A4 | Engines：readiness 卡片 + **Refresh** + repair | 面板 | SystemProbe 探测 + 刷新 + one-click repair + 官方指引 | ✅ |
| A5 | Fallback chain：目标列表 + **Save** | 面板（折叠） | model/provider 编辑 + authorize + 排序 + session/default save | ✅ |
| A6 | Team：worker 列表 + dispatch + 生命周期 + **End run** | 面板（折叠） | 完整管理面 + per-worker queue pump（真实执行） | ✅ |
| A7 | 折叠机制（fallback/team 可收起） | 面板头 | collapsed map + chevron 按钮 | ✅ |

### B. Topbar（顶栏）
| # | 功能 | 位置 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| B1 | Engine 选择（alva/claude/codex） | 左 | Select ✓ | ✅ |
| B2 | Permission 选择（3 档，session 固定 + 提示） | Engine 右 | Select + Fixed 提示 + eligibility 详情 | ✅ |
| B3 | Details toggle（开 drawer） | 右 | Details/Hide details + drawer | ✅ |
| B4 | status pill（running/completed/…/Ready） | 最右 | 12 态动态 label + 语义色 | ✅ |

### C. Main（主区）
| # | 功能 | 位置 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| C1 | Timeline 卡片（多态） | topbar 下 | 9 态投影 + 实时 run 事件 | ✅ |
| C2 | fallback 卡片控件：**Stop / Switch back / View handoff** | 卡片内 | 齐全（Stop 经 active_engines + store cancel） | ✅ |
| C3 | permission 卡片：**Allow / Deny** | 卡片内 | 齐全（router 应答 + 审计 + answered 防重发） | ✅ |
| C4 | Empty state（图标+标题+描述+会话数） | 居中 | ✓ | ✅ |
| C5 | Composer：输入 + **Run with {engine}** | 底部 | multiline + 随引擎文案 + model/provider 行 + summary | ✅ |
| C6 | composer readiness 警告（选中引擎不健康） | composer 上 | ✓ | ✅ |

### D. Details drawer（详情抽屉）
| # | 功能 | 位置 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| D1 | Branches / Attempts / Handoffs / Memory / Diagnostics | 右抽屉 | 全部实现 + envelope 展开 + 添加 memory | ✅ |

### E. 审批组
| # | 功能 | 位置 | GPUI 现状 | 差距 |
|---|---|---|---|---|
| E1 | 聚合审批卡片（N workers + Allow all / Deny all） | 主区 | ✓（respond_many + 逐会话审计） | ✅ |

## 对齐状态矩阵（逐项核查，2026-08-12）

> ✅=已对齐 ⏳=部分 ⚠️=样式差异（允许） ❌=未对齐

### 窗口 / 布局
| 项 | React | GPUI | 状态 |
|---|---|---|---|
| 主窗口（两栏 sidebar+main） | ✓ | ✓ | ✅ |
| 窗口尺寸/最小尺寸 | 1180×760 min 860×600 | WindowOptions | ⚠️ 需按旧版设 bounds |
| Details 抽屉 | ✓ 右滑出 | ✓ 右侧固定面板 | ✅ |

### Sidebar
| 项 | React | GPUI | 状态 |
|---|---|---|---|
| 品牌区+tagline | ✓ | ✓ | ✅ |
| Workspace：路径+Choose 按钮 | ✓ | ✓ | ✅ |
| Sessions 空态 | ✓ "Hub runs will appear here." | ✓ 同文案 | ✅ |
| Sessions 数据态 | ✓ 名称/引擎/状态/相对时间/结果 | ✓ | ✅ |
| Sessions 搜索过滤 | ✓ 即时过滤+计数 | ✓ | ✅ |
| New session 按钮 | ✓ | ✓ | ✅ |
| Engines readiness 卡片 | ✓ 版本/认证/stream/能力/reasons+Refresh+修复 | ✓ | ✅ |
| Fallback 链编辑 | ✓ model/provider/authorize/排序+Save | ✓ | ✅ |
| Team（worker/调度/生命周期） | ✓ | ✓ | ✅ |
| 折叠（fallback/team） | ✓ aria-expanded | ✓ chevron | ✅ |

### Topbar
| 项 | React | GPUI | 状态 |
|---|---|---|---|
| Engine 选择 | ✓ | ✓ | ✅ |
| Engine 选项后缀/禁用（ineligible/不健康） | ✓ | 静态选项 | ⚠️ 视觉细节 |
| Permission 选择 | ✓ | ✓ | ✅ |
| status pill 动态 | ✓ | ✓ | ✅ |
| Details 按钮 | ✓ | ✓ | ✅ |

### Main
| 项 | React | GPUI | 状态 |
|---|---|---|---|
| 空态（图标/标题/描述/会话数） | ✓ | ✓ | ✅ |
| Timeline 卡片多态 | ✓ | ✓ | ✅ |
| fallback 卡片控件（Stop/Switch back/View handoff） | ✓ | ✓ | ✅ |
| handoff 完整信封（goal/instruction/work/files/skills/MCP/transcript） | ✓ | ✓ | ✅ |
| permission Allow/Deny | ✓ | ✓ | ✅ |
| Composer 输入 | ✓ | ✓ | ✅ |
| Run with {engine} | ✓ | ✓ | ✅ |
| 提交→状态更新 | ✓ | ✓ | ✅ |

### 逻辑（行为）
| 项 | React/后端 | GPUI | 状态 |
|---|---|---|---|
| run_persisted（真实引擎执行） | ✓ 139 测试 | ✓ UI 接线 | ✅ |
| 引擎切换/fallback 链 | ✓ | ✓ | ✅ |
| 审批路由/分组 | ✓ | ✓ | ✅ |
| 团队调度/资源声明/worker pump | ✓ | ✓ | ✅ |
| worktree 隔离/adoption | ✓ | ✓（review/confirm/discard） | ✅ |
| 崩溃恢复 | ✓ | 后端迁入 ✓ | ✅（store 层） |
| 会话持久化/重启恢复 | ✓ | 后端迁入 ✓ | ✅（store 层） |

### 结论
- 功能/位置/操作路径与 React 版 1:1 对齐完成（批 1-7）。
- 剩余：P4（GPUI 测试 + 打包）、样式微差（组件库不同，允许）、窗口尺寸按旧版设置。
