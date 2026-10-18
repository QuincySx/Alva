# 夜间总队列：20 张待修或待验卡

2026-09-17，固定 HEAD `c8aca9c8`，与 round7 相同；本轮新增 11 张（R81–R91），合并既有 9 张（R45、R72–R80）。仅计本次确认需继续处理的卡，不宣称全仓其余历史卡已验收。

执行入口：[HANDOFF](HANDOFF.md)。原实现 agent 的身份/终端没有确认，本目录是可直接交接的队列，不代表已自动开工。

## 阶段 0：先排构建阻断

[R72](../kernel-boundary-review-round6/issues/R72.md) 当前 dirty tool_batch 有 E0382：移动 resource_keys 后 test 日志借用整个结构。先修这一个前置阻断，不必等 R72 全部并发矩阵完成才做其他独立卡。保留其他 agent 的未提交工作。

## 新增卡

| 顺序/组 | 卡 | 级别 | 要修的问题 | 本次证据 |
|---|---|---|---|---|
| A1 | [R83](issues/R83.md) | P2 | Normal harness 不能把 fatal 当成功 | 真实失败流仍被接受 |
| A2 | [R81](issues/R81.md) | P1 | Codex 正常结束被误判失败、答案清空 | execute 反例已失败 |
| A3 | [R82](issues/R82.md) | P1 | Pi 正常结束被误判失败、答案清空 | execute 反例已失败 |
| B1 | [R84](issues/R84.md) | P1 | Desktop 重复 Run 拒绝后删除原控制面 | 静态确认 |
| B2 | [R85](issues/R85.md) | P1 | Hibernate 取消失败仍删活句柄 | 静态确认 |
| B3 | [R86](issues/R86.md) | P1 | 取消失败丢审批、运行悬挂 | 静态确认 |
| B4 | [R91](issues/R91.md) | P1 | Desktop 取消事实/Interrupted 结算过早 | 静态确认 |
| B5 | [R87](issues/R87.md) | P1 | Stop 强杀被当 Crash 并自动 fallback | 实际分类器 + 静态调用链 |
| C1 | [R90](issues/R90.md) | P1 | handoff fatal 后提前重试丢上下文 | 静态确认 |
| D1 | [R88](issues/R88.md) | P2 | 旧 openSession 覆盖较新导航 | App 反例已失败 |
| D2 | [R89](issues/R89.md) | P2 | A 的延迟刷新污染 B 的详情 | App 反例已失败 |

A1 先把验收加强，A2/A3 再修实现；无需要求 A1 中所有 adapter 一开始就绿。B 组同一 Desktop main.rs，单 agent 串行。B3/B4 共享夹具、分开断言。B5 必须同时验证成功取消不继续和真实 crash 仍 fallback。

## 既有卡：继续做，不重开副本

| 卡 | 状态/剩余工作 | 依赖 |
|---|---|---|
| [R80](../kernel-boundary-review-round7/issues/R80.md) | 核心 cancel 各分支仍可能释放新 claim | 控制面组先做 |
| [R77](../kernel-boundary-review-round7/issues/R77.md) | HTTP 迟到 ack 污染新 attempt | R80 → R77 |
| [R78](../kernel-boundary-review-round7/issues/R78.md) | 拒绝重复 Run 清掉 HTTP Stop marker | R77 → R78 |
| [R79](../kernel-boundary-review-round7/issues/R79.md) | Inspector attempt+tool 身份及真实焦点 | 与 D 组共享 UI，串行 |
| [R72](../kernel-boundary-review-round6/issues/R72.md) | dirty 实现未完、编译阻断、真实 child 并发矩阵未验 | 阶段 0 后继续 |
| [R73](../kernel-boundary-review-round6/issues/R73.md) | 测试的 A session 仍未接入首轮 | 独立 |
| [R74](../kernel-boundary-review-round6/issues/R74.md) | Alva KilledMidRun、Claude Full 矩阵等未全覆盖；quarantine 保持真实状态 | A 组后用加强的 harness |
| [R75](../kernel-boundary-review-round6/issues/R75.md) | 测试导航及 spacing token 收尾 | 最后同步 |
| [R45](../kernel-boundary-review-round5/issues/R45.md) | 真实浏览器审批、滚动、密度、键盘等验收未完 | UI 修复后 |

## 最终交付要求

每张卡记录 commit、具体测试、真实结果；未完成的明确保留 open。与本卡相关的反例修后必须能通过，反向变异仍能失败。共享文件不并发覆盖，不以“现有测试全绿”代替新增行为验收。
