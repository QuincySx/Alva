# 10 — 打包、视觉与三引擎验收

**Status:** planned

## Goal

以真实打包的 `ALVA Hub.app` 验收 chat-first 产品，而不是只验证开发环境组件。

## Scope

- 删除被新聊天流取代的旧 dashboard panel 和死状态；
- 完成 light/dark、空状态、长文本、代码块、小窗口、键盘导航和可访问性检查；
- 验证 Alva、Claude、Codex 的运行、切换、fallback、审批、恢复和兼容性提示；
- 验证 provider 添加、gateway 生命周期和错误恢复；
- 对照旧 Tauri 产品完成最终功能与视觉 parity checklist；
- 所有验收通过后删除 `crates/alva-app-tauri/` 和 `crates/alva-hub-desktop/`；
- 更新架构、GPUI 迁移和用户使用文档。

## Acceptance

- `cargo check -p alva-hub-gpui`、相关 crate tests、依赖边界检查通过；
- 打包应用冷启动、重启恢复和退出清理正常；
- 三引擎验收矩阵有可复现结果；
- 主界面只保留 chat-first 信息架构，旧运维仪表盘不再作为用户入口。
- 旧 Tauri 目录仅在 parity checklist 全绿后删除，且 Cargo、CI、文档无残留引用。

**Blocked by:** 04 — 把运行操作内联进聊天流; 08 — 嵌入协议转换 Gateway; 09 — TeamRun / Worker 协作内联.
