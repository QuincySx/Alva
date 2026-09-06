# 07 Packaged UX Release Smoke

> 预检状态（2026-08-30）：浏览器 fixture 验证已完成，G6 尚未通过。详见
> [`../evidence/ui-preflight/REPORT.md`](../evidence/ui-preflight/REPORT.md)。最终签名包与
> VoiceOver 验收仍依赖 Tickets 02、05、06。

## 目标

在最终签名包上验证发布关键交互，不重新打开已完成的 UI redesign 范围。

## 工作

- 以键盘完成新会话、Engine/model 选择、发送、Stop、审批、fallback 查看和 switch back；
- 检查 focus visible、popover/dialog 关闭后的焦点恢复和无焦点陷阱；
- 在中文、英文、1440×900、1280×800、最小窗口和 Workbench overlay 下 smoke；
- 开启 reduced-motion 验证空间动画降级且状态反馈保留；
- 检查 VoiceOver 可读名称、状态、错误和动态 timeline 通知；
- 在触控板/鼠标下检查滚动所有权、popover 边界和 resize。

## 验收

- 所有 release-critical flow 可仅用键盘完成；
- 无黑屏、双滚动、焦点丢失、overlay 越界或中英文破版；
- reduced-motion 和 VoiceOver 结果记录在最终签名 artifact 身份下；
- 发现的问题按 release blocker / follow-up 分类，不在 smoke 中顺手重构设计系统。

## 依赖

Tickets 02、05、06。
