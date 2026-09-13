# 2026-09-12 第四轮整体 Review

**结论：第三轮20张卡在总表全部标done，但没有全部达到原验收。已建立14张返工卡R24–R37。**

## 范围与判定方式

固定基线 `f9d0e57`，审查 `git diff f9d0e57...07b75f8`。9月11日16个实现/文档提交及当天完成记录、9月12日凌晨补充记录均核对到当前HEAD。按当前README的唯一清单审查R14–R23/UI01–UI10；旧01–19/R01–R13作为已归档来源，不宣称本次重新完成全历史验收。Standards与Spec分别审查，不将“符合风格”当作“需求完成”。

新增探针通过公开接口/真实React App运行；UI bridge受控mock，不宣称真实Tauri端到端。未改生产实现。证据与复跑步骤见[evidence](./evidence/README.md)。

## Standards

1. **P1，装配键迁移只改写端。** `agent_builder.rs:370`、`run_child.rs:110` 注册 `Arc<dyn PersistentSessions>`，`tool_batch.rs:650` 仍读 `StoreSessionCell`。违反已声明的能力接线契约，真实工具得不到store。新装配探针失败。返工[R24](./issues/R24-restore-tool-session-capability.md)。
2. **P2，存储边界未兑现。** `run_child.rs:75–89` 的 CellSessionsBridge仍在kernel包装具体cell，`runtime_context.rs:156–160` 和ABI桥继续暴露它。违反CONTEXT.md:16–19、87–92及R18明确验收。返工[R27](./issues/R27-finish-kernel-storage-boundary.md)。
3. **P3，文档未级联。** kernel-abi目录仍指向迁出的实现，新增接口未进入地图，agent-core投递/能力描述与真实代码相反。违反FRACTAL-DOCS接口/职责变更规则。返工[R28](./issues/R28-sync-final-docs-and-closure.md)。
4. **P3，新增文案绕过typed i18n。** product-pages.tsx多个新增locale条件分支违反web/src/AGENTS.md领域组件规则。返工[R37](./issues/R37-typed-ui-copy.md)。

判断性建议（不计硬违规）：`agent_session.rs:559–596`和`:643–675`复制两套投递逻辑，已经演化出不同追加语义；随R25统一，不单独建风格卡。

Standards：4项硬问题、1项合并建议；本轴最高风险为装配键断线。

## Spec

1. **R14/R15：真实链路和验收仍有缺口。** R14修好了dyn方法和child自己的cell，但真实ToolBatch取不到能力，见R24；R15的state锁修改合理，flush失败用例存在，仍缺卡要求的确定性swap/turn及递归持久化联验。不能因泛化655测试通过就关闭专项验收。
2. **R16：广播隔离未完成且新增空闲停投。** append/append_message仍内联await；取消追加者丢本地batch；typed append不给Notify发信号。三个专项探针均失败。见[R25](./issues/R25-complete-broadcast-driver.md)。
3. **R17：简单clear修好，在途交错仍漏。** clear重置cursor后旧batch继续advance，跳过新seq1，专项gate探针失败。见[R26](./issues/R26-reset-broadcast-generation.md)。
4. **UI01/UI04：live终态隐藏聊天正文。** App.tsx:1728–1747隐藏整个HubTimeline而非只移执行图；completed事件受控测试失败。**完成历史重新打开的控制测试通过**：其末项通常投影assistant_message，不在隐藏名单；因此不把问题描述成所有历史都丢。见[R29](./issues/R29-keep-chat-after-terminal-event.md)。
5. **UI04：阅读流主要要求没实现。** hub-timeline.tsx:170仍有执行泳道，:197–213是纯字符串p，没有Markdown/代码复制/阅读位置闭环。见[R30](./issues/R30-finish-chat-reading-ui.md)。
6. **R21：权限与执行保护仍可假操作。** product-pages.tsx:807–815局部permissionMode、:822静态保护徽标、:518–553局部Worker/Git控制。删除Plan和加开发中提示没有满足“不支持项禁用”。见[R31](./issues/R31-truthful-security-controls.md)。
7. **R22/UI02/UI09：生产假状态仍残留。** 外观设置局部状态、用量/支出/版本/更新结论硬编码，MCP/SSH样例仍在生产并可局部增删。见[R32](./issues/R32-remove-production-sample-states.md)。
8. **UI05/UI06：详情联动与逐层返回只做了一部分。** 原生Inspector与产品尝试仍叠放，没有选择联动及逐层滚动/焦点恢复，执行记录也承认未完成。见[R33](./issues/R33-unify-inspector-navigation.md)。
9. **UI07/UI08：高频导航遗留未转卡。** 紧凑输入和友好标签已经落实，但模型搜索、引导收紧、Sidebar状态矩阵未完成。见[R34](./issues/R34-finish-composer-and-session-navigation.md)。
10. **UI03/UI10/R23：验收不足且详情截图覆盖退化。** 现有测试只两档demo viewport；四档、键盘、长文案/审批/真实产品缺证据。visual.spec.ts:44–49回到根后才拍LLM详情截图。见[R35](./issues/R35-complete-responsive-acceptance.md)、[R36](./issues/R36-restore-detail-visual-baseline.md)。

