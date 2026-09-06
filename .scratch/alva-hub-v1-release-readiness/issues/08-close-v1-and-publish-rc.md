# 08 Close V1 and Publish Release Candidate

## 目标

把代码、证据、规范和发布物收敛成同一个可审计的 V1 结论。

## 工作

- 更新 `docs/ALVA-HUB-V1.md` 的 Current State 与 status，移除已经失真的“尚未实现”描述；
- 逐条更新 `crates/alva-hub-desktop/ACCEPTANCE.md`，链接 A/P/L/D 证据；
- 更新 root/目录 `AGENTS.md` 中 Engine 范围和发布状态，仅记录已接受事实；
- 生成 release notes、known limitations、artifact checksums、数据路径和 rollback；
- 记录 deferred advisories、preview Engine 和 external gate；
- 建立 RC tag/commit 与证据目录的双向索引。

## 验收

- Gate G0–G6 全部通过或有明确阻止发布的 external gate；
- 18 个 V1 acceptance scenarios 不再依赖含糊的“已有测试”描述；
- 正式文档不把 preview 能力写成 release 承诺；
- 发布物可验证、可安装、可升级、可回滚；
- 完成后才把 V1 标记为 accepted implementation。

## 依赖

Tickets 03–07。

