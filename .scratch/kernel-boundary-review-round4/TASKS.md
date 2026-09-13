# 第四轮：当前返工执行清单

2026-09-12，审查 HEAD `07b75f8`，对比昨天实现前 `f9d0e57`。
第三轮20张卡均被原执行者标done，**复审不接受全部完成**。本表是新的唯一返工入口；不要整批重做历史卡。

> **2026-09-13 执行完毕**：R24–R27、R29–R37 全部由本轮执行者完成并记录（commit `bd8c5ea`→`30dbad1`，本地分支 `refactor/kernel-boundary`，未推送）。
> R24–R34、R37 验收满足；R35/R36 主体完成、显式边界记录在各卡执行记录。
> R28 于 `同步最终职责文档` 提交收口。逐卡 commit 与专项断言见各卡执行记录。

| 卡 | 优先级 | 标题 | 依赖 | 状态 |
|---|---|---|---|---|
| [R24](./issues/R24-restore-tool-session-capability.md) | P1 | 恢复真实 ToolBatch 的会话能力接线并联验递归持久化 | R27 | done |
| [R25](./issues/R25-complete-broadcast-driver.md) | P1 | 完成广播 driver 的唤醒、取消隔离和回收 | 无 | done |
| [R26](./issues/R26-reset-broadcast-generation.md) | P2 | 隔离 clear 与在途广播的序号代际 | R25 | done |
| [R27](./issues/R27-finish-kernel-storage-boundary.md) | P2 | 真正收口内核与具体会话存储的边界 | 无 | done |
| [R28](./issues/R28-sync-final-docs-and-closure.md) | P3 | 同步最终职责文档并修正卡片完成口径 | R24、R25、R26、R27 | done |
| [R29](./issues/R29-keep-chat-after-terminal-event.md) | P1 | 终态事件到达后保留用户问题与回答 | 无 | done |
| [R30](./issues/R30-finish-chat-reading-ui.md) | P1 | 完成聊天阅读流、Markdown 与执行图迁移 | R29 | done |
| [R31](./issues/R31-truthful-security-controls.md) | P1 | 移除仍可假生效的权限和执行保护设置 | 无 | done |
| [R32](./issues/R32-remove-production-sample-states.md) | P2 | 清理生产设置与扩展页的样例状态 | 无 | done |
| [R33](./issues/R33-unify-inspector-navigation.md) | P2 | 完成 Inspector 选择联动与逐层返回体验 | R30 | done |
| [R34](./issues/R34-finish-composer-and-session-navigation.md) | P2 | 补齐模型搜索与会话导航剩余交互 | 无 | done |
| [R35](./issues/R35-complete-responsive-acceptance.md) | P2 | 完成四档布局、键盘和真实产品验收 | R29、R30、R31、R32、R33、R34 | closed（显式边界见执行记录） |
| [R36](./issues/R36-restore-detail-visual-baseline.md) | P3 | 恢复 LLM 详情截图并按真实状态重审基线 | R30、R33、R35 | closed（显式边界见执行记录） |
| [R37](./issues/R37-typed-ui-copy.md) | P3 | 将新增产品文案归入 typed i18n | R30、R31、R32、R33、R34 | done |

## 调度与关闭

- 核心边界：R27 → R24；广播：R25 → R26；随后R28核对最终文档。R24和R27应由协调的执行者承接，不能用具体cell双注册临时绕过边界。
- 产品优先：R29聊天终态、R31权限假控制；R30消息阅读流与R33 Inspector共同确定数据/导航分工。
- R32设置、R34导航分别推进；R37跟随最终文案收口；R35做行为/真实产品矩阵，R36最后重新审阅视觉基线。
- 卡片是有限返工范围，允许诚实禁用未实现能力；不要求借返工新增调度、SSH等完整产品。
- 每卡记录对应commit和专项断言。通用cargo/vitest通过、截图批量更新、执行记录写了done均不能替代卡内验收。

交接给执行Agent：先读本目录README/REVIEW与各卡，核对最新HEAD；只处理本轮待办，历史R14–R23/UI01–UI10用于追溯。不得自动重做已完成部分，不推送/部署。
