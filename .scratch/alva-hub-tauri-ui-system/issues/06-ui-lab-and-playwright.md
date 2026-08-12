# 06 UI Lab and Playwright Baseline

## 目标

在真实 Vite/Tauri web 环境建立开发测试专用组件展台和稳定视觉基线。

## 工作

- 增加开发/test-only UI Lab 入口；
- 为 Control、Region、Surface、Domain component 建立确定性 fixture；
- 覆盖 default/hover/active/focus/disabled/loading/open/close/error/empty；
- 增加 default/compact、英文/中文、normal/reduced-motion 组合；
- 引入 Playwright 固定视口截图与差异图；
- 保留少量连接真实 store/Tauri command 的 live scenario。

## 验收

- 正式产品导航不暴露 UI Lab；
- 每个 fixture 可通过稳定 URL/ID 直接打开；
- 固定数据和动画时钟下截图可重复；
- Playwright web 基线与 Tauri smoke 职责分离；
- fixture 组件与真实业务使用同一实现。
