# Round 7 增量复审（2026-09-16）

本轮检查 9 月 15 日的实现及 9 月 16 日补修后的当前状态，固定 HEAD 为 `c8aca9c8704cabdcd66655213cd4c4b998e4c65a`。沿用 round6 的旧卡与证据，重点复查取消控制面、HTTP Stop、Inspector 定位及未完验收，不声称重新验收全部历史卡。

## 交付

见 [TASKS](TASKS.md)。新增 R77–R80 共四卡（3 个 P1、1 个 P2），包含精确责任路径、错误交错与有限验收；旧 R72/R73/R74/R75 和 R45 保持待验，不复制卡。

原 agent 的终端尚未识别，**未投递、未声称已启动修复**。Orca status 返回 app.running=false、runtime unreachable；已向用户请求原终端/会话标识。仓库存在他人未提交的 R72 修改（agent_spawn.rs、r11_tool_lock_matrix.rs、tool_batch.rs），本 reviewer 未改动这些产品文件。

## 验证边界

- 实际运行 `python3 scripts/public-surface-regressions.py`：7/7 PASS。
- 实际运行 web 的 `./node_modules/.bin/vitest run src/components/workbench-tools.test.tsx src/components/hub-timeline.test.tsx`：19/19 PASS，有现存 act 与 controlled/uncontrolled warnings。
- 新增 HTTP/定位问题来自静态路径和确定性交错分析，本轮没有新增并运行它们的反例。测试绿不代表已覆盖这些场景。
- R66 同步失败回收、R68/R69/R76 在本次检查范围未发现新问题，不等同于全部产品验收完成。
- R73 的 session_a 仍未安装到实际首轮；R74 的 Alva KilledMidRun/Claude Full 合规矩阵仍缺；R75 的测试导航及聊天 padding literal 仍在。R72 是进行中的 dirty work，不盖完成章。
- 待后续验证：Stop ack 后遇到 Crash/Transport fatal 与授权 fallback 的组合，是否还启动下一引擎。纯 EOF 的合成错误归 Other，不能把它误写为 Transport。此疑点尚未立修复卡。

## 执行交接

收到本目录后先读 TASKS 和相应卡；先完成手头 R72 的一致实现，保留他人修改，然后按卡有限验收逐项修复。每个逻辑修改独立提交并记录命令与真实结果；不得仅更新截图基线或将既有绿测当作新反例已通过。
