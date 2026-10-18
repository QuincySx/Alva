# Cindy UI Reference & ALVA Hub Redesign Assets

本目录是针对 Cindy（0.1.44 / 0.1.45）的本地交互取证审计与 ALVA Hub 重构参考归档库。

---

## 📁 目录结构

- **`ascii-wireframes/`**：**【标准规范】** 重新绘制的 7 大核心模块专业 **ASCII 线稿库**，包含字符级精确定位、区域标注（`[SCROLL]` / `[DOCK]` / `[OVERLAY]`）、响应式规范与交互流说明。
- **`screenshots/`**：
  - `screenshots/v0.1.44/`：0.1.44 版本 67 张原始截图。
  - `screenshots/v0.1.45/`：0.1.45 版本 43 张增量交互与取证原图（包含 Goal 模式、多选长图分享、detached 独立窗口、SSH 连接、微信 IM 门禁等）。
- **`product-docs/`**：产品策略、UX/UI 规范、状态机矩阵、功能需求与系统集成蓝图（01~07 完整规范）。
- **`source-analysis/`**：逆向代码分析、路由组件映射表、IPC 通信接口、业务状态机与数据模型。
- **`HUB-REDESIGN-SPEC.md`**：ALVA Hub 改版的布局、领域组件和验收规范。
- **`COMPONENT-CATALOG.md`**：UI 领域组件与基础组件映射目录。

---

## 📑 ASCII 标准线稿索引

完整规范见 [ascii-wireframes/README.md](./ascii-wireframes/README.md)：

1. **[01-chat-shell-and-timeline.md](./ascii-wireframes/01-chat-shell-and-timeline.md)**：聊天优先三栏主壳、单阅读轴时间线、折叠活动行（Disclosure）、改动审查卡片、长对话多选分享图片模式。
2. **[02-new-conversation-and-composer.md](./ascii-wireframes/02-new-conversation-and-composer.md)**：新会话空白首屏、纯对话/项目切换浮层、Composer 完整操作菜单、Goal 自主目标与高级限制、权限与模型选择器。
3. **[03-right-workbench.md](./ascii-wireframes/03-right-workbench.md)**：右侧多标签工作台架构、添加标签菜单、系统与进程资源用量表、Subagent 活跃树、独立多窗口工作台。
4. **[04-automation.md](./ascii-wireframes/04-automation.md)**：自动化任务列表、模板网格、创建/编辑表单、Cron 及相对时间调度选择器。
5. **[05-plugins-and-skills.md](./ascii-wireframes/05-plugins-and-skills.md)**：插件与技能双标签首页、已安装/推荐市场卡片、插件详情/配置/权限弹窗、本地技能库管理。
6. **[06-settings-and-account.md](./ascii-wireframes/06-settings-and-account.md)**：设置双栏导航、通用与外观（主题/字体/Worker/LSP）、个人资料、模型供应商管理与排序、自定义端点、远程 SSH 主机、IM 机器人通道。
7. **[07-context-menus-and-overlays.md](./ascii-wireframes/07-context-menus-and-overlays.md)**：项目/会话右键菜单、侧栏全局搜索与多维筛选、侧栏整理弹层、版本更新公告弹窗。

---

## 🎯 关键架构与交互原则

1. **聊天优先（Chat-First）**：页面保持单阅读轴，正文最大宽度 820 px；工具执行与审查紧凑折叠在时间线内，不跳离当前会话。
2. **多标签工作台（Tabbed Workbench）**：右栏支持自由打开/关闭多个工具标签（文件、Diff 审查、Subagent、终端、资源监控），并支持弹出为独立副屏窗口。
3. **极简入口，渐进暴露**：新会话首先在 Composer 内选择项目/纯对话、模型与权限，不强迫跳转复杂设置页。
4. **统一卡片与弹层**：自动化、插件、设置与右键菜单统一复用低对比度边框、微圆角与标准 Popover/Dialog 规范。
