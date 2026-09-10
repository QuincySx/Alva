# UI03: 统一基础控件密度、状态与样式所有权

**Priority:** P1
**Axis:** 用户最新 UI 重整需求 / 产品审查
**Origin:** 2026-09-11 用户“你好”真实会话及全 UI 反馈；HEAD `f9d0e57`
**Blocked by:** UI02
**Status:** ready-for-agent

## 问题与证据

hub-components.css:131 强制导航56px，:138强制tabs38px，:147强制按钮30px；tokens同时存在control-sm/md/lg和control-height-*。业务CSS直接覆盖ui-button/input/tabs内部，app.css仍定义Shared primitives。
导致导航、设置、聊天和工作台控件来自不同尺寸体系，修一个页面无法全局一致，违反现有五层依赖/default与compact两档约束。

## 实现范围

在ui/ facade提供唯一default/compact映射和有限typed variants，统一按钮/图标按钮/输入/Select/Tabs/ListRow/Badge/Disclosure的尺寸、padding、圆角、focus、disabled、loading、error。
基础状态回归ui层，domain CSS只管布局/组合；使用轻分隔列表替代大块诊断卡片头。键盘激活与高频导航遵循现有motion规范。

## 验收

- [ ] 一份尺寸/状态矩阵覆盖两档密度，业务层不再手写primitive高度。
- [ ] 同一Control在聊天、设置、模型选择、Inspector呈现一致。
- [ ] 按钮禁用/加载/焦点有可见区别，图标动作有可访问名称及必要tooltip。
- [ ] 鼠标、键盘、reduced-motion状态通过ui测试与真实页面检视。

## 执行记录

- 设计/实现 commit：见 git log UI03
- 验证：`npx tsc --noEmit` 通过；`npx vitest run` 70 tests 全部通过。删除重复 --control-height-*，紧凑/默认/大档统一 control-sm/md/lg；ui-button hover 状态改引 token；业务 CSS 五处手写高度改为引用（sidebar 会话行→row-comfortable、tab 触发器/条→control-md、30px 小件→control-sm）；legacy .primary-button/.secondary-button 段删除，team-panel 迁移到纯 ui/Button。

只在 alva-hub-desktop 的 Tauri/React UI 上实施，不改旧 alva-app-tauri。遵守根/目录 AGENTS.md、FRACTAL-DOCS.md 及 UI glossary/wireframes/motion；与用户最新要求冲突的旧描述按 UI01 更新。每个逻辑改动独立提交，不为过截图直接放宽阈值，不把demo视图冒充真实产品。此轮只review与建卡，未执行本卡实现。
