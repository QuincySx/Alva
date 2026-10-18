# Core review evidence
> Round 6 核心复审的独立进程内回归探针。

## 地位
只属于 scratch 审查证据，不是产品代码或 Cargo 根 workspace 成员。

## 逻辑
从根 Cargo.lock 派生依赖锁，path 依赖审查 HEAD；使用真实 Session/Hub API 和 fake engine gate 重现。

## 约束
不访问真实外部 engine、用户 session/provider 数据或网络；每个 probe 的失败是待修复契约断言。

## 业务域清单
| 文件 | 职责 |
|---|---|
| Cargo.toml / Cargo.lock | 独立构建入口和锁定依赖 |
| cross_session.rs / .log | 非 Tokio A callback append B 丢实时投递 |
| tokio_delivery.rs / .log | Tokio A listener await 时 B driver 投递停滞 |
| stale_cancel.rs / .log | 旧 cancel 的迟到 Unsupported 误弃置新轮 |
| late_registration.rs / .log | 旧 Starting 轮迟到登记覆盖新轮 RunHandle |
| core-tests.log | 现有核心单元测试命令输出 |
