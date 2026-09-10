# UI08: 整理 Sidebar、顶栏与会话身份展示

**Priority:** P2
**Axis:** 用户最新 UI 重整需求 / 产品审查
**Origin:** 2026-09-11 用户“你好”真实会话及全 UI 反馈；HEAD `f9d0e57`
**Blocked by:** UI01、UI02、UI03
**Status:** ready-for-agent

## 问题与证据

真实Sidebar、顶栏、副标题和composerfooter重复暴露session-UUID目录；两条同名“你好”难区分。侧栏品牌/导航占用明显，主内容则以技术身份充当标题。
需要让用户按项目、会话名称、最近活动和真实状态寻找任务，调试ID仅在诊断详情出现。

## 实现范围

统一项目/独立会话的友好名称与会话标题，技术ID在详情可复制；Sidebar的导航、搜索、会话列表建立轻重层级，减少重复品牌口号和元数据。
顶栏保留会话标题、必要状态与明确的检查器/详情入口，避免多个含糊图标。搜索使用真实会话数据，空/无结果/加载状态一致；导航切换保留选中和阅读位置。

## 验收

- [ ] 新建独立会话、同名会话、长项目路径均可读可区分，主界面不出现完整内部UUID。
- [ ] 运行/失败/已完成状态紧凑一致，颜色不是唯一提示。
- [ ] Sidebar搜索与会话切换可键盘完成，选中/hover/focus区分清楚。
- [ ] 顶栏检查器、详情、工作台控制名称与实际归属一致，无重复入口。

## 执行记录

- 设计/实现 commit：见 git log UI07/UI08 合并提交
- 验证：tsc / vitest 70 tests / build 通过。顶栏/Sidebar/composer footer 的 UUID 常驻暴露清除，改为友好标签 + tooltip；同路径完整副本仍可经会话详情/上下文菜单获取。已知部分：Sidebar 搜索空/加载态统一与品牌口号精简未实现。

只在 alva-hub-desktop 的 Tauri/React UI 上实施，不改旧 alva-app-tauri。遵守根/目录 AGENTS.md、FRACTAL-DOCS.md 及 UI glossary/wireframes/motion；与用户最新要求冲突的旧描述按 UI01 更新。每个逻辑改动独立提交，不为过截图直接放宽阈值，不把demo视图冒充真实产品。此轮只review与建卡，未执行本卡实现。
