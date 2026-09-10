# R21: 权限与执行设置必须作用于真实策略

**Priority:** P1
**Axis:** Hub 实测 / 用户控制失效
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** 无
**Status:** ready-for-agent

## 问题与证据

原生 Hub 设置→权限与安全，选择“只读计划 (Plan Only)”；新建会话仍是“允许写入工作区”。截图/tree 见 native-plan-selected 和 native-plan-new-session。
product-pages.tsx:408 的 permissionMode 仅组件 useState，:889–890 只调用 setPermissionMode，未传给 App 的 PermissionProfile 或持久化默认策略。同页静态显示“严格保护中”，通用设置还以局部状态显示 Worker 并发/资源档位/Git 自动保存点，并没有实际配置接线。
用户可能依赖这些控件保护文件或限制执行，实际运行并未受它们控制。只读计划与当前 Hub PermissionProfile 语义不同，不应简单将一个标签机械映射到另一种更宽权限。

## 实现范围

将安全/执行设置接到唯一的后端策略来源；不支持的选项必须禁用或移出生产面。说明默认设置仅影响新会话，已存在会话保持其稳定权限契约；不虚构不存在的 Plan profile 或默认弱化权限。顺带核对 Worker/Git 保护开关是否具有真实能力，未接线的明确不可用。

## 验收

- [ ] 设置页显示的实际权限与新会话的提交参数/后端保存值一致。
- [ ] 选择只读策略后真实工具写入被限制；如果不能实现等价 Plan，则不提供可成功选择的假选项。
- [ ] 切页/重启不会丢失已成功保存的设置；既有会话不会被静默改权。
- [ ] 静态“保护中”徽标由真实能力状态决定；Worker/Git 控件同样不假成功。
- [ ] 在真实 command/受控引擎测试中验证行为，不用同组件 state 回读代替。

## 执行记录

- Commit：待填写
- 实际验证命令及结果：待填写

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
