# 第三轮 Review 与 Hub UI 重整交接

**本轮执行记录已归档；当前唯一返工清单为 [第四轮 R24–R37](../kernel-boundary-review-round4/TASKS.md)。原20卡完成声明的复审见 [第四轮 REVIEW](../kernel-boundary-review-round4/REVIEW.md)。**

本轮用户要求只 review、跑测试、启动 Hub 检查并建卡，修复交给其他执行者。未修改生产代码，未提交或推送。

## 最新需求（2026-09-11）

用户在真实 Hub 发送“你好”后要求全面整理 UI：主聊天回归常规 Agent 的消息阅读/操作体验；execution timeline 只放检查器；Inspector默认显示重点，Middleware移入额外展开/详情，避免占用大块空间；整体design tokens、布局、控件与所有主要页面共同重整。
[真实问题截图](./evidence/native/user-hello.png)及[界面树](./evidence/native/user-hello.txt)是本轮验收样本。旧文档若要求主聊天三泳道，按用户最新要求由UI01统一更新，不以旧基线维持用户已否定的布局。

## 范围与来源

- 核心复审：`git diff 134b8b3...f9d0e57`，R09–R13对应5个实现/快照提交。
- Hub：当前前端单测、生产构建、Playwright回归；原生Tauri实际检查自动化、插件、权限设置和用户“你好”的聊天/Inspector。桌面启动使用已有二进制加载当前Vite前端，最新Rust桌面构建未完成，不能冒充最新Rust全链路验收。
- 核心运行/架构问题R14–R19；Hub真实功能/验证问题R20–R23；UI整体验收UI01–UI10。各卡明确区分静态证据与实测，具体结果见[evidence](./evidence/README.md)。
- 上一轮[修复卡R09–R13](../kernel-boundary-review-round2/README.md)、更早R01–R08和原01–19仅作来源；归档不表示无缺陷，不重新按旧待办开工。

## 开工要求

1. 核对最新HEAD与工作区，先确认卡中问题仍存在，不覆盖其他执行者的改动。
2. 读根与目录AGENTS、FRACTAL-DOCS；Hub UI还须读UI glossary/wireframes/motion与用户最新要求。
3. 依赖顺序见TASKS。UI先定共同信息架构/tokens/controls，再做聊天和Inspector，再扩展各区域；不能只换配色或更新截图掩盖问题。
4. 每卡填写commit、真实验证与结果。核心最小契约、真实持久化/取消并发、真实Tauri命令与视觉/交互分别验收。
5. 不将局部useState、demo或静态样例包装成真实成功；未实现的生产功能诚实禁用/隐藏。

## 文件导航

| 文件 | 用途 |
|---|---|
| [TASKS.md](./TASKS.md) | 唯一待办、优先级、依赖、交接指令 |
| [REVIEW.md](./REVIEW.md) | Standards / Spec 独立结论与UI审查 |
| [evidence/README.md](./evidence/README.md) | 实际验证矩阵、复现源码、日志与截图 |
| `issues/R*.md` | 10张运行/产品/验证修复卡 |
| `issues/UI*.md` | 10张用户要求的UI重整卡 |
