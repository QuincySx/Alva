# 03 — 实现 Chat-First 主界面

**Status:** planned

## Goal

用一个聊天窗口取代 timeline 仪表盘，形成用户每天使用的主表面。

## Scope

- 用户指令右对齐气泡；
- assistant 回复使用 Markdown/代码渲染并实时流式更新；
- tool/reasoning/progress 合并成可展开的「已工作 Ns」组；
- 支持自动滚动、用户离开底部后暂停跟随、回到底部按钮；
- 空状态、错误状态和 composer 使用 gpui-component。

## Acceptance

- 新建、运行、流式响应、恢复旧会话都只通过聊天主表面完成；
- 长对话滚动流畅，流式更新不跳动；
- 不再把普通 turn 渲染为 dashboard status card；
- 键盘可完成输入、发送、停止和展开工作组。

**Blocked by:** 01 — 建立 gpui-component UI 基线; 02 — 建立统一 ChatItem 投影模型.
