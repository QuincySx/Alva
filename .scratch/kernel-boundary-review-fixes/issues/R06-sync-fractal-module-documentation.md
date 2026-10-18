# R06: 补齐重构后的分形文档与模块所有权地图

**Priority:** P2
**Axis:** Standards / 明确规范违例
**Origin:** 原卡 09、12–19；2026-09-09 review
**Blocked by:** R01, R02, R03, R04, R05, R07, R08（最终一致性检查；各前置卡自身仍必须同步文档）
**Status:** archived-reviewed

> 2026-09-10：本卡已有实现并复审，停止按本卡重新开工。当前待办见 [R09–R13](../../kernel-boundary-review-round2/TASKS.md)；归档不代表全部验收通过。

## 问题与规范

FRACTAL-DOCS.md 要求源码目录有 AGENTS.md；新增/删除文件以及接口/职责变化须级联更新清单。
这轮只改了根 AGENTS.md：新 `alva-agent-abi` 缺目录文档，agent-core 没登记迁入的
agent_session/tool_lock，kernel-core 的文档仍描述已迁出的实现。根文档还残留旧 Phase 路径等描述。

## 实现范围

- 按 `git diff 5800395...HEAD --name-status` 核对实际新增、迁移、删除的源码模块。
- 补 `crates/alva-agent-abi` 及其源码目录地图；同步 agent-core、kernel-core、kernel-abi、agent-context、app-core、hub-desktop 受影响的目录清单。
- 核查源码 INPUT/OUTPUT/POS 注释的直接依赖、公开出口、职责；不复制历史路径。
- 更新根 AGENTS.md 中层次、模块所属层、Phase/会话/锁/分析埋点及新 Hub 模块的准确描述。
- 保留现有 ADR 决策，不利用文档整理扩大实现范围；描述最终落地事实，不把验收缺口写成已完成。

## 验收

- [ ] 新增/迁移模块均可从上级地图导航到真实路径，旧目录不再登记已搬走的文件。
- [ ] 新目录 AGENTS.md 遵循项目五段结构；受影响源文件三行注释准确。
- [ ] 文档描述与卡 R01–R05、R07–R08 完成后的实际接线一致。
- [ ] Markdown 本地链接存在，无失效模块名或错误层级归属。
- [ ] 本卡仅修改文档/注释，无行为改动；记录文档核查结果，无需为文案编写行为测试。

## 通用约束

遵守根 AGENTS.md、对应目录 AGENTS.md 与 FRACTAL-DOCS.md。公开表面变化同步更新相关快照；每项逻辑修复单独提交，记录实际运行的检查和结果，不把未运行的检查标为通过。
