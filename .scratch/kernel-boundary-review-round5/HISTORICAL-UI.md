# 历史 UI 与发布验收总审计

审查时点：`5c64af1`（2026-09-15）。本表按用户补充的“所有都要审查”扩展到历史卡的当前实现，不限于第五轮新增 diff。旧实现中发现的缺陷明确标为历史残留，不冒充本轮引入。只读源码、任务与证据账本；没有调用付费引擎、修改生产代码或重跑签名包。第四轮问题沿用 R40–R47，不重复建卡。

## 结论与口径

原 `.scratch/alva-hub-tauri-ui-system/TASKS.md` 声称 00–12 全完成、67/67 状态和真实工具/设置/自动化操作全部通过，**与当前实现不符**。后续 R20/R22 明确把自动化/安装降为不可用，R31/R32 禁用设置；这些是正确的诚实化修复，但不能反向证明原 UI10/11 已接通真实业务。

`SCREEN-MAP.md` 的 67/67 是参考截图和 linework 数量，自己列出的浏览器 baseline 只有 9 张；它不是 67 个当前实现状态逐条三联验收记录。任务 12 的 63 Vitest、11 Playwright 与一张 native smoke 也不能覆盖这个承诺。本次未删除旧历史证据，建议通过 R46 添加权威现状入口并纠正完成口径。

## 原 UI 系统 00–12 逐卡矩阵

| 卡 | 当前可保留成果 | 当前结论 / 缺口 | 后续映射 |
|---|---|---|---|
| 00 冻结参考 | SCREEN-MAP、原图与线稿引用存在 | 参考冻结可保留；没有 67 个状态 current 的逐条三联验收索引，不能宣称完整验收 | R45、R46 |
| 01 七张总层架 | `wireframes/01…07.svg` 七图存在，正式 wireframe 文档定义区域合同 | 绘图交付保留；未在本轮重新逐像素审美验收；chat-first 已改变主聊天执行图归属，文档需同步 | R42、R46 |
| 02 Region | AppFrame/ScrollRegion/DockRegion、四断点和可调 Workbench 已实现 | 部分完成；打开态 overlay 缺焦点转移/背景 inert/Esc；单滚动完整矩阵未证明 | R45 |
| 03 Token/密度/动效 | tokens、primitive、default/compact 与静态检查保留 | 主实现保留；新增聊天 magic values 与两密度/完整 motion 矩阵仍未闭合 | R47、R45 |
| 04 控件/Overlay | Radix facade、键盘控件与有限 variants 存在 | 部分完成；不能以少数现有测试代替每控件全状态、触屏/反向动画/长文案/AA 验收 | R45 |
| 05 Surface/Region目录 | ui/layout、surface、navigation 等共享积木存在 | 实现可保留；Inspector 内滚动与 WorkbenchPage 关系、全页唯一 scroll owner 仍需真实布局验证 | R45 |
| 06 UI Lab/Playwright | 开发 fixture 和 visual/behavior spec 存在 | 基础设施完成；覆盖量不等于完整状态组合，真实 store 场景不足不应标全验收 | R45、R46 |
| 07 Chat | ChatTranscript、审批/fallback 控件、dock 与历史加载接线存在 | 部分完成；非正文终态合并回归、Markdown 实体错误、执行图删除未迁移 | R40、R41、R42 |
| 08 Composer/Model | 真实 provider route、跨 engine 搜索、不可用原因、权限选择与 workspace focus | 主实现保留；真实引擎/原生目录选择/窄屏完整键盘路径仍有验收缺口 | R45 |
| 09 Workbench | 多 tab 开关、宽度调整、Agent/Tasks 真 team DTO、Inspector 真 IPC | 未完成真实工具合同；Browser/Terminal/Text 伪连接，Git/Files/Review/Usage 混淆证据语义；tab 顺序及布局只在 React state，无会话持久化 | 本文 HUI-01/02/03；R45 |
| 10 Automation/Extension | 共享页面、模板预览与不可用说明 | 正确降为不可用；没有完成原卡“真实创建/安装/启停”要求。以 R20/R22 的收窄范围接受，不宣告功能完成 | R46；未来产品卡 |
| 11 Settings/Search/Menu | provider 保存、语言切换、会话搜索、打开/复制菜单与文件选择 | 部分完成；profile 假保存；全局搜索仅 sessions；无独立 ProjectMenu/危险项目动作合同；许多设置已诚实禁用 | R43、R44；HUI-04；R46 |
| 12 总验收 | 旧自动测试和 ad-hoc 打包 smoke 记录可保留 | 未完成；67/67 current、真实全业务、VoiceOver/触屏/完整矩阵不能由现有摘要推得 | R45、R46 |

