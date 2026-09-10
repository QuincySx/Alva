# UI10: 落实响应式、键盘与状态矩阵

**Priority:** P2
**Axis:** 用户最新 UI 重整需求 / 产品审查
**Origin:** 2026-09-11 用户“你好”真实会话及全 UI 反馈；HEAD `f9d0e57`
**Blocked by:** UI03、UI04、UI05、UI06、UI07、UI08、UI09
**Status:** ready-for-agent

## 问题与证据

primitives.css:422–434 在1023px以下仍固定220px Sidebar，ui/layout.tsx:56常驻渲染；未实现文档<760px Sidebar Sheet。工作台/聊天/输入之间也需要在重新定稿后验证，不能仅把三栏压缩。
现有截图回归多是固定1440×900 demo，不能覆盖用户真实问候、超长hooks、失败与窄窗口的交互可用性。

## 实现范围

以AppFrame统一四档区域关系：≥1360、1024–1359、760–1023、<760；窄屏Workbench和必要Sidebar使用可关闭Sheet，Main不被双重挤压。
统一键盘焦点顺序、可见focus、Esc关闭/返回、overlay焦点恢复、reduced-motion和状态反馈。测试覆盖真实内容/长文案/错误/审批，而非仅静态正常截图。R23负责最终视觉基线收口。

## 验收

- [ ] 四档viewport无非预期水平溢出，composer/发送/停止/审批一直可达。
- [ ] narrow Sidebar与Workbench独立开关，退出后焦点返回正确；每Region单一scroll owner。
- [ ] 仅键盘完成新会话、选模型、发送、开Inspector、下钻和返回。
- [ ] 正常/reduced-motion、深色、中英文长文案、空/加载/失败/禁用矩阵有证据。
- [ ] 最終真实Tauri/store流程与确定性截图共同验收，不以demo通过代替产品通过。

## 执行记录

- 设计/实现 commit：见 git log UI10
- 验证：tsc / vitest 70 tests / build 通过。<760px Sidebar 独立 sheet + scrim + Esc + 自动收起，顶栏菜单按钮仅窄屏显示。已知部分：四档 viewport 的截图证据矩阵、长文案/失败/审批窄屏实测与真实 Tauri 端到端验证未完成，与 R23 收口时补齐。

只在 alva-hub-desktop 的 Tauri/React UI 上实施，不改旧 alva-app-tauri。遵守根/目录 AGENTS.md、FRACTAL-DOCS.md 及 UI glossary/wireframes/motion；与用户最新要求冲突的旧描述按 UI01 更新。每个逻辑改动独立提交，不为过截图直接放宽阈值，不把demo视图冒充真实产品。此轮只review与建卡，未执行本卡实现。
