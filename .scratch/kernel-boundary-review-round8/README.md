# 2026-09-17 扩大复审

本轮新增 **11 张卡 R81–R91**，加上仍待修/待验的 9 张，共 [20 张夜间队列](TASKS.md)。[给实现 agent 的入口](HANDOFF.md)。新卡 8 个 P1、3 个 P2；不重复为 R72 的编译阻断另开卡。

## 最重要的结果

Codex/Pi 的正常完成被当作异常 EOF，最终答案清空。独立 fixture 通过真实 adapter.execute 已复现两个失败，而现有 Normal harness 仍把两个失败流判为合规。这解释了为什么只看“测试绿”不能证明修复完成；R81/R82 修产品，R83 修验收。

另外确认 Desktop 重复 Run/休眠失败清理会删除活控制面、取消失败会丢审批、Stop 强杀可能自动 fallback、Desktop Stop 过早宣布退出，以及两条 UI 异步串会话问题。handoff 失败后的重试时序单列 R90。

## 范围与验证边界

- 产品 HEAD 仍为 `c8aca9c8704cabdcd66655213cd4c4b998e4c65a`；从上轮至本轮无新提交。
- 生产 dirty 文件仍为 agent_spawn.rs、r11_tool_lock_matrix.rs、tool_batch.rs，reviewer 未修改。
- 本轮实际执行 Codex/Pi 的既有 Normal 测试：2/2 PASS。
- [独立 adapter probe](evidence/adapters/RESULTS.md)：2 个成功路径反例失败（exit 101），两者 fatal=true、answer_preserved=false、normal_harness_accepts=true。
- [真实 App probe](UI.md)：2 个受控 Promise 反例失败；无后端/真实浏览器声明。
- [server witness](evidence/server-control-probe.mjs)：当前 Rust 分类器实际编译执行为 Crash/可 fallback；其他控制面问题是静态顺序确认，仍需实现者补集成测试。
- R72 E0382 是最小 Rust 同形探针实际编译失败；不是整包构建已执行的声明。
- R90 静态确认；动态 probe 未完成，未运行 Cargo，无遗留构建进程。

本轮未修生产代码、未提交或推送，也未把任务投递给未识别的终端。新增的是本地复审材料、可复现证据和明确验收卡；HANDOFF 可直接交给原实现 agent。
