# Round 8 evidence
> 可运行反例和明确边界的静态检查。

## 地位
测试用假数据，不读取真实会话/凭据。
## 逻辑
adapter probe 运行真实 execute；UI probe 运行真实 App；server witness 只运行分类器和源码顺序检查。
## 约束
不得将源码 witness 说成 HTTP/Tauri 端到端通过；反例失败是待修证据。
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Adapter | adapters/ | 成功误判与 Normal 漏检 |
| UI | ui-session-races.test.tsx / ui-session-races.txt | 受控 Promise 导航/刷新反例 |
| Server | server-control-probe.mjs | 当前 Rust 故障分类器与清理顺序 witness |
| R72 构建 | r72-partial-move.rs | test cfg 部分移动后借用的 E0382 最小反例 |
