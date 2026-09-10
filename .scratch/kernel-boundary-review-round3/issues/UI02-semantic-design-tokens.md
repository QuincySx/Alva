# UI02: 重建字体、颜色、间距与表面层级 tokens

**Priority:** P1
**Axis:** 用户最新 UI 重整需求 / 产品审查
**Origin:** 2026-09-11 用户“你好”真实会话及全 UI 反馈；HEAD `f9d0e57`
**Blocked by:** UI01
**Status:** ready-for-agent

## 问题与证据

tokens.css 主要提供字体族，字号/行高/字重仍散落：hub-components.css:69–91 混用10/11/12/20px，primitives.css:204–230 另设标题。hover/tooltip/badge/status 又在业务/基础CSS各配色；存在未定义 --color-surface-sunken。
真实“你好”截图出现多层近似底色、重边框、紫/绿整块消息与弱辅助文案，主要信息和诊断信息缺少清晰层级。用户明确要求全面整理design token，不能只改品牌色。

## 实现范围

建立有限语义角色：页面/分组标题、正文、辅助、代码的字号行高字重；canvas/panel/inset/overlay 表面；默认/hover/selected/focus 与 success/warning/error 的前景、背景、边框；spacing/radius/shadow/z-index。
先以聊天和Inspector同屏验证：正文建议14–15px起、稳定行高，辅助信息不承担正文任务；状态色用于明确反馈，避免每段回答整面着色。所有值只有一个来源，消除未定义/重复命名。浅色或跟随系统若未完整实现，生产设置应诚实禁用。

## 验收

- [ ] 有可浏览的token角色表和真实聊天/Inspector样例，不仅色板。
- [ ] 中英文正文、长代码、辅助标签可读且层次清楚；状态对比度达到现有规范。
- [ ] 无未定义token；相同语义不在多个CSS复制不同值。
- [ ] 各页面使用同一表面、分隔和字体层级，减少层层盒子的视觉噪声。

## 执行记录

- 设计/实现 commit：ff27def
- 验证：`npx tsc --noEmit` 通过；`npm run build` 通过；`npx vitest run` 6 files / 70 tests 全部通过；token 一致性脚本确认无未定义 var() 引用（radix/* 与 TSX 运行时注入除外）。字体 8 档 / 表面 7 角色 / 状态 3×3 三元组收口完成，聊天正文提升为 14px 主文字色，UI Lab `?ui-lab=1` 新增 Tokens 角色表页。

只在 alva-hub-desktop 的 Tauri/React UI 上实施，不改旧 alva-app-tauri。遵守根/目录 AGENTS.md、FRACTAL-DOCS.md 及 UI glossary/wireframes/motion；与用户最新要求冲突的旧描述按 UI01 更新。每个逻辑改动独立提交，不为过截图直接放宽阈值，不把demo视图冒充真实产品。此轮只review与建卡，未执行本卡实现。
