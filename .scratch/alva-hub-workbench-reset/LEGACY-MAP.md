# H01 · 旧卡去重映射（LEGACY-MAP）

- 目的：R 系列旧缺陷编号是唯一缺陷身份。本映射不覆盖历史记录，只登记"定义在哪、当前状态、由哪张 H 卡执行"。
- 基线：764864da；核实方式：`git log --all --grep` 未找到以 R77-R91/R96/R97 命名的修复 commit；round9（2026-09-19）明确这些旧卡"照旧待执行"。
- 旧 UI 总表纠错：`kernel-boundary-review-round5/HISTORICAL-UI.md` 已审计算定 `.scratch/alva-hub-tauri-ui-system` README/TASKS（2026-08-30）"任务 00-12 全部完成、67/67 通过"与实现不符——Browser/Terminal 从未接入（当前前端为诚实禁用，`web/src/components/workbench-tools.tsx:541-543、557-559`）。旧总表不作为任何"已验收"证据。

## 映射表

| 编号 | 缺陷一句话 | 定义/证据路径 | 当前状态 | 执行入口 |
|---|---|---|---|---|
| R45 | 真实产品四档发送/停止/审批、无程序化键盘跳焦、真实滚动恢复验收从未补齐 | `.scratch/kernel-boundary-review-round5/issues/R45.md` | 部分执行，open（23 playwright + 99 vitest 绿 ≠ 真实 Tauri 验收） | H18 最终验收引用，不重复开卡 |
| R77 | HTTP 迟到 Stop ack 把取消事实/marker 写进新 attempt | `.scratch/kernel-boundary-review-round7/issues/R77.md`（源 round6 R65） | open，静态确认 | **H15** |
| R78 | 被拒重复 Run 在认领前删除活动轮 Stop marker 致 Interrupted 丢失 | `.scratch/kernel-boundary-review-round7/issues/R78.md` | open，静态确认 | **H15** |
| R79 | Inspector Locate 只按 tool ID、无 attempt 限定、无真实焦点转移 | `.scratch/kernel-boundary-review-round7/issues/R79.md`（源 round6 R67） | open，静态确认 | **H14**（依赖 H03/H09） |
| R84 | Desktop 重复 Run 被拒后无条件删 active_hubs/permissions，原 Stop/审批失效 | `.scratch/kernel-boundary-review-round8/issues/R84.md` | open | **H15** |
| R85 | hibernate_worker 对 cancel Err 只 debug 并删活句柄，任务失控 | `.scratch/kernel-boundary-review-round8/issues/R85.md` | open | **H15** |
| R86 | Stop 在 await cancel 前 clear_session，取消失败丢审批路由且运行悬挂 | `.scratch/kernel-boundary-review-round8/issues/R86.md` | open | **H15** |
| R87 | Claude cancel grace 强杀文案被分类 Crash 并触发自动 fallback | `.scratch/kernel-boundary-review-round8/issues/R87.md`（涉及 `alva-engine-adapter-claude/src/adapter.rs:391-428`、`desktop/failure.rs:128-134`） | open（分类器实跑 Crash/fallback=true） | **H15** |
| R88 | 过期 openSession 响应无条件安装，覆盖较新导航（A→New 被旧 A 重开） | `.scratch/kernel-boundary-review-round8/issues/R88.md`（`web/src/App.tsx:962-981,1040-1079`） | open，动态反例 EXPECTED FAIL | **H15** |
| R89 | 旧会话迟到终态/fallback 刷新把 A 的详情写进 B 的 Details | `.scratch/kernel-boundary-review-round8/issues/R89.md`（`web/src/App.tsx:697-714,1087-1090`） | open，动态反例 EXPECTED FAIL | **H15** |
| R91 | Desktop Stop 在真实结束前写取消事实并广播 Interrupted，UI 文案虚假 | `.scratch/kernel-boundary-review-round8/issues/R91.md`（`desktop/main.rs:1249-1265`） | open | **H15** |
| R96 | Pi context-overflow 压缩恢复被 pending error 提前 fatal 截断、丢合法恢复答案 | `.scratch/kernel-boundary-review-round10/TASKS.md`（`alva-engine-adapter-pi/src/mapping.rs:118-126`、`adapter.rs:181-182`） | 待修复，动态反例已复现（exit 101） | **H16**（直接执行 round10 TASKS） |
| R97 | Hub handoff 无显式 Error 的 SessionStarted→EOF 走合成 fatal，重试窗口先于 handoff 身份恢复 | `.scratch/kernel-boundary-review-round10/TASKS.md`（`hub-core/runtime.rs:977,993,1021,1327`） | 待修复，静态并发交错确认 | **H16** |

## 已修、不得重报的编号

- R72（内核 `Scheduling::execution_mode` 注释）：已修，commit `358cdfa5`。
- R74（适配层取消/pending-model 终止补验）：已修，commit `764864da`。
- R92-R95（hub-core 释放/首交接轮/committed terminal/临时 assistant error）：已修，commits `a49d1d19`/`bae66d0f`/`fc45b7eb`/`36d44ec9`/`c5e37703`。
- R53/R54（Browser/Terminal/Text 伪连接、Git 面板非真实状态）：已在 round6 诚实化为"未接入"占位（round6/UI.md），后续由 H10-H13 接真实实现。
- R41/R42（聊天滚动假证明、工具事件归位）：R42 已按当时规范落地；H02 迁移其位置决策，不重报缺陷。

## 去重规则

1. 任何修复 commit 的 message 必须携带原 R 编号；禁止另造新缺陷编号指代同一问题。
2. round9 三探针与各原卡验收矩阵是 H15/H16 的回归底线。
3. 历史记录文件只追加不改写；状态变化以新 commit + EXECUTION.md 记录为准。
