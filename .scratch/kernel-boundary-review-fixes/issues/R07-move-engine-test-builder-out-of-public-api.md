# R07: 测试构造便利函数移出生产公开 API

**Priority:** P3
**Axis:** Standards / 已接受的设计整理建议（非行为故障）
**Origin:** 原卡 17；2026-09-09 review
**Blocked by:** 无
**Status:** archived-reviewed

> 2026-09-10：本卡已有实现并复审，停止按本卡重新开工。当前待办见 [R09–R13](../../kernel-boundary-review-round2/TASKS.md)；归档不代表全部验收通过。

## 问题

`crates/alva-hub-desktop/src/engine_registry.rs` 公开了 `engine_registry_for_test_build`，
硬编码 WorkspaceWrite、空 provider、临时 workspace/history；唯一消费者是 readiness 集成测试，
且函数经 lib.rs 对生产调用者导出。测试便利性不应另建需要维护的生产装配入口。

## 实现范围

- 将测试默认参数构造移到 `tests/readiness_tests.rs` 的私有辅助函数，移除生产定义与 re-export。
- 第五引擎测试优先走已有正式 `build_engine_for_target` 等装配入口，验证权限协商/注册表构造，而非复制生产实现。
- 保持现有 EngineBlueprint 注册和真实构造能力不变，不新增更泛化的公开测试入口。

## 验收

- [ ] 生产源码/公开导出不再有 engine_registry_for_test_build。
- [ ] 第五引擎 readiness→权限协商→构造的原行为断言仍通过。
- [ ] 无新公开 helper、无重复实现的引擎装配逻辑。
- [ ] readiness 集成测试与 Hub 现有构造相关测试通过；无需新增镜像式测试。

## 通用约束

遵守根 AGENTS.md、对应目录 AGENTS.md 与 FRACTAL-DOCS.md。公开表面变化同步更新相关快照；每项逻辑修复单独提交，记录实际运行的检查和结果，不把未运行的检查标为通过。
