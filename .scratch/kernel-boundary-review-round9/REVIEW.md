# Round 9 复审：修复主体有效，仍有边界与验收缺口

范围固定 `git diff c8aca9c8...bae66d0f`，10 个提交、14 个文件，来源为 R72–R75、R80–R83、R90 本地任务卡。没有重新检查全部历史问题，也没有把未实现的 Hub UI 卡重复算成新增问题。

## Standards

**1 项明确规范补项，P3**：R72 修改并行波支持 Coordinator，但 `crates/alva-kernel-abi/src/tool/scheduler.rs:99` 的公开注释仍限制只有 Parallel。FRACTAL-DOCS 要求职责变化同步对应文档。补回原 R72，不新开泛化重构卡。

R72 实现与 Coordinator 不持锁的既有语义相容；本轮未发现新的 SDK/app 依赖边界违规。未根据主观代码气味另开卡；工具已提示的 warning 不列成新规范发现。

Standards：1 项，最高 P3（公开调度契约注释不同步）。

## Spec

**4 项已动态确认的行为问题**：

1. R80 取消守卫仍漏“新 claim 已认领、RunHandle 未登记”窗口，旧取消将新 Starting 置 Failed → [R92](issues/R92.md)，P1。
2. R90 只恢复提示词，EOF 前重试还带失败 native ID，与 EOF 后恢复契约不一致 → [R93](issues/R93.md)，P1。
3. R81/R82 的原生终态识别有效，但随后读取错误导致 fatal+成功结果并存 → [R94](issues/R94.md)，P2。
4. R82 没有覆盖真实 Pi 重试顺序，assistant error 被过早判 fatal、后续 retry 永远读不到 → [R95](issues/R95.md)，P1。

**2 项测试证明不足**（均补回 R74，P2）：Claude Full OutputAfterFatal 的输入只 throw/结束，没有 fatal 后输出；Alva KilledMidRun 主动放行模型 gate，没证明持续 pending 时取消可收敛。当前内核已 race stream.next/complete 与取消，因此后者不列为生产故障。

Spec：6 项（4 个行为问题、2 个测试补验），最高 P1 为取消代际/handoff 身份/原生重试。

## 已修复部分与范围界线

| 原卡 | 本轮判断 |
|---|---|
| R72 | E0382 调试残留已删除，Coordinator opt-in 与锁语义相容；r11 专项 7/7 通过；保留一项公开注释补充 |
| R73 | A 绑定真实首轮，并先证消息存在，再验无污染；r10 专项 1/1 通过 |
| R74 | Alva/Claude Full 矩阵已接入，但上述两项行为证明需补；quarantine 仍待决 |
| R75 | 新测试目录导航与聊天 spacing token 已提交，本轮静态检查未发现这两处残留 |
| R80 | 原两种交错修复存在，但 R92 说明整体代际安全尚未闭环 |
| R81/R82 | 真实成功对照均通过、答案保留；边界行为继续 R94/R95 |
| R83 | Normal 现在拒绝 fatal；有健康无文本/可恢复错误对照，既有 harness 测试通过 |
| R90 | 原 banner 丢失已修，完整 native 身份验收继续 R93 |

Agent SDK 本轮没有确认新的生产架构问题；新增四卡属于多引擎衔接。Hub UI/HTTP/Desktop 的未实施卡独立保留，不能据此宣称 Agent 架构拆解一直没有进展。

## 验证与交付

[实际命令与边界](evidence/RESULTS.md)。Hub/kernel/runtime 常规 lib 测试通过；R72/R73 专项 8/8 通过；Codex 17 通过；Pi 12 通过、1 个机器配置相关真实模型测试失败（原因未定位，不作为 R82 失败结论）。独立反例共 5 个失败，分为 4 张卡；健康 adapter 对照 2 个通过。

[修复队列](TASKS.md)。本轮只有 review 文件和探针，未改产品源码、未提交推送、未投递外部终端。
