# 04 Crash, Repair, and Migration Soak

## 目标

证明 Hub 的持久化不变量在 packaged runtime 的崩溃、重启、旧 schema 和长时间运行中成立。

## 工作

- 在 handoff、branch switch、attempt terminal、TeamRun/Worker 更新的关键持久化相位注入退出；
- 重启后检查 orphan running repair、active attempt 唯一性和 append-only timeline；
- 用受支持的历史 schema fixture 逐级迁移到当前版本；
- 验证迁移失败时原 DB 可恢复，不留下半迁移状态；
- 执行多轮 session/branch/fallback/worker soak，检查锁、连接、文件句柄和数据库增长；
- 校验 private DB、journal、trace/blob 与 workspace 数据边界。

## 验收

- 不出现缺失 branch/handoff identity 的 active attempt；
- restart 能恢复 active、parked、pending、hibernated 对象；
- schema migration 幂等，旧 fixture 不被静默丢数据；
- soak 有明确轮次和资源上限，不用“运行过一晚”代替断言。

## 依赖

Ticket 01。

