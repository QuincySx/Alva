# Adapter success regression

2026-09-17；产品 HEAD c8aca9c8；独立 probe 使用本目录 Cargo.lock，workspace 常规测试使用根锁文件。

实际运行：

`cargo test --offline -p alva-engine-adapter-codex -p alva-engine-adapter-pi normal_run_conforms -- --nocapture`

结果：Codex 1/1 PASS，Pi 1/1 PASS，exit 0。

独立检查：

`cargo run --offline --manifest-path .scratch/kernel-boundary-review-round8/evidence/adapters/Cargo.toml --target-dir target`

结果：exit 101（预期反例失败）；两个真实 adapter 的 execute 都把完整成功 fixture 判成失败：

```text
codex: fatal=true, answer_preserved=false, normal_harness_accepts=true
Error { message: "codex exited without a terminal event", recoverable: false }
Completed { session_id: "review-codex", result: None, usage: input=1/output=2/num_turns=1 }

pi: fatal=true, answer_preserved=false, normal_harness_accepts=true
Error { message: "pi exited without a terminal event", recoverable: false }
Completed { session_id: "review-pi", result: None }

assertion failed: successful adapter runs must preserve success
left: 2
right: 0
```

上面是工具输出的精简摘录，不是完整 stdout。两者之前都已产生包含 expected successful answer 的 Message。fixture 不连接网络、不使用真实 CLI 或凭据。

R81/R82 修复成功后本 probe 应 exit 0。R83 另须增加“给 Normal 失败流必须拒绝”的负对照，避免成功路径修复掩盖 harness 缺陷。
