# 第五轮验证证据

固定源码 `5c64af1`。已有依赖上运行，不更新视觉基线。只写临时测试并归档到本目录，生产测试目录无残留。

| 检查 | 本轮结果 | 证明范围 |
|---|---|---|
| web `npm test` | 6 files / 93 passed | 原组件、App、i18n suite |
| web `npm run test:visual` | 23 passed，34.9s | demo bridge行为11项+视觉12项，不是真实engine |
| `review-round5.test.tsx` | 2 failed | Markdown正文双转义、同item工具终态被旧message覆盖 |

UI复跑：确认目标不存在后将本目录 `review-round5.test.tsx` 暂放 `crates/alva-hub-desktop/web/src/`；在 web 运行 `npm test -- src/review-round5.test.tsx`，归档输出并移回。失败是本轮发现的缺陷，不是测试环境故障。没有读取系统剪贴板或任何凭证。

既有suite的结果由本轮工具输出记录；未复制为旧执行者日志。真正动态核心探针及专项cargo结果另列于本目录。

## 核心和 Hub 动态结果

| 检查 | 本轮实际结果 | 证据 |
|---|---|---|
| 广播回归+订阅 | 12/12 + 1/1 PASS，exit0 | core-probe/broadcast-result.txt |
| 真实三级递归持久化/重开 | 1/1 PASS，exit0 | core-probe/r24-result.txt |
| 确定性swap gate | 目标1/1 PASS；记录只保证该目标，不冒称所有过滤测试执行完成 | core-probe/r24-result.txt |
| 弃置会话driver退出 | FAIL，仍1个task，exit101 | core-probe/drop-result.txt |
| 非Tokio重入追加 | FAIL，完成界限1秒超时，exit101 | core-probe/reentrant-result.txt |
| Hub工具终态顺序 | FAIL，Completed索引3，ToolEnd索引4 | hub-probes.txt |
| Hub无cancel能力释放 | FAIL，Unsupported(cancel)，状态Running | hub-probes.txt |
| 最小ConversationLog | 1/1 PASS，定向命令exit0 | HISTORICAL-CORE记录本轮输出结论 |
| 工具批中间件 | 报告7/7；两项漏await，只有另外5项实际运行 | HISTORICAL-CORE及R60源码定位 |
| 公开表面scanner | 字段/变体/trait改变前后结果相同；async公开函数无输出 | HISTORICAL-CORE动态scan_scope对照 |

Hub复跑：确认不存在后，将 `review_round5.rs` 暂放 `crates/alva-hub-core/tests/`，运行 `cargo test --offline -p alva-hub-core --test review_round5 -- --nocapture`；完成后移回。本轮这两个测试使用纯进程内fixture，没有启动外部引擎。非Tokio/drop探针独立Cargo包与命令见core-probe/README。

本轮未完整重跑Rust workspace、wasm、签名包或真实引擎桌面验收。独立探针6个失败证明6个场景；不能推断其余所有路径正确或错误。
