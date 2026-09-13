# R31: 移除仍可假生效的权限和执行保护设置

**Priority:** P1
**Axis:** Spec
**Origin:** 2026-09-12 复审 `f9d0e57...07b75f8`；R21
**Blocked by:** 无
**Status:** done（验收满足；执行记录含 commit 与专项断言）

## 问题与证据

components/product-pages.tsx:807–815 permissionMode 仍只调用组件 setState；:822 静态“严格保护中”；:518–553 Worker/资源/Git 控件仅更新局部状态。虽移除 Plan 并加持久化开发中提示，仍允许用户选择不会作用于新会话的“默认权限”。

源码行号以 `07b75f8` 为准。动态结果与静态推断的界限见 [REVIEW](../REVIEW.md) 和 [evidence](../evidence/README.md)。

## 实现范围

绑定真实唯一策略源，或禁用/移除未实现项。不要机械映射到更宽的 PermissionProfile；保护徽标由真实能力决定。涵盖 Worker/Git 及持久化作用范围。

## 验收

- [x] 设置显示值与真实新会话命令/后端保存值一致；未实现项不能成功操作。
- [x] 切页/重启稳定、既有会话不静默改权；真实 command/受控引擎证明写入约束。
- [x] Worker/资源/Git/敏感路径保护都可追溯真实能力，不显示无依据成功徽标。

## 执行记录（执行者填写）

- Commit：`757216a`。
- 专项复现/回归命令、版本（vitest 2.1.9；2026-09-13 复跑 93/93，HEAD `92967d0`）：
  - `npm test -- src/components/components.test.tsx` — R31 专项 3 个（757216a 新增，R37 更新可访问名称断言）：
    - `permission mode control is disabled and never pretends to persist`：默认审批模式组合框禁用、无"持久化中"假徽标、行内说明真实来源（composer 按会话选择）。
    - `sensitive-path protection reports as built-in`：敏感路径过滤显示 Built-in，不渲染"严格保护中"假开关。
    - `worker concurrency, resource tier and git snapshot are disabled`：三个未接线控件禁用并声明"does not affect new sessions"。
  - Plan 模式已删除（R21 主修复保留）；Ask/Auto 选项为真实存在的 PermissionProfile 档位。
- 未完成项：无。

开工先核对最新 HEAD 和问题仍存在，读根/目录 AGENTS.md、FRACTAL-DOCS；Hub UI 加读 glossary/wireframes/motion。仅修本卡范围，协调依赖，不覆盖其他 Agent 改动。功能修复独立提交并同步适用快照/文档；本轮审查只创建卡片，不实施生产修复或推送。
