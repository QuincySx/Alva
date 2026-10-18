# 2026-09-15 第五轮整体 Review

**R24–R37 有实质进展，但不能整体关闭。** 首轮审查固定 `07b75f8...5c64af1` 的11个提交；用户随后明确要求所有实现，已扩展至八套清单106张卡，全部本地分支覆盖关系见 [SCOPE](SCOPE.md)。工作树原有未跟踪资料保留，未修改生产实现、提交或推送。

## 全范围结果

- [HISTORICAL-CORE.md](HISTORICAL-CORE.md)：38张内核/历史返工卡。
- [HISTORICAL-UI.md](HISTORICAL-UI.md)：36张UI/发布卡（含9张待开始或预检的release卡）。
- [HISTORICAL-SEAM.md](HISTORICAL-SEAM.md)：18张引擎契约卡。
- 本文下方：最新14张R24–R37。合计106，不以归档或done代替验收。

统一返工清单 **R38–R61，共24张open卡**。这是任务拆分数，不等于24个动态复现bug：其中包含确认缺陷、局部未实现和明确待验收项。保留全部有效成果，不重做整个系统。

**全范围 Standards：** 本文3组文档/token问题之外，原公开表面快照漏async函数/字段/enum等（R56）；历史完成口径不实统一R46。扫描器动态对照确认漏检，不冒称CI完整通过。

**全范围 Spec：** 最新7组实现问题、2组验收缺口之外，历史新增Hub终态工具顺序R49、cancel能力后备R50、HTTP取消记账时序R51、RunHandle代际R58；Workbench伪工具R53、Git/Review语义R54、未知费用R55、全局搜索范围R59；审批自述接线R57、空async测试R60；适配器契约矩阵/违约处置R52和正式第五引擎入口R61为未完成验收/能力。详细触发、范围和证据见各卡。

**动态结果：** 六个独立回归探针全部失败：UI两项、广播两项、Hub两项；均成功编译/加载后断言失败。原93项前端、23项浏览器、12项广播/1项订阅仍通过；真实三级递归、确定性swap gate、最小日志各1项通过。工具批显示7/7，但其中2项漏await为空绿，只认可另5项实际场景。测试绿灯与需求闭环已明确分开。

R51/R58等静态发现未冒称动态复现；旧发布00–08保持待执行/预检，不当新增功能bug。所有本地功能分支均被HEAD包含，备份17项补丁等价。

## Standards（最新一波）

1. **P2：新广播文档描述了不存在的同步契约。** `crates/alva-agent-core/AGENTS.md:75` 称 clear/restore/rollback 都在队列锁内递增代际并重置游标；`agent_session.rs:653` restore 直接委托 inner，rollback 亦然，clear 释放 queue 后才重置游标。违反 FRACTAL-DOCS 职责同步。文档必须描述真正实现，不用文档承诺证明并发安全。见 [R46](issues/R46.md)。
2. **P2：新聊天组件未完成分形级联。** 新增 `chat-transcript.tsx` 未进入 `web/src/AGENTS.md` 清单，Timeline 仍被称作中央会话读取轴；kernel-core 地图仍描述迁出的 context_apply/tool_lock。违反新增/职责变更级联规则。见 [R46](issues/R46.md)。
3. **P3：新增聊天样式绕过 tokens。** `web/src/styles/app.css:1151` 起成组 padding/gap/radius 字面量，违反 `web/src/AGENTS.md` 第18条。只收口新增部分，不借机全仓整理。见 [R47](issues/R47.md)。

无独立高价值 Fowler smell；架构要求的 capability bridge 不因转发就判 Middle Man。

Standards：3组硬问题，最高P2（文档契约与职责地图不实）。

## Spec（最新一波）

