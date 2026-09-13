# 本轮验证证据

审查源码HEAD `07b75f8`。测试使用当前源码及已有依赖，未安装包、未改生产实现。探针只临时放到对应测试目录编译/运行，结束后移回本目录，生产测试目录无残留探针。

| 文件 | 本轮结果 | 证明范围 |
|---|---|---|
| core-probes.txt | 7项：2通过/5失败 | 公开会话API及真实AgentBuilder装配键；不是完整root→grandchild持久化 |
| chat-probe.txt | 1通过、25原用例跳过 | 完成历史重新打开仍可看到回答的控制测试 |
| chat-live-probe.txt | 1失败、25原用例跳过 | 同一App看到正文后注入live completed，正文消失 |
| existing-web-tests.txt | 6文件70测试通过 | 原有前端suite，受控bridge/组件测试 |
| existing-core-regressions.txt | 3通过 | 既有广播和订阅回归，与新增探针对照 |
| review_round4_probes.rs | 7个独立探针源码 | gate/abort/idle driver/clear代际及typed key |
| review-round4.test.tsx | 原集成fixture+单个专项用例 | 含历史正文控制断言和live completed后断言 |

## 复跑

从repo根操作，先确认目标文件不存在且不覆盖其他执行者：

1. 将 `review_round4_probes.rs` 复制为 `crates/alva-agent-core/tests/review_round4_probes.rs`；运行 `cargo +1.98.1 test --offline -p alva-agent-core --test review_round4_probes`。
2. 将 `review-round4.test.tsx` 复制为 `crates/alva-hub-desktop/web/src/review-round4.test.tsx`；在web目录运行 `npm run test -- --run src/review-round4.test.tsx -t 'round4 completed' --maxWorkers=1 --minWorkers=1`。此文件仅用于vitest运行，不纳入产品构建；来自现有fixture的无关未使用定义可在正式落回归时精简。
3. 历史恢复控制测试使用同一fixture，删除末尾live callback及第二条正文断言；本轮先执行该控制并通过，再增加terminal事件断言复现失败。
4. 正式修复时把针对性测试整合到已有模块，补适用源码头/目录说明；不要把整份fixture复制当成长期测试组织。

本轮既有测试命令：
- `cargo +1.98.1 test --offline -p alva-agent-core --test review_broadcast_regression --test review_subscription_probe`
- web目录 `npm run test -- --maxWorkers=1 --minWorkers=1`

## 解释与限制

- 超时200/100ms只用于有界判定；探针先等监听进入gate或已消费首批，再制造关键交错，当前线程runtime保证调度可控。取消用例真实abort并检查JoinError.is_cancelled。
- 装配键探针构造真实AgentBuilder，核对tool_batch当前消费的精确键；源码进一步证明实际工具读端断线，但不声称已运行持久化递归链。
- React使用真实App与受控bridge；不是demo截图，也不是最新Tauri/store/engine端到端。
- 未调用真实外部模型、未改用户会话/权限/账户设置。
- 旧第三轮日志与完成记录属于审查对象，不能作为本轮全部验收通过的依据。
