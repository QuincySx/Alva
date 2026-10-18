# Hub engine seam 18卡复审

范围：用户要求所有agent实现，核对当前 `5c64af1`，不是只看本轮diff。原清单多数done与单卡TODO/未勾验收并存。以下静态认可不等价全量/真机复跑。GPUI按已退役路线核对遗留，不恢复其产品权威。

## 主要发现

- **P1 / seam12，R49：** runtime.rs先转发Completed后在循环外补ToolEnd；EOF路径修复了，带Completed的未闭合工具路径仍不符合终态收尾顺序。正式产品run.rs在Completed处收尾，可能丢后续工具关闭。
- **P1 / seam08/10，R50：** HubRuntime.cancel live RunHandle分支绕过capabilities.cancel检查，无法取消的引擎发SessionStarted后，Hub的abandon后备路径不再执行，claim留Running。
- **P1 / seam10/13，R51：** HTTP Stop先record_attempt_cancelled再await engine.cancel。失败仍已记账，ack成功但进程未终止也立即Interrupted；server仍是独立active_engines控制面，未统一RunHandle。
- **P1 / seam07/13，R58：** run表尾部无代际remove；旧run完成后新run已登记，旧尾部可删新handle。spawn先于insert也有快结束残留窗口。静态竞态证据，未宣称动态复现。
- **P2 / seam16，R52：** conformance未完成所有adapter×五剧本，且三个scenario断言缺强度；quarantine/阈值/解除未实现且无新ADR决定，不能整卡done。不是要求本次审查擅自选择策略或扩实现。

## 逐卡矩阵

| 卡 | 当前结论 | 对应实现/证据与界限 |
|---|---|---|
| 01 Codex/Pi fatal停流 | 静态认可 | 两adapter生产is_fatal_error后break；stops_reading_after_fatal_error和事件契约断言存在。未重新运行所有CLI fixture。 |
| 02 Alva早退终态 | 静态认可 | adapter terminal辅助路径和session删除/agent channel early-close回归存在；不把本轮nested通过当其专项通过。 |
| 03 审批结果真实性 | 主修复保留，GUI未全验收 | Alva resolve结果被消费；permissions/respond_many失败不加入answered；Claude语义ack见18；真实过期UI矩阵仍R45。 |
| 04 GPUI移植 | 退役路径静态保留 | fallback消费HUB_ENGINE_IDS，team调用真实interrupt入口；不在本轮重建GPUI或宣称人工测试。 |
| 05 Server句柄清理 | 主修复静态认可 | main.rs:763 RunEngineLease通过Drop清理handles/router，覆盖handler取消；Stop时序另见R51。 |
| 06 能力词表 | 主修复静态认可 | 采用白名单/引擎词过滤，capability_incompatible_target_is_skipped_with_reason存在；不把“model词不能当engine词”当新第五引擎限制。 |
| 07 claim序列 | session修复保留，control表遗漏 | session尾写校验my_seq，stale_turn_tail测试存在；RunHandle remove无代际，R58。 |
| 08 abandon出口 | 部分完成 | public abandon取消token并abort任务；未知native id释放测试存在；已知id且cancel不支持漏后备R50。旧abandon_stops测试只sleep无退出断言，不能完整证明终态语义。 |
| 09 restore状态 | 运行时约束认可；原类型级验收未兑现 | restore_session:397拒绝非Completed/Failed且测试存在；仍接受HubSessionStatus参数，没有原卡要求RestoredStatus类型编译期证明。应明确接受运行时方案或补类型边界，不声称类型闸已通过。 |
| 10 三档取消 | 部分完成 | Claude Full已有grace超时kill和终态测试；Hub live capability漏检R50，HTTP先成功记账R51；全产品真机终止矩阵未完成。 |
| 11 resume自愈 | 主修复静态认可 | rejected_resume_clears_stored_engine_session_id、real_alva_resume_failure_text_triggers_upgrade存在，Codex无thread_id不生成假id；本轮未复跑产品重启链。 |
| 12 未闭合工具 | 确认缺陷 | 只EOF专项覆盖；带终态后置ToolEnd，R49。 |
| 13 RunHandle | 部分完成 | strong engine/abort/native slot存在，unregister后cancel回归存在；登记/清理代际R58，server影子表R51；原记录自己承认产品增量未全却总表done。 |
| 14 switch权威 | 认可已记录决策 | ADR0002权威为store/branch，core为瞬时dispatcher；保留CLI单槽便利是明示边界，不重报scope creep；无resume引擎fresh回归存在。 |
| 15 descriptor | 主边界保留，验收未全 | 产品blueprint注册与构造已由后续kernel17推进；测试仍复刻正式build路径见HISTORICAL-CORE。原“协商全上移/旧文件消失/≤40字面量”等未逐项证明；能力位真实消费的完成口径需核对，不以名称搜索替代行为。 |
| 16 conformance/quarantine | 未完成 | Pi/ClaudePrint五剧本存在；Codex2 harness+旧边界断言，Alva1 harness，Full未接统一harness。Killed/EOF仅检查Completed(None)，OutputAfterFatal若无fatal也通过；quarantine代码/ADR缺失，R52。 |
| 17 Claude fatal怀疑 | 保留非bug关闭结论 | 读取循环收到Completed即break，mapper fatal与Completed成对；不因“零harness”反推生产一定违约（测试缺口另R52）。 |
| 18 Claude权限语义ack | 主修复静态认可，真机未验收 | pending_responses先登记、accepted/rejected解析、超时均存在；late_permission_response和ack_timeout测试可定位。bridge仍保留config缺省60_000，真实Rust已下发；不把备用默认值独立判生产bug。 |

## 验证分层

新增Hub公开API探针覆盖R49/R50，输出见evidence/hub-probes.txt；与static-only R51/R58明确分开。没有调用真实外部引擎或触及用户会话数据。其余项目读取实现和现有测试，不宣称18卡全套历史验收重跑。

对于范围选择/指标/原卡未做的验收，沿R46纠正历史完成口径并显式建open待办；不要求为满足旧机械指标盲目删除正确结构。
