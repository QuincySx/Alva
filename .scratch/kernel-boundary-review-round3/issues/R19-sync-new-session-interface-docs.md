# R19: 补齐新增会话接口及职责的分形文档

**Priority:** P3
**Axis:** Standards / R13 遗漏
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** R14–R18
**Status:** ready-for-agent

## 问题与证据

旧 Analytics 与迁出模块表已修正，但 StoreSessionCell 新公开接口没有进入 kernel-abi agent_session.rs 文件头 OUTPUT/目录 AGENTS；Agent::full_session 返回类型与 middleware 接线职责改变，上层 AGENTS 未同步。
违反 FRACTAL-DOCS.md“修改接口/职责”级联规则。不是再次重做已经完成的旧 Analytics 文档修复。R18 可能调整最终所有权，本卡以最终实现为准。

## 实现范围

核对 R14–R18 最终代码的公开输出、当前会话所有权、子会话接线、广播生命周期和模块清单；更新源码三行头、对应目录及受影响上级。删除面向临时卡号的过期说明。

## 验收

- [ ] 每个新增/变更公开接口及职责都有准确文件头与目录说明。
- [ ] 模块表真实存在且所有权没有互相冲突。
- [ ] 记录人工核对范围；不因纯内部实现变动重写无关文档。

## 执行记录

- Commit：31aa211
- 内容：StoreSessionCell / PersistentSessions 所有权与职责文档同步至 kernel-abi/agent-core 分形文档。
- 验证（2026-09-12 复跑）：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` 32 个测试目标全部 ok，655 passed / 0 failed；`scripts/ci-check-deps.sh` 通过（Rule 17/18/19 边界完好）。
- 实际验证命令及结果：`cargo +1.98.1 test --offline -p alva-agent-core -p alva-app-core -p alva-kernel-core -p alva-kernel-abi` → 32 个测试目标 ok，655 passed / 0 failed（2026-09-12 复跑）；`scripts/ci-check-deps.sh` exit 0。

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
