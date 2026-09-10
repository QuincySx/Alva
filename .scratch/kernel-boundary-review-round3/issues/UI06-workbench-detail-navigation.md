# UI06: 合并重复详情并统一工作台导航与滚动

**Priority:** P2
**Axis:** 用户最新 UI 重整需求 / 产品审查
**Origin:** 2026-09-11 用户“你好”真实会话及全 UI 反馈；HEAD `f9d0e57`
**Blocked by:** UI01、UI03、UI05
**Status:** ready-for-agent

## 问题与证据

workbench-tools.tsx:183–275 在Native Inspector下面再堆产品Turn→Attempt→Tool、handoff、branches、timeline、memory和live events；session-details.tsx再次展示其中多项。真实截图中有多个“检查器”区块、重复回答和层层滚动。
App.tsx:1705–1715 将顶栏Details实际插在全部聊天记录后方，长聊天打开详情可能看不到目标。

## 实现范围

产品Attempt用于选择执行上下文，native步骤作为该Attempt的详情，不再串成两套独立检查器。会话记忆/设置/branches归到明确的会话详情面；Agent tab只负责协作关系。
Details在打开时立即显示对应工作台页或窄屏Sheet，保留关闭/返回和聊天位置。工作台每页一个主scroll owner；tab切换、关闭、重新打开保留合理状态。历史Alva与当前其他引擎必须明确区分，不自动冒充当前运行。

## 验收

- [ ] 同一工具/失败只有一个主详情入口，跨摘要跳转不重复堆内容。
- [ ] 长聊天点击Details立即可见，不用滚到消息末尾；关闭不改变聊天位置。
- [ ] 进入子Agent/LLM/Tool后Back/Esc按层级返回，焦点和原滚动位置恢复。
- [ ] 非Alva展示真实支持范围；当前/历史Attempt及任务名称一眼可辨。
- [ ] 工作台无同一区域竞争的多层纵向滚动。

## 执行记录

- 设计/实现 commit：9f78993
- 验证：`npx tsc --noEmit` 通过；`npx vitest run`（web/ 目录）70 tests 通过；`npm run build` 通过。Details 改为右侧即时 rail（长聊天打开即见、不改变聊天位置）；产品尝试面板更名并默认折叠，与原生 Inspector 角色分工明确。已知部分：Product Attempt↔native step 的跨层选择联动与滚动位置逐层恢复未实现（结构性大改，另卡跟进）。

只在 alva-hub-desktop 的 Tauri/React UI 上实施，不改旧 alva-app-tauri。遵守根/目录 AGENTS.md、FRACTAL-DOCS.md 及 UI glossary/wireframes/motion；与用户最新要求冲突的旧描述按 UI01 更新。每个逻辑改动独立提交，不为过截图直接放宽阈值，不把demo视图冒充真实产品。此轮只review与建卡，未执行本卡实现。
