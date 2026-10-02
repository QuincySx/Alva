# Hub 工作台重整

## 地位
用户 2026-09-20 产品方向对应的规划、逐卡执行和验收证据目录；正式产品验收见 `crates/alva-hub-desktop/ACCEPTANCE.md`。

## 逻辑
现状审计→规范对齐→主流程→工具接线→真实验收；Kernel 单独检查。

## 约束
不得将待确认产品决策当作已批准；不复制旧缺陷编号；保留原证据。

## 业务域清单
| 文件 | 职责 |
|---|---|
| TASKS.md | 21 张 Hub 工作卡与 2 张条件化 Kernel 卡，含中央展示性能门槛 |
| HANDOFF.md | 可直接交给执行 Agent 的指令 |
| EXECUTION.md | 逐卡历史与接手修正证据；最新修正优先于历史完成标记 |
| native-before-worktree-fix.png | 隔离真实 Tauri 的部分读取证据，早于 worktree IPC 修复，不是最终 UI 验收截图 |
