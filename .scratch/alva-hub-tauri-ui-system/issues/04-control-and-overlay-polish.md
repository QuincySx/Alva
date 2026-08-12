# 04 Control and Overlay Polish

## 目标

逐个整理 L1 基础控件，统一两档密度、hover、active、focus、disabled、loading、open 和 close。

## 工作

- Button/IconButton、Field、Select、Checkbox、Switch、Tabs；
- Tooltip 连续浏览、Popover trigger-origin、Dialog center-origin；
- Menu/Popover/Dialog 使用可中断 transition；
- Disclosure 快速反向和内容高度处理；
- overlay size variants：compact/default/wide/full-height；
- 使用 composition、slots 和有限 typed variants；
- 中英文长短文案 fixture 和 WCAG AA 对比度检查。

## 验收

- 每个组件有完整状态矩阵和行为测试；
- 触屏无 sticky hover，键盘完整可操作；
- 快速开关弹层不会闪回起点；
- focus ring 不被 overflow 裁切；
- 业务层无需覆盖 primitive 视觉状态。
