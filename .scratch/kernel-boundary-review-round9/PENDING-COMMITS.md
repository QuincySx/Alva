# PENDING COMMITS — 1Password 签名锁定期间未能落库的四个提交

2026-09-18。R92（a49d1d19）/ R93（fc45b7eb）已入库；以下四个逻辑改动已完成并全部验证，
但 git commit 的 SSH 签名（1Password op-ssh-sign）在 R93 之后因金库自动锁定失败
（`1Password: failed to fill whole buffer`，`ssh-add -l` → no identities）。
改动分别处于 index / 工作区，**解锁后按顺序执行即可**，每条已经是独立的干净提交：

```sh
# 1. R94 —— 当前 index 里已暂存的就是这一组，直接 commit（不要 -a）
git commit -m "fix(engine-adapters): a committed native terminal settles the turn; trailing noise never pairs fatal with success (R94)

native_terminal_seen 之后，adapter 侧错误路径（读取失败 / 迟到
cancel）仍发出 fatal Error，但最终结算只看 mapper.failed 与
native_terminal_seen——正常原生终态之后、EOF 之前发生错误时，输出
fatal Error 的同时仍携带成功答案，终态自相矛盾。

结算边界明确为：原生终态提交成功即定型。终态之后的尾部传输失败与
迟到 cancel 只结束读取（warn 记录），不发 fatal，已提交的答案与
usage 原样保留；终态之前的读取失败/取消仍按 fatal + result=None
结算。禁止 fatal Error 与 Completed(Some) 并存。

回归覆盖两 adapter 的 terminal→读取错误（非法 UTF-8 尾行）与
terminal→cancel；健康对照、KilledMidRun、EofWithoutResult 等既有
剧本不变。codex 19/19；pi 14/15（唯一失败为 real_pi 上游 401
CreditsError 环境项，与本改动无关）。"

# 2. R72 文档补项
git add crates/alva-kernel-abi/src/tool/scheduler.rs
git commit -m "docs(kernel-abi): Scheduling::execution_mode contract matches the R72 wave rule

公开契约注释仍写\"仅 Parallel 且 concurrency_safe 可入并行波\"，与
5a03b95b 之后的 wave composer 实现（concurrency_safe 且非
SerialGlobal 即候选，concurrency-safe Coordinator 同样入波）不一致。
只改注释，与 tool_batch.rs::is_parallel_candidate 逐条对齐；公开
符号快照不含文档注释，无需更新快照。kernel-abi 48 passed + 1 ignored。"

# 3. R74 两项测试补验
git add crates/alva-engine-adapter-alva/src/adapter.rs crates/alva-engine-adapter-claude/src/adapter.rs
git commit -m "test(engine-adapters): alva killed sample cancels a pending-forever model; claude Full output_after_fatal feeds real post-fatal frames (R74 re-review)

① Claude Full full_output_after_fatal_conforms 此前 generator throw
即结束，fatal 后没有任何输入。fixture 改为 error-subtype result 之后
继续产出 assistant/system 帧；变异（去掉 fatal 与 Completed 的成对
抑制）真实执行并按预期 FAILED：event[3] 违反 'only Completed may
follow a fatal Error'。恢复后 102/102。

② Alva killed_mid_run_conforms 此前 cancel 后放行 tool call，证明不
了持续 pending 的模型可独立取消。改为 PendingForeverModel（进入 LLM
调用后永不产出，complete() 同样挂起），cancel 后有界收敛 fatal +
Completed{None}：PASS（0.03s）。alva 31/31。"

# 4. R95
git add crates/alva-engine-adapter-pi/src/mapping.rs crates/alva-engine-adapter-pi/src/adapter.rs crates/alva-engine-adapter-pi/src/AGENTS.md
git commit -m "fix(engine-adapter-pi): provisional assistant errors defer to the native retry decision (R95)

Pi 0.85.1 真实时序：message_update(error) → message_end(error) →
agent_end(willRetry=true) → auto_retry_start → 重试成功 → agent_end(
false)。mapper 此前凭单个 assistant 错误立即定性 fatal，adapter 随
即停止读取，CLI 的合法重试被截断。

新增 pending_error 待定状态：assistant 错误帧只记录不外发；定性点在
agent_end(willRetry:false)（携带未恢复错误 → 唯一 fatal）或
auto_retry_end(success:false)（重试耗尽，既有路径）；成功 assistant
消息清除待定错误。willRetry(true) 后 EOF 仍按缺终态 outage 报 fatal。

真实顺序 fixture 四条（重试恢复/重试耗尽/不可重试/EOF 挂起重试）+
mapper 单测两条；probe 的 recoverable_retry_lost 转为 false，全套
exit 0。pi 20/21（唯一失败为 real_pi 401 CreditsError 环境项）。"
```

执行后把 R94/R95/R72/R74 卡执行记录里的"commit: 待记录/阻塞"改为真实 hash。
