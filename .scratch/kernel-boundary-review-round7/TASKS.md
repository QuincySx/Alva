# Round 7 修复队列

审查 HEAD：`c8aca9c8`。当前状态：卡已落盘，尚未交给原执行终端。

| 卡 | 优先级 | 来源 | 任务 |
|---|---|---|---|
| [R77](issues/R77.md) | P1 | R65 | 旧 HTTP Stop ack 不得写入新 attempt |
| [R78](issues/R78.md) | P1 | R65 | 被拒绝的重复 Run 不得清除当前 Stop marker |
| [R79](issues/R79.md) | P2 | R67 | Inspector 按 attempt+tool 定位，并真正转移焦点 |
| [R80](issues/R80.md) | P1 | R63 | cancel 能力降级与 fallback 快照仍会误伤新轮 |

建议 R80 → R77 → R78，先完善 Hub 取消再验证 HTTP 组合；R77/R78 共享 server 控制面，串行实现。R79 责任文件独立。全部新卡先补反例，再修复并记录结果。

旧卡继续使用原文件：

- [R72](../kernel-boundary-review-round6/issues/R72.md)：工作区正在修改，完成后再验收。
- [R73](../kernel-boundary-review-round6/issues/R73.md)：A 会话仍未接入首轮。
- [R74](../kernel-boundary-review-round6/issues/R74.md)：补齐缺失合规场景，quarantine 未完成项明确留 open。
- [R75](../kernel-boundary-review-round6/issues/R75.md)：测试导航与 spacing token 收尾。
- [R45](../kernel-boundary-review-round5/issues/R45.md)：原真实浏览器行为验收继续 open。

不要回退当前 dirty work；reviewer 未提交、未推送、未修改生产实现。