1. **P1 / R25：非 Tokio 回调重入死锁。** `agent_session.rs:563` 获取 delivery_lock，`:582` 持锁 await listener；无 Tokio/wasm 内联 fallback 的 listener 再 append 同会话会等待自己持有的锁。违背“非 Tokio/wasm 适用路径有测试”和源码回调可读写保证。见 [R38](issues/R38.md)。
2. **P2 / R25：空闲 driver 没有弃置唤醒。** `agent_session.rs:700–713` 把 core 放掉后只 await Notify；无 Drop/关闭通知，Weak 失效不会唤醒任务，空闲会话每次弃置可残留一项停泊任务。违背“会话弃置能退出”。见 [R39](issues/R39.md)。
3. **P1 / R29：正文保留逻辑误用于所有执行事件。** `App.tsx:513` 对同 item 一律优先 previous.message。生产 `run.rs:909/953` ToolStart/ToolEnd 同ID的 running→failed 最终仍显示 running；fallback→exhausted 同 attempt 的最终摘要亦被旧文本覆盖。独立使用真实 activity_started/activity_completed 类型的探针失败。见 [R40](issues/R40.md)。
4. **P2 / R30：Markdown 双重转义。** `chat-transcript.tsx:59–74` escapeHtml 后交给 React 文本节点，`a < b && c > d "quoted"` 显示实体文字；独立渲染探针失败。现有组件测试未直接测试 ChatTranscript 的 Markdown/复制/滚动。见 [R41](issues/R41.md)。
5. **P2 / R30：执行图删除而非迁入 Inspector。** 全 src 已无 TimelineOverview；主列仍通过 `App.tsx:1783` 渲染执行事件，工具摘要无 Inspector 定位动作。没有满足“执行泳道完整迁入 Inspector”“工具摘要按需定位”。见 [R42](issues/R42.md)。
6. **P2 / R31：安全能力仍被泛化声明。** 新 `i18n.tsx:583/1263` 保证每个引擎有敏感路径过滤、不能执行必 ineligible；`engine_wiring.rs:396–402` 却允许 Claude unrestricted，明确 bypassPermissions auto-allows everything，未接 ALVA 过滤中间件。这是能力声明不实，不声称本轮实际读取过任何敏感文件。见 [R43](issues/R43.md)。
7. **P2 / R32：个人资料仍可假保存。** `product-pages.tsx:305–306` 假身份，`:372–381` 编辑直改 state，Cancel/Save 同样仅关闭；取消不回滚、离开页后保存丢失。历史残留但在本卡“所有设置分区无假保存”验收内。见 [R44](issues/R44.md)。
8. **P2 / R35/R36：明确未做的验收不能 closed。** 两卡仍有未勾验收；demo 四档/23项绿灯不等于真实 store/Tauri/engine、审批活事件、两密度全状态验收。behavior 的可见断言不证明未被 overlay 遮挡，程序化 focus/fill 也没有证明完整 Tab 路径。保留真实 LLM 详情截图修复；见 [R45](issues/R45.md)。
9. **P2 / R24：实现修复保留，组合验收仍缺。** 真 nested 测试和确定性 swap gate 已存在；前者重开 repository 查询，未经 Inspector，后者只 root turn，没有运行中 child 与 swap 联验。按“保留已开始 child 身份”“重启 Inspector 可逐级查到”补证据，而非宣布递归实现失败。见 [R48](issues/R48.md)。

Spec：7组确认实现问题 + 2组验收缺口，最高P1（非 Tokio 重入、工具终态合并）。

## 14卡完成审计

| 卡 | 本轮判定 | 保留成果 / 后续 |
|---|---|---|
| R24 | 主修复成立，组合验收未全 | TypeId统一、真实三级递归、repository重开、root swap gate保留；R48补Inspector/运行中child组合 |
| R25 | 未完成 | native异步唤醒/真实abort修复保留；R38/R39补重入与任务回收 |
| R26 | 已有专项修复，边界保留 | 在途旧batch代际隔离实现成立；并发clear/append窗口未动态确认，不计新故障 |
| R27 | 静态认可主边界 | tool path仅消费dyn PersistentSessions，具体Cell已移出；本轮不冒称重跑完整wasm检查 |
| R28 | 未完成 | 新文档仍错、完成口径过满；R46 |
| R29 | 修复主体保留，有新增回归 | 五种终态聊天保留通过；非消息状态更新R40 |
| R30 | 部分完成 | 独立阅读组件已实现；Markdown/迁移/定位/专项行为R41/R42 |
| R31 | 部分完成 | 假开关禁用保留；保护声明R43 |
| R32 | 部分完成 | MCP/SSH/用量/外观清理保留；资料假保存R44 |
| R33 | 主实现可保留，真实布局待验 | 选择联动/友好路径/多级Back存在；短详情clamp与恢复在R45实测 |
| R34 | 主实现可保留，产品验收未全 | 跨引擎搜索/焦点修复保留；真实键盘/布局R45 |
| R35 | 不接受closed | demo行为矩阵通过；缺项R45 |
| R36 | 部分完成，不接受closed | 截图前唯一详情元素断言已恢复且视觉通过；审批矩阵R45 |
| R37 | 本轮范围可认可 | typed文案迁移与测试通过；历史分支排除合理，长文案布局仍R45 |

## 验证与限制

- 本轮前端原有 Vitest：6 files / 93 tests passed。
- 本轮 Playwright：23/23 passed（含11行为、12视觉）；未更新 expected、未放宽阈值。
- 新增独立 UI 探针：2/2 failed，正文双转义与工具失败消息被旧进度覆盖，源码和输出见 evidence。
- 核心广播两个独立探针均失败；Hub两个公开API探针均失败。R24真正nested通过1/1、swap gate目标通过1/1；核心动态记录见 [evidence/README.md](evidence/README.md)，与旧卡执行者声称区分。
- 本轮未运行真实Tauri/store/外部引擎交互、完整Rust workspace或真实wasm运行。R33 DOM切换后scrollTop可能被布局clamp、clear并发窗口仅作待验证风险，不记作已复现故障。

返工入口：[TASKS.md](TASKS.md)。只处理本轮有限缺口，保留有效实现和测试。
