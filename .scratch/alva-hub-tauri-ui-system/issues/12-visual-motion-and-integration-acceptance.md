# 12 Visual, Motion, and Integration Acceptance

## 目标

完成所有页面、组件、动效和真实业务的最终验收。

## 工作

- 67 个状态逐一点击并保存 Hub 截图与差异图；
- 1440×900、1280×800、窄窗口和 Workbench 展开态；
- 鼠标、键盘、触屏能力、英文/中文和 reduced-motion；
- 0.25× 慢放检查开关和快速反向；
- fixture 与真实 store/Tauri command 双路径；
- React tests、TypeScript、Vite build、Playwright、Tauri package；
- 严格 UI review 使用 Before/After/Why 表格。

## 验收

- 67/67 状态有 reference/linework/current 三联验收；
- 无黑屏、双滚动、焦点丢失、弹层越界和文案破版；
- 动效符合 `docs/ALVA-HUB-UI-MOTION.md`；
- 键盘完整可用，正文与控件满足 WCAG AA；
- macOS 视觉验收通过且实现未写死平台；
- 完整测试、构建、打包通过后再提交完成状态。
