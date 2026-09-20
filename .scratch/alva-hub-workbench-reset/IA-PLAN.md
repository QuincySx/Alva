# H03 · IA-PLAN：全 UI 信息架构与交互状态方案（待用户确认）

- 日期：2026-09-21；依赖：H02 SPEC（产品方向已冻结）。
- 状态：**结构性方案，未经用户确认不实施大面积改动**（H03 卡验收要求）。确认后按 §8 批次落地，H10–H14/H17 在对应批次内实施。
- 组件策略：复用现有 Tokens → Controls → Regions/Surfaces → Domain → Pages 五层体系（ADR-0006），不推翻组件库；本文只调整信息架构、导航与状态，不新增视觉语言。
- 事实底稿：本方案的所有"现状"描述均有 文件:行号 依据（见 §9 勘察记录），不是回忆。

## 1. 页面清单（终态）

| # | 页面/状态 | 入口 | 现状 | 方案 |
|---|---|---|---|---|
| P0 | StartupGate（两模式选择） | 启动 / 无会话且无 workspace | 已实现（H08） | 保持；仅按 §3 调整项目模式聚合 |
| P1 | 会话页（对话模式） | New session / 打开历史会话 | 已实现 | 保持；§2 中央轴方案已由 H09/H21 落地 |
| P2 | 会话页（项目模式） | 项目行 → 会话 | 已实现（H08） | 保持 |
| P3 | Settings | 侧栏导航 | 已实现，10 section | §5 清理 |
| P4 | Automations | 侧栏导航 | 静态展示 | §5.1：诚实占位降级为入口说明 |
| P5 | Extensions/Plugins | 侧栏导航 | 静态展示 | §5.1 同上 |
| P6 | Search | 侧栏导航 | 会话列表 + 快捷动作 | §4：接真实搜索语义 |
| P7 | Workbench 独立窗口 | `#/sidebar-window` / 弹出按钮 | 已实现 | 保持 |
| P8 | UI Lab / perf harness | `?ui-lab=1` / `?perf=1` | dev-only | 不进产品导航（保持） |

导航路径无死路要求：P4/P5/P6 的每个可点击控件要么接真实后端，要么改为不可操作状态+原因（沿用 R31/R32 惯例），不允许"能点但无事发生"。

## 2. 中央阅读轴（已落地，方案确认保留）

- 单轴顺序：用户/助手消息 + 工具调用/结果 + 审批 + 状态卡（H09，commit `4b946a79`），窗口化渲染（H21，`bounded-list.tsx`）。
- ChatTranscript（prose）与 HubTimeline（执行/审批/降级卡）按 H20 timeline-model 的稳定 id 分块投影到同一滚动区域，视觉上是一条轴。
- 分组审批卡出现在 composer 坞上方（App.tsx:1898-1935），不挪出中央轴；单请求审批在 timeline 卡内联。
- **本方案不动这个结构**；未来任何"收拢"（折叠/摘要）只作为展示策略叠加在 TimelineEntry 投影层，权威记录不动（SPEC §1）。

## 3. 启动与项目导航（两模式，H08 已落地 + 一处补强）

- 保持：StartupGate 两模式均为正式入口；入口选择持久化；模式切换只改列表过滤范围，不搬迁会话（App.tsx:828-845 按会话自身 project_id 过滤）。
- **补强（待确认）**：项目模式打开一个项目后，项目/工作树信息现在只存在于 StartupGate；进入会话页后顶栏只显示 workspace 名（hub-topbar.tsx:39-85）。方案：侧栏 Sessions 组上方增加一个只读"当前项目"行（项目名 + 工作树状态 Badge，数据全部来自既有 listProjects/discoverProjectWorktrees 投影），点击回到 StartupGate 的项目列表。不新增权限、不改 cwd 契约。
- 非 Git 目录进项目模式：保持 H04/H05 语义（非 Git 是状态不是错误，`code: "not_git_repository"`），UI 已如实显示；**不做** git init。

## 4. 搜索（P6 重定义，小改）

现状：SearchPage 是"会话过滤 + 三个快捷动作"，显示 ⌘K 徽标但无绑定（product-pages.tsx:669）。
方案（待确认）：
1. SearchPage 文案改为"会话搜索"，去掉 ⌘K 徽标（或实现绑定，二选一，默认前者）。
2. 会话内消息搜索：H21 的窗口化使"全量 DOM 搜索"不可依赖；方案是在 SearchPage 内基于既有 `getHubSession` detail 的 turns/timeline 做后端侧过滤（数据已在本地 store/DTO，不发新 IPC），结果行点击跳转对应会话。不做 transcript 内滚动定位（依赖窗口化重建，成本高、易碎，列为暂缓并如实标注）。
3. 全局快捷键（⌘K/⌘N/⌘B）：Settings keybindings section 已是纯展示。方案：本期只实现 ⌘N（New session）与 ⌘B（Workbench 开关）两个真实绑定，并在 keybindings section 把未实现的键从展示中移除（诚实原则，不留假徽标）。⌘K 搜索绑定与搜索实现同批。

## 5. 设置与次级页清理（H17 范围，先立规则）

