# Round 10 修复卡

范围：Hub / 多引擎适配层，不代表 SDK 内核拆解未完成。生产代码未修改，未派发外部执行。

## R96 — P1：Pi context-overflow 压缩恢复被提前终止

状态：待修复；独立动态反例已复现。

位置：`crates/alva-engine-adapter-pi/src/mapping.rs:118–126`、`adapter.rs:181–182`。

Pi 0.85.1 的 context overflow 不属于普通自动重试分类：先 `agent_end(willRetry=false)`，后续 post-run 才发起压缩并继续运行。当前 pending error 在这个 agent_end 被升为 fatal，适配器停止读流并终止子进程，丢掉合法恢复答案。详见 ADAPTERS.md 的本机上游源码证据。

修复：按完整请求的 settlement 或等价经过验证的恢复状态判定失败，不能把普通 auto-retry=false 当作所有恢复结束。兼容已有版本协议，不可简单吞掉真实错误。

验收：正常完成、503 重试成功/耗尽、overflow 压缩成功、压缩失败/取消、恢复中 EOF、用户取消；每条仅一个 Completed，fatal 不带成功 result。

反例命令：`cargo run --offline --target-dir target --manifest-path .scratch/kernel-boundary-review-round10/evidence/adapters/Cargo.toml`。本轮正常 control 通过，恢复场景 fatal=true、answer_preserved=false，退出 101。

## R97 — P2：Hub handoff 异常 EOF 仍先开放重试再恢复身份

状态：待修复；静态并发交错确认，尚未动态复现。

位置：`crates/alva-hub-core/src/runtime.rs:977`、`:993`、`:1021`、`:1327`。

没有显式 fatal 的 `SessionStarted → EOF` 进入 mark_session_failed；它只把状态置 Failed，仍保留失败 native id。随后向消费者发送合成 Error/Completed，最后才 restore_failed_handoff。另一线程在恢复前认领下一轮时，会 resume 失败 native；旧 restore 又因 claim_seq 已更新而跳过。R93 修复覆盖显式 fatal，但没有覆盖这条合成 fatal 路径。

修复：释放可重试状态与 handoff 身份恢复必须同一临界区完成，同时保持 generation/claim 防护和合成终态的原流 native id。

验收：新增无显式 Error 的 SessionStarted→EOF 并发用例，在合成 Error 可见或状态 Failed 后立即 retry，确认 resume_session=None、handoff banner 恰好一次、旧尾部不能覆盖新轮；保留显式 fatal 与同步 execute 失败回归。
