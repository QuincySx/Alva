# 00 Freeze RC Baseline and Scope

## 目标

把“当前工作区能跑”收敛成一个可重复、可引用的 Release Candidate 基线。

## 工作

- 先形成一个干净的逻辑提交序列，再记录候选 commit；
- 固定 Rust、Node、npm、Tauri、macOS、Engine CLI 和 Provider route 版本；
- 记录 `Cargo.lock`、`package-lock.json` 与 `.app` 的 SHA-256；
- 明确 Alva、Claude、Codex、Pi 哪些是 V1 release、preview 或 excluded；
- 建立证据目录命名、secret scan 与 A/P/L/D 标签规范；
- 对照 `docs/ALVA-HUB-V1.md`、root `AGENTS.md` 与真实 UI，列出状态陈述漂移。

## 验收

- 任意开发者能从候选 commit 重建同版本 `.app`；
- release/preview Engine 范围只有一份权威表；
- 工作区脏改动、测试结果与历史 smoke 不被误算为 RC 证据；
- 文档漂移进入 Ticket 08，不在本 ticket 提前宣告 V1 accepted。

## 非目标

不提交新功能，不修改 Engine 合约，不处理 Apple 签名凭证。

