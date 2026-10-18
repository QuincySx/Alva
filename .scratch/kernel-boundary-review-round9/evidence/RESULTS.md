# 实际验证摘要（2026-09-18）

HEAD bae66d0f；以下为工具输出摘要，非完整原始日志。

## 常规测试

`cargo test --offline -p alva-hub-core -p alva-kernel-core -p alva-engine-runtime -p alva-engine-adapter-codex -p alva-engine-adapter-pi --features alva-engine-runtime/test-support --lib`

exit 101：Codex 17/17 PASS；Pi 12 PASS、1 FAIL。失败项为 `real_pi::real_pi_completes_a_turn_conformant`，事件为 SessionStarted、EOF fatal、Completed(None)。该测试只检查本机 binary 存在，模型默认依赖机器配置；本轮未区分环境与协议原因，不能单据此判 R82 修复失败。没有再次执行真实模型测试。此命令在 Pi 失败后未执行其余包。

补跑尚未执行的包：

`cargo test --offline -p alva-hub-core -p alva-kernel-core -p alva-engine-runtime --features alva-engine-runtime/test-support --lib`

exit 0，三个包全部通过（engine-runtime 56、kernel-core 68；包含 hub 新 R80/R90 测试）。已有 warning 保留，未因本次审查修改产品消除 warning。

## 独立 Hub 反例

另重跑原拆解专项：

`cargo test --offline -p alva-app-core --test r10_compaction_swap_chain --test r11_tool_lock_matrix`

exit 0，R73 的 r10 为 1/1 PASS，R72 的 r11 为 7/7 PASS（包含真实 child 同资源互斥、异资源/读读重叠）。本轮没有再次执行实现者记录的全局串行化变异，不将他人的历史变异记录冒充本轮执行。

见 [hub/RESULTS](hub/RESULTS.md)，两项均编译成功后断言失败 exit 101：

- cancel_claim_gap：新轮 Starting 被旧取消置为 Failed。
- handoff_native：banner 一次，但 retry native=Some("native-0")，期望 None。

## 独立适配器反例

`cargo run --offline --manifest-path .scratch/kernel-boundary-review-round9/evidence/adapters_extra/Cargo.toml --target-dir target`

编译成功，exit 101。真实 adapter.execute + 本地假 CLI，不连接真实服务。

```text
codex: invalid_utf8=false, fatal=false, has_result=true
pi: invalid_utf8=false, fatal=false, has_result=true
codex: invalid_utf8=true, fatal=true, has_result=true
pi: invalid_utf8=true, fatal=true, has_result=true
pi: recoverable_retry_lost=true
assert left=(2, true), right=(0, false)
```

两个 healthy controls 成功，三个边界样本暴露问题。总计独立动态反例 5 个（Hub 2 + Adapter 3），分成 4 张修复卡。