## round3 UI01–UI10 / R20–R23 逐卡矩阵

这些卡的源文件仍标 `ready-for-agent`、验收框未勾，但有执行记录；旧 TASKS 的 done 不足以单独决定完成。以下以当前源码为准。

| 卡 | 当前结论 | 证据与后续 |
|---|---|---|
| UI01 信息架构 | 部分完成 | chat-first 组件拆分落地；执行概览未迁至 Inspector，旧 AGENTS 仍写主聊天三泳道；R42/R46 |
| UI02 tokens | 主改造保留 | 语义字体/表面 tokens 存在；新增聊天数值归并 R47、全矩阵 R45 |
| UI03 密度/状态 | 实现保留，未全验收 | ui primitive default/compact 已有，不能把普通 tests green 当全状态矩阵；R45 |
| UI04 阅读流 | 未完成 | ChatTranscript 已实现但双转义、非消息终态合并、代码复制/阅读位置证据不足；R40/R41/R42 |
| UI05 Inspector | 主实现保留，部分验收 | hooks 默认折叠、LLM/Tool 详情、友好身份/未知 token 已有；真实滚动恢复未复现证明；R45 |
| UI06 详情导航 | 主实现保留，部分验收 | Product Attempt 选择 native session、path/back、位置和 focus 记忆存在；保存 scrollTop 在新 DOM commit 后，真实 clamp 风险仅列限制；R45 |
| UI07 Composer | 主实现保留 | R34 已补跨 engine 搜索与 workspace focus；真实目录对话框、发送/停止矩阵未完成；R45 |
| UI08 导航身份 | 主实现保留，部分完成 | sidebar 友好项目名称/状态/更新时间，无结果与错误可区分；没有显式 loading 面；完整键盘菜单尚未验收；R45 |
| UI09 次级页面 | 部分完成 | 假套餐已清除、MCP/SSH 样例移除、typed 文案落实；profile 假保存与工具辅助页假状态仍在；R44、HUI-01/02 |
| UI10 响应式/可达性 | 未完成全卡 | 四宽度 demo 与部分键盘已补；真实审批、overlay 焦点合同、触屏/VoiceOver 尚未闭合；R45 |
| R20 自动化真实 | 收窄后的主修复可接受 | 创建/启用禁用、模板只预览；不是原 UI10 的真实 scheduler 功能完成；R46 纠正完成口径 |
| R21 权限设置真实 | 部分完成 | 假开关已禁用；“每个 engine 内建敏感路径过滤”仍无真实能力依据；R43 |
| R22 插件真实 | 收窄后的主修复可接受 | 安装禁用、无假 installed、注册不伪成功；不等于已完成真实安装；R46 |
| R23 视觉基线 | 基线修复保留，非全产品验收 | R36 详情截图目标纠正；本轮主代理已核对 visual；R45 保留真实产品限制 |

## 新增历史残留发现

### HUI-01 · P2 · Browser、Terminal 和文本预览冒充已接通

源码：`crates/alva-hub-desktop/web/src/components/workbench-tools.tsx`。

- **Terminal** `:353–356`：任意非空命令只追加 `$ command` 和 `t("tool.commandQueued")`，再清空输入，没有 bridge/队列/进程调用。`i18n.tsx:1184` 明说“命令已加入本机终端适配器队列”。真实生产入口 `App.tsx:1635` 无 demo 限制。可见 Run 正常可点，用户无法知道没有执行。应该接真实通道，或禁用并准确说明未支持。
- **Browser** `:337–344`：Open/Enter 只 `setLoadedUrl(url)`，页面只有 EmptyState，没有 WebView/iframe/navigation bridge；仍说“Chrome WebView 已就绪”和会在标签页打开（`i18n.tsx:1179/1181`）。生产入口 `App.tsx:1632` 无 demo 限制。
- **Text** `:53–65`：选择 path 后 `<pre>` 只输出“文件预览已连接到本机工作区适配器”加 path（`i18n.tsx:1107`），未读取文件。生产入口 `App.tsx:1633`。

