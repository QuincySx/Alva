# Round 9 evidence
> 受控假引擎/CLI 的真实 API 验证。

## 地位
不读取真实用户数据，不改生产源码。
## 逻辑
hub 用 gate 固定交错；adapters_extra 用有效 JSONL 与非法 UTF8 尾部验证终态一致性。
## 约束
反例失败不冒充测试通过；每个结果记录实际命令和退出状态；环境依赖测试单独报告。
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Hub | hub/ | 新 claim 与旧 RunHandle 的取消窗口、handoff 重试 |
| Adapter | adapters_extra/ | 正常终态之后读取错误不能产生 fatal+成功答案 |
| 验证摘要 | RESULTS.md | 实际命令与结果 |
