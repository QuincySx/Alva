# Round 9 修复/补验队列

复审 HEAD `bae66d0f8aeebac36b4a57b84fa955489ae63f0d`；新增 **4 张卡**，均有动态反例。仅本地交付，未发送给外部终端；未修改生产实现。

## A. 新增多引擎修复

| 卡 | 优先级 | 问题 | 验证 |
|---|---|---|---|
| [R92](issues/R92.md) | P1 | 旧取消误伤已认领但未登记的新轮 | Hub gate 反例失败 |
| [R93](issues/R93.md) | P1 | handoff EOF 前重试仍 resume 失败 native ID | 完整请求反例失败 |
| [R94](issues/R94.md) | P2 | Codex/Pi fatal 与成功结果并存 | 两 adapter 反例失败 |
| [R95](issues/R95.md) | P1 | Pi 暂时性错误抢先中止合法重试 | 原生顺序 fixture 失败 |

执行：R92 → R93（同 runtime.rs），R94 → R95（Pi adapter/mapping 相关）。先运行 evidence 复现，再修复并通过相应包；健康成功对照必须继续成立。四卡不涉及 Hub UI 设计，不应扩大到整套架构重构。

## B. 原卡补项，不重复建卡

- [R72](../kernel-boundary-review-round6/issues/R72.md)：并发实现静态符合锁语义；公开 `Scheduling.execution_mode` 注释仍说只有 Parallel 可入波，补同步 Coordinator 的规则。属 SDK 文档收尾。
- [R74](../kernel-boundary-review-round6/issues/R74.md)：Claude Full 的 OutputAfterFatal 应实际注入 fatal 后的输出并做变异；Alva 取消样本保持模型 gate 关闭验证有界终止。属多引擎测试补验，不是已确认生产取消 bug。quarantine 策略仍明确待定，不能由“矩阵已接入”盖过去。

每个补项记录结果与 commit；不要删掉原执行记录，用追加复审结论保留历史。

## C. 未实施的 Hub 产品旧卡照旧，不算本轮新发现

R77/R78（HTTP Stop）、R79（Inspector）、R84/R85/R86/R87（控制面/审批/fallback）、R88/R89（UI异步）、R91（Desktop Stop）及 R45（浏览器行为验收）仍沿原卡推进。

入口：[round7](../kernel-boundary-review-round7/TASKS.md)、[round8](../kernel-boundary-review-round8/TASKS.md)。这些历史索引保留当时快照；截至本轮的修复判断以 [REVIEW](REVIEW.md) 为准。

## 交接约束

先读根/目录 AGENTS 和相关原卡；所有人共享工作区，保留他人修改。每个逻辑改动独立提交，记录真实测试及变异，不只写“全绿”。Pi real_pi 测试依赖机器模型配置，本轮失败原因未定；不得将不受控真实调用作为普通 unit 测试通过的前提，也不要为了验证反例读取真实凭据。

## 执行状态（2026-09-19 收尾轮）

- R92 → done，commit `a49d1d19`；R93 → done，commit `fc45b7eb`（前一轮已入库，本轮复验 hub-core 50/50）。
- R94 → done，commit `36d44ec9`（纯暂存态验证：codex 19/19、pi 14/15 仅 real_pi 环境项；probe exit 0）；R95 → done，commit `c5e37703`（pi 20/21 仅 real_pi 环境项；probe exit 0、recoverable_retry_lost=false）。
- R72 文档补项 → done，commit `358cdfa5`（注释与 `is_parallel_candidate` 一致，kernel-abi 48+1 PASS，public-surface --check OK）。
- R74 两项测试补验 → done，commit `764864da`（alva 31/31、claude 102/102、engine-runtime test-support 56/56；Claude OutputAfterFatal 变异实验真实执行：去 Completed → FAILED "only Completed may follow a fatal Error"，恢复 → PASS）。quarantine 仍 open。
- 以上四提交于 2026-09-19 签名恢复后按 R94 → R95 → R72 → R74 顺序独立入库，gpgsig 均在位；此前 1Password 锁定阻塞约 30 分钟、12 次探测未恢复，解锁后一次通过。
- pi `real_pi_completes_a_turn_conformant` 失败原因为 403 "An active OpenCode Go subscription is required"（机器订阅环境项），维持"不受控真实调用不作为 unit 前提"的交接约束。