三者均是源码可确定的假状态，不是仅缺截图。违背旧 09 “fixture 通过后连接真实工具状态”和“fixture 与真实工具生命周期均通过”。不需运行实际 shell 或外部浏览器才能确认；本次没有真的提交命令。

### HUI-02 · P2 · 工具执行记录被当作文件/Git修改和成功Review

`FilesTool` `:35` 与 `changedFiles` `:48` 把**所有** side_effect.path 当 changed files；`GitTool` `:70–81` 把每条 side_effect 计为 Git change，`succeeded !== false` 就显示 Modified。没有 git status/diff，也没有判断 tool 是否写入。真实 `run.rs:899–933` 在所有 ToolStart 写 SideEffectEvidence，读取文件的工具同样进入该列表。只读 read_file 得到“文件已修改”；无本会话工具记录但项目已有脏改动得到 Clean，均不符合 Git 状态。

`ReviewTool:99` 对 `succeeded: null`（运行中）也给 CheckCircle2，不区分待完成与成功。建议准确称为“工具记录/涉及路径”，或接真实文件修改与 Git diff；对 pending 使用中立状态。旧 09 的真实 Files/Review 合同未完成，且不能用“来自真实 DTO”替代语义正确。

### HUI-03 · P2 · 未报告费用被显示为已报告零成本

`UsageTool:324` 使用 `entry.total_cost_usd ?? 0`，`:331` 始终显示 `$0.00` 和“已报告费用”。空 session、所有 provider 未上报成本、或部分 attempt 成本未知时都没有未知/部分统计提示。真实 token/duration 累加保留，但 unknown cost 不等于 free。应区分已知零、未知、部分统计；这是旧 09 的真实用量面与 UI05“未知值不显示假的0”的同类缺口。

### HUI-04 · P2 · 全局搜索合同未落实

`product-pages.tsx:645–648` 的 SearchPage 仅接受/过滤 sessions；`:660` query 非空后只有 session results，原 settings/Inspector/Usage quick actions 全部消失。`i18n.tsx:996` 却承诺搜索“会话、项目、设置与命令”。例如输入“设置”不会搜到设置入口（除非恰巧是会话标题），空结果只留空白列表。应真实索引支持的域并补 empty/loading/error，或收窄产品文案和原卡范围。

菜单现状：`hub-sidebar.tsx:155–160` 只有 Open/Copy ID/Find related；真实打开与复制可保留。没有对应旧 11 的独立 ProjectMenu 和危险项目动作；这属于未实现需求，不是存在了未经确认的删除。不要为了过卡凭空增加危险操作，先明确有效产品范围。标准 Shift+F10 无 fallback 的问题有历史动态报告，本轮没有重新动态复现，不能作为本轮新动态证据。

### HUI-05 · P2 · 已知 Workbench overlay 键盘合同仍未闭合

历史 release preflight UI-PF-01 动态发现打开焦点丢失、背景可 Tab、Esc 不关闭。当前 `ui/layout.tsx:58–75` 只提供 scrim 和关闭态 Workbench inert，没有打开态 focus transfer/background inert/keydown handler；`App.tsx:590` 的 Escape 只服务 narrowSidebarOpen。R35 新增了工作台**关闭按钮**的焦点恢复，没有补完整 overlay 合同。

本轮静态确认实现缺口，未重演旧截图场景。应沿用 R45 加入真实浏览器键盘验收：打开进入工作台，Tab 不落背景，Esc 一层关闭并恢复触发点，scrim 路径等效。`HubTimeline` handoff 按钮仍没有 aria-expanded/aria-controls（历史 UI-PF-04）；同样映射 R45，不重复建卡。

## Workbench 全工具核对表

