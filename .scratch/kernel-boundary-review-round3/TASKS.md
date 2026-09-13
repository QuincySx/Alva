# 第三轮 Review：当前唯一执行清单

> **2026-09-12 复审：下表 done 为原执行者声明，不代表验收全部通过。当前返工入口已转到 [第四轮 TASKS](../kernel-boundary-review-round4/TASKS.md)，逐卡结论见 [第四轮 REVIEW](../kernel-boundary-review-round4/REVIEW.md)。请勿继续按旧表关闭或重复执行。**

> 2026-09-11；仅执行 **R14–R23 + UI01–UI10，共 20 张新卡**。旧 01–19、R01–R08、R09–R13 仅作历史来源，未完成行为转入本轮。
> 复审 diff：`134b8b3...f9d0e57`，并实测当前 Hub 前端/桌面流程。当前 HEAD 改变后先复核，不重复修已解决问题。

| 卡 | 优先级 | 标题 | Blocked by | 状态 |
|---|---|---|---|---|
| [R14](./issues/R14-recursive-session-persistence.md) | P1 | 递归子会话必须保存到正确的父仓库 | R18 | done |
| [R15](./issues/R15-atomic-session-switch.md) | P1 | 将日志与当前存储能力作为一次原子切换 | R18 | done |
| [R16](./issues/R16-isolate-broadcast-delivery.md) | P2 | 隔离慢监听者并保证追加取消后的实时投递 | 无 | done |
| [R17](./issues/R17-reset-safe-subscription-cursors.md) | P2 | 清空会话后实时订阅不得跳过新事件 | R16 | done |
| [R18](./issues/R18-restore-kernel-storage-boundary.md) | P2 | 将具体会话存储接线移回装配层 | 无 | done |
| [R19](./issues/R19-sync-new-session-interface-docs.md) | P3 | 补齐新增会话接口及职责的分形文档 | R14–R18 | done |
| [R20](./issues/R20-truthful-automation-state.md) | P1 | 自动化不能假成功或切页即丢失 | 无 | done |
| [R21](./issues/R21-truthful-permission-settings.md) | P1 | 权限与执行设置必须作用于真实策略 | 无 | done |
| [R22](./issues/R22-truthful-extension-catalog.md) | P2 | 插件安装状态与注册操作必须来自真实数据 | 无 | done |
| [R23](./issues/R23-review-visual-baselines.md) | P3 | 核对并修正 Hub 视觉基线漂移 | UI04–UI10 | done |

## UI 重整卡（用户本轮新增要求）

主聊天以阅读/操作为主；执行时间线移入Inspector；Middleware按需下钻。以 `evidence/native/user-hello.png` 为真实问题样本。

| 卡 | 优先级 | 标题 | Blocked by | 状态 |
|---|---|---|---|---|
| [UI01](./issues/UI01-agent-ui-information-architecture.md) | P1 | 确定聊天优先的信息架构与视觉方向 | 无 | done |
| [UI02](./issues/UI02-semantic-design-tokens.md) | P1 | 重建字体、颜色、间距与表面层级 tokens | UI01 | done |
| [UI03](./issues/UI03-consistent-control-density.md) | P1 | 统一基础控件密度、状态与样式所有权 | UI02 | done |
| [UI04](./issues/UI04-readable-chat-stream.md) | P1 | 重做主聊天阅读流与 Markdown 呈现 | UI01、UI02、UI03 | done |
| [UI05](./issues/UI05-focused-inspector.md) | P1 | 重做 Inspector 首屏重点与分层下钻 | UI01、UI02、UI03 | done |
| [UI06](./issues/UI06-workbench-detail-navigation.md) | P2 | 合并重复详情并统一工作台导航与滚动 | UI01、UI03、UI05 | done |
| [UI07](./issues/UI07-compact-composer-onboarding.md) | P2 | 收紧输入区并梳理新会话、模型与上下文操作 | UI01、UI02、UI03 | done |
| [UI08](./issues/UI08-readable-navigation-identity.md) | P2 | 整理 Sidebar、顶栏与会话身份展示 | UI01、UI02、UI03 | done |
| [UI09](./issues/UI09-secondary-product-page-consistency.md) | P2 | 统一设置、自动化、插件等次级页面层级 | UI01、UI02、UI03、R20、R21、R22 | done |
| [UI10](./issues/UI10-responsive-accessible-ui.md) | P2 | 落实响应式、键盘与状态矩阵 | UI03、UI04、UI05、UI06、UI07、UI08、UI09 | done |

## 执行建议

- 会话能力链：R18 → R14 → R15，同一执行者协调所有权，保持每个逻辑问题独立提交。
- 广播链：R16 → R17；不能继续以历史回放测试替代实时投递/取消测试。
- Hub 产品：优先 R21 权限控制、R20 自动化真伪；然后 R22 插件状态和 R23 视觉基线。
- R19 最后核对核心文档，但每张卡仍需即时维护自身分形文档。

ready-for-agent 不豁免依赖。交付必须记录 commit 和真实验证，不得把“有修复提交”当作“验收完成”。

## UI 执行顺序与交接

先 UI01 明确信息归属与四种代表状态，再 UI02/UI03 建立共同tokens和controls；UI04/UI05优先落地主聊天与Inspector。之后 UI06–UI09完善区域/高频操作/导航/次级页，UI10完成响应式和键盘；最后R23审核重建视觉基线。
功能真假问题R20–R22优先处理，UI09只负责其最终界面一致性，不能靠视觉重做掩盖未接线控件。

> 只执行 `.scratch/kernel-boundary-review-round3/TASKS.md`。本轮包含10张运行/产品修复卡和10张UI重整卡，旧三组卡均不再开工。先读README、REVIEW与evidence，核对当前HEAD和最新用户UI要求；逐卡实现/验证/独立提交并登记。范围不包含推送、部署。