规则（对全部 section 生效）：禁用控件必须带原因文案（现状大多数已有"未接线"说明）；纯静态展示行降级为文本说明（不渲染成可交互控件）；本地 state 无消费者的开关删除（如 experiments "Show inspector"，product-pages.tsx:299,606——R79 洗过一遍仍漏）。
- **5.1 Automations / Extensions**：模板卡与 install 按钮全部改为"说明 + 不可用原因"（不可点击），保留数据与入口可发现性；不伪造安装/保存流。
- **5.2 keybindings**：见 §4.3。
- **5.3 IM section**：三行静态 Badge 改为一段说明文本（"未接入"），删除假表格结构。
- **5.4 theme light/system、密度、reduced-motion 开关**：保持禁用（H02 暂缓清单），但 reduced-motion 行说明改为"跟随系统设置（已生效）"——OS 媒体查询是真的在生效（primitives.css:392-412），现在的文案没有体现这一点。
- **5.5 Composer "+" 附件按钮**：`onAttach` 未传、纯装饰（hub-composer.tsx:67-72）——直接移除按钮，而不是留一个点了没反应的图标。

## 6. 工具区（Workbench）左右可逆方案（响应 SPEC §3 未决项）

约束：用户两次说"左边"，现行规范是右侧；未经确认不得当作已接受决策。
方案（可逆、token 化）：
1. 布局列方向抽成布局 token：`--hub-workbench-side: right`（现值）。Workbench 列在 grid 中的顺序由该 token 决定（一处 CSS 变量 + 一处 grid template 变体），拖拽宽度、overlay 断点（≤1023px 变抽屉）、`inert` 关闭区逻辑全部不变。
2. 运行时开关放在 View 菜单（顶栏），写 localStorage，默认 `right`。**默认值保持右侧**直到用户明确决策；开关本身让"左/右"成为 30 秒可验证的体验，而不是纸面争论。
3. 确认后：把选定值写进 `docs/ALVA-HUB-UI-WIREFRAMES.md` 与词汇表，token 保留（可逆性不删除）。
4. H12（Browser）/H13（Terminal）按各自卡实施；本方案只保留它们的 tab 位（browser/terminal 现为诚实禁用态，接线前不改结构）。

## 7. 交互状态矩阵（中英文 / 键盘 / reduced-motion / 窄窗口）

- 中英文：所有新增文案走 typed i18n dictionary（R37 惯例）；本方案涉及的文案清单在实施时随批次提交，先英文后中文补全不允许（同 commit 双语齐）。
- 键盘：现状全局仅 Esc 关抽屉（R35）。本方案新增 ⌘N/⌘B（§4.3）；焦点管理保持既有惯例（workspace 选完回焦 composer、Workbench 关闭回焦 topbar、drag handle 方向键）。折叠/展开区域一律用现有 Disclosure（Radix Collapsible，键盘可达）。
- reduced-motion：不新增动效；新增 UI 遵循 docs/ALVA-HUB-UI-MOTION.md 频率/时长表；OS 媒体查询路径保持唯一权威，Settings 开关保持禁用并按 §5.4 改文案。
- 窄窗口：现有断点（1359/1023/759/640）保持；新增"当前项目"行在 ≤759px 随侧栏 off-canvas；Workbench 左侧化不影响断点行为（token 只改列顺序）。最小宽度 320px 保持。
- 空/错误/不支持态：沿用现有 EmptyState/Notice 体系；每个新增区域必须同批交付 loading/empty/error 三态（现行惯例）。

## 8. 落地批次（确认后执行，每批独立提交+证据）

| 批次 | 内容 | 对应卡 |
|---|---|---|
| B1 | §5 设置/次级页诚实化清理（含 §4.3 快捷键二选一、§5.5 附件按钮移除） | H17 主体 |
| B2 | §4 搜索重定义 + ⌘K（若选实现） | H17 |
| B3 | §6 Workbench 左右 token + 顶栏开关（默认 right） | H03 未决项 |
| B4 | §3 "当前项目"行 | H08 补强 |
| B5 | H10 文件浏览 / H11 Git diff / H12 Browser / H13 Terminal / H14 Inspector 整理（各自卡的验收为准） | H10–H14 |
| B6 | H18 真实 Tauri 全流程验收（含 B1–B5 的桌面壳复测） | H18 |

## 9. 待用户确认的决策点

1. §6 工具区默认位置：**保持右侧 + 提供左右开关**（推荐，可逆），还是直接改左侧为默认？
2. §4.3 快捷键：只删假徽标（推荐，诚实最小改），还是实现 ⌘K 全局搜索？
3. §3 "当前项目"行是否需要？（纯增量只读，不影响权限）
4. §5.1 Automations/Extensions 静态展示保留还是整页降级为"未接入"说明页？（推荐保留结构+诚实化，保住 H17"可发现入口"要求）

## 10. 勘察事实索引（本方案的现状依据）

- 导航状态机：App.tsx:199（HubPage 五值）、224-240（URL 门控）、1810/2053-2086（条件渲染）；无路由库。
- entry-mode 过滤：App.tsx:519-522（持久化）、828-845（会话过滤）、1013（browser 不支持）。
- 中央轴：App.tsx:1868-1935；hub-timeline.tsx:50-59（prose 过滤）、178-238（审批内联）。
- Workbench 13 tab 现状：App.tsx:184-216；workbench-tools.tsx（files 真实 32-45 / text 预览未接 63-67 / git 非真 status 72-91 / browser 未接 539-553 / terminal 禁用 555-567 / inspector 真实 184-497）。
- Settings 10 section 现状：product-pages.tsx:259-270,330-341 与 §5 各条目行号。
- 死路清单：§5 与 §1 所列（composer 附件 67-72、⌘K 徽标 669、Browser/Terminal、IM 静态、experiments 无消费者 299/606 等）。
- 键盘/reduced-motion/窄窗口：primitives.css:392-465、ui/layout.tsx:29-103、App.tsx:573-586、hub-topbar.tsx:39-85。
