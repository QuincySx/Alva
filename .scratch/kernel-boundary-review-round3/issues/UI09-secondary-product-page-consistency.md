# UI09: 统一设置、自动化、插件等次级页面层级

**Priority:** P2
**Axis:** 用户最新 UI 重整需求 / 产品审查
**Origin:** 2026-09-11 用户“你好”真实会话及全 UI 反馈；HEAD `f9d0e57`
**Blocked by:** UI01、UI02、UI03、R20、R21、R22
**Status:** ready-for-agent

## 问题与证据

原生设置页混用emoji导航、大量卡片、硬编码Wang/Pro资料和与真实策略无关的开关；插件/自动化也呈现可执行样例。真实性由R20–R22处理，本卡负责统一布局、文案与层级。
现有实现不同页各自创建字体、surface与控件样式，用户难以区分已保存状态、样例、不可用功能和真正可用配置。

## 实现范围

按一套SettingsFrame/PageHeader/Group/Row构建次级页面；统一图标语言、标题、说明、操作与保存反馈。生产页面不出现虚构账号/套餐、占位连接或装饰性成功徽标。
高频真实设置靠前，高级诊断与尚未开放功能明确归类；provider配置的发现/测试/保存三步有明确状态且失败保留输入。与R20–R22约定真实数据来源，不重复实现它们的后端功能。

## 验收

- [ ] 设置/自动化/插件共享字体、行高、分组、空态、加载与错误surface。
- [ ] 成功保存与仅编辑草稿有明显区别；失败信息与对应操作同处。
- [ ] 无硬编码个人身份/Pro套餐/伪安装状态，样例仅在demo/UI Lab。
- [ ] 新建/编辑/取消/返回、长文案和无配置状态均可用。

## 执行记录

- 设计/实现 commit：见 git log UI09
- 验证：tsc / vitest 70 tests / build 通过。虚构 Pro 套餐/账号移除、emoji 导航清理、SSH/MCP 样例数据如实标注并声明未接后端。已知部分：SettingsFrame/PageHeader 全套页面级统一重构与保存/草稿状态区分未完成（automation/extensions 页的真实性已由 R20/R22 落地）。

只在 alva-hub-desktop 的 Tauri/React UI 上实施，不改旧 alva-app-tauri。遵守根/目录 AGENTS.md、FRACTAL-DOCS.md 及 UI glossary/wireframes/motion；与用户最新要求冲突的旧描述按 UI01 更新。每个逻辑改动独立提交，不为过截图直接放宽阈值，不把demo视图冒充真实产品。此轮只review与建卡，未执行本卡实现。