| 工具 | 真实数据/动作 | 判定 |
|---|---|---|
| Files | typed side_effect.path 列表 | 无 workspace 文件浏览；read path 被叫 changed files，HUI-02 |
| Text | 同 path 列表 + 本地 selectedFile | 没有文本读取，HUI-01 |
| Git | 所有 side_effect | 没有 Git 状态/差异，HUI-02 |
| Review | side_effect + duration | 不是 code diff，pending 假成功，HUI-02 |
| Agent | TeamRun workers/tasks，Focus 调 openSessionById | 真实团队关系及打开动作可保留；不等于所有 native spawned children，后者在 Inspector |
| Inspector | getHubInspection IPC + typed records | 真实下钻主实现保留；R42/R45 处理概览定位及验收 |
| Tasks | TeamRun.tasks | 真实只读队列状态；编辑/取消在 TeamPanel，不伪装此页已调度 |
| Usage | 持久 attempt usage | token/duration有来源，null cost处理错误，HUI-03 |
| Browser | 本地 url state | 假 ready/open，HUI-01 |
| Terminal | 本地字符串数组 | 假 queued，无执行，HUI-01 |
| Engines/Fallback/Team | bridge与真产品状态 | 不在本分工重复审 Rust 实现；本轮保留已审动作和明确验收缺口 |

Tab 生命周期：App.tsx:604–605 的 open/active tabs 仅在组件 state；没有排序 UI、重启恢复或按会话保存。切 tab 用 forceMount 保留本地内容，关闭整个工作台不卸载内容可保留，但由于终端/浏览器根本不存在 native lifecycle，不能拿此证明旧卡“关闭工作台不停止终端/浏览器”真实验收。

## Release-readiness 00–08 逐卡矩阵

本批是待执行规划，不把未开工写成生产实现缺陷。`git log 07b75f8..5c64af1 -- .scratch/alva-hub-v1-release-readiness crates/alva-hub-desktop/ACCEPTANCE.md` 无本波提交；目录只有规划、visual-plan、旧 ui-preflight 报告与四张截图，没有这波新增逐 gate 执行证据。本结论是未见新增实现/验收证据，不声称整个仓库不存在其他未归档发布工作。

| 卡 | 账本状态 | 当前判定 |
|---|---|---|
| 00 RC基线/范围 | 待开始 | 未有本波冻结的 RC artifact/lock/toolchain/engine release范围清单；应保持 pending |
| 01 packaged scenario runner | 待开始 | 未有本批 deterministic packaged runner/证据信封交付；已有普通测试不等价 |
| 02 真实Engine GUI矩阵 | 待开始 | 无本批 fresh/restart/resume 全发布engine GUI矩阵；已有CLI/个别打包观察不可替代 |
| 03 continuity/fallback | 待开始 | 未有六触发三层证据及最终包矩阵；不因后台单测通过就关闭 |
| 04 crash/repair/migration soak | 待开始 | 保留已有 durability实现，不等于完成本批 fault/soak/recovery 审计 |
| 05 macOS分发 | 待开始 | 旧 ad-hoc app不等于Developer ID/notarization/冷安装升级；外部门槛不冒充通过 |
| 06 安全隐私依赖 | 待开始 | 未见本批诊断导出/secret scan/依赖/权限报告；R43也说明UI安全声明需纠正 |
| 07 packaged UX | 预检完成，待修复与正式包 | 旧报告明确G6未通过；HUI-05等仍需修复/复验；VoiceOver/最终签名包未验证 |
| 08 RC发布关闭 | 待开始 | 依赖00–07，不能宣布accepted/released；未做发布动作 |

## 证据边界

- 本文是历史卡逐项源码/完成口径审计，没有新的动态 UI 执行日志；确定性数据流缺陷与待验证风险已分开。
- Markdown/mergeLiveTimeline 的本轮动态探针、视觉测试结果由第五轮主报告统一记录，本文不重复虚构执行。
- 不把旧报告的“通过63/70测试”等当作当前重新运行结果。
- 不建议重复重建 tokens、模型搜索、Inspector typed IPC；返工围绕上述局部语义/接线与未完成验收。