Spec：10组需求/行为问题，按不同修复责任拆卡；本轴P1包含真实会话接线、广播投递、聊天与权限行为。没有将静态未验收项冒充已动态复现的故障。

## 20张卡完成审计

“已有实现”保留成果；“未验收”不等于已证明代码错误；二者都不自动等同全卡完成。第三轮单卡Status仍ready-for-agent、验收框均未勾，与总表done矛盾，已在旧入口加复审转向说明。

| 原卡 | 本轮结论 | 保留成果 / 缺口 | 后续 |
|---|---|---|---|
| R14 | 未完成 | dyn方法和child身份修复保留，真实工具能力断线 | R24 |
| R15 | 实现方向成立，专项未验收 | state锁内flush/切换保留，缺确定性交错/真实链联验 | R24验收项 |
| R16 | 未完成 | driver骨架存在，阻塞/取消/唤醒探针失败 | R25 |
| R17 | 部分完成 | 简单clear新订阅通过，在途交错失败 | R26 |
| R18 | 未完成 | trait包装未解除具体cell依赖，读写键不一致 | R27、R24 |
| R19 | 未完成 | 目录地图与新职责仍不一致 | R28 |
| R20 | 主修复可认可（静态） | 自动化创建/启用已禁用，模板预览；无新增故障证据 | R35共享产品验收 |
| R21 | 未完成 | Plan删除保留，Ask/Auto/Worker/Git仍局部假控制 | R31 |
| R22 | 部分完成 | 安装禁用保留，生产样例隔离未完成 | R32 |
| R23 | 部分完成 | Back断言执行顺序改善，LLM截图目标却错误；矩阵缺失 | R35、R36 |
| UI01 | 未完成 | 方向声明chat-first，真实消息/执行图未分离 | R29、R30 |
| UI02 | 部分完成 | 语义tokens/字体改善保留，外观假选择与状态验收未收口 | R32、R35 |
| UI03 | 已有尺寸改造，矩阵待验 | 统一control尺寸保留，本次未证实需重写；两密度/状态验收未齐 | R35 |
| UI04 | 未完成 | 正文尺寸改善保留，live终态回归、Markdown等缺失 | R29、R30 |
| UI05 | 部分完成 | Middleware折叠、Hook子页保留；导航恢复未完成 | R33 |
| UI06 | 部分完成 | 即时详情rail/默认折叠保留；跨层联动未完成 | R33 |
| UI07 | 部分完成 | 紧凑composer/友好路径保留；模型搜索/引导未完 | R34 |
| UI08 | 部分完成 | UUID常驻暴露减少；同名区分/搜索状态矩阵未收口 | R34 |
| UI09 | 部分完成 | 假身份/emoji清理保留；假数据与文案归属未收口 | R32、R37 |
| UI10 | 部分完成 | Sidebar sheet/scrim/Esc已加；四档/焦点/真实验收未做全 | R35 |

## 验证结论

- 核心新增7个探针：**2 passed / 5 failed**；分别验证dyn方法、简单clear，以及键断线/阻塞/取消漏投/空闲不醒/clear交错。
- React App受控bridge：完成历史恢复控制 **1 passed**；追加live completed后的正文保留 **1 failed**。
- 原有前端套件本轮复跑：**6 files / 70 tests passed**。说明通用绿灯没有覆盖上述真实需求回归。
- 原有核心广播/订阅测试本轮复跑：**3 passed**。名为cancelled的旧测试仍未真正abort，不能证明取消安全。
- 本次没有重跑完整Rust workspace、最新原生Tauri/真实存储全链路或视觉矩阵；相关缺口已进入专门卡，不声称这些验收通过。

本轮只创建可执行返工卡与证据，未修复生产代码，未提交/推送。
