# 00 — 冻结旧 Tauri 对照并建立迁移清单

**Status:** completed

## Goal

保留旧 Tauri 产品作为只读功能与视觉对照，明确两套 GPUI 产品还缺什么；此阶段不删除旧目录。

## Scope

- `crates/alva-app-tauri/` 和 `crates/alva-hub-desktop/` 保持退出 Cargo workspace；
- 禁止继续向旧 Tauri 代码添加功能，只允许读取和对照；
- 建立 Alva App 与 Hub 的页面、组件、状态、交互、打包能力对照表；
- 标记每项为 migrated、partial、missing、obsolete；
- 明确 `alva-hub-gpui` 是目标产品入口，`alva-app-gpui` 是单引擎 GPUI 参考实现。

## Acceptance

- 有可审查的功能与视觉 parity checklist；
- 每个未迁移能力都有后续 ticket 或明确 obsolete 决策；
- 旧 Tauri 目录没有新增产品实现；
- 删除旧目录明确延后到 ticket 10。

## Result

审计结果记录在 `../PARITY-CHECKLIST.md`。结论是功能迁移大体完成，但两套 GPUI UI 的组件库与视觉迁移仍未完成，旧目录继续保留。

**Blocked by:** None.
