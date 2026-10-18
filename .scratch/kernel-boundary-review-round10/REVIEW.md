# 昨晚至今早实现质量复审

基线 `bae66d0f`，HEAD `764864da`，六个提交：R92/R93/R94/R95 与 R72/R74 补项。只审查和保存证据，未改产品代码。

## Standards

本次规范轴无新增问题。R72 execution_mode 文档已与 Coordinator 并行候选规则一致。未发现必须为架构边界或代码气味新增卡的问题。

## Spec

- R92 claim-to-registration 窗口已通过同锁 claim/generation 校验修复。
- R93 显式 fatal 的 handoff 身份恢复已修，但合成 fatal 的异常 EOF 仍有重试窗口：见 R97（P2，静态交错证据，未动态复现）。
- R94 trailing transport failure 的终态一致性修复符合原卡；当前仍读取至 EOF，不能误报“正常终态立即杀进程”。
- R95 普通 auto retry 已修；context-overflow 的 compaction 恢复仍被截断：见 R96（P1，动态复现）。
- R74 新测试静态有效：Alva 模型流永久 Pending 后取消，不依赖放行；Claude 真正产生 fatal 后帧。此次未重新执行这两个包，不宣称动态验收通过。

## 验证

离线执行 Hub core、engine runtime、Codex adapter、Pi adapter lib tests，启用 engine-runtime/test-support，跳过 real_pi：50 + 56 + 19 + 19 = 144 passed，0 failed；Pi 两项被过滤。真实订阅、UI、全仓及 Alva/Claude 包测试不在本轮动态结论内。

独立 Pi compaction 反例：正常对照 fatal=false/answer=true；恢复场景 fatal=true/answer=false，退出 101。使用本地假进程、真实适配器，无模型网络调用。

上一轮三个独立探针已重跑，均退出 0：adapters_extra（正常对照、Codex/Pi malformed tail、Pi 普通重试）；cancel_claim_gap（新 claim 保持 Starting）；handoff_native（retry native=None、banner 一次）。因此新卡是遗漏分支，不是宣称原反例仍失败。

总结：Standards 0 项；Spec 2 项，最严重为 R96 P1。修复确实改善了原分支，但尚不能整体验收关闭。
