# Hub private store
> SQLite 产品状态与耐久记录；不依赖 UI，不扩展 Kernel。

## 地位
L7 Hub Desktop/Server 共享持久化边界。

## 逻辑
`mod.rs` 管连接与公共 DTO；领域簇使用事务更新，`db.rs` 统一 schema migration 与启动恢复。

## 约束
- schema 19 增加工具全文 metadata/chunks；schema 20 增加 assistant 可见文本消息日志，旧摘要不伪装全文。
- schema 21 是本地 interval 自动化，最多 100 配置，title 120 字符、prompt 16 KiB、interval 5–10080 分钟。创建/编辑暂停；enable 核对既有 branch/EngineTarget，普通会话专用。认领与 Turn 原子写入，busy/missed 不排队不补跑；失败/Stop/路由变化暂停，保存期间不会执行。删除仅配置，不删 session/history/files。
- 重启时运行中的 schedule 标 interrupted 并暂停；未运行但 enabled 的 schedule 等完整 interval，绝不立即消费漏过的 tick。运行期间 Pause 不取消当前 Turn（用户去 session Stop），成功终态不得复活其 enabled 状态。
- 消息按 attempt/原始 item/UTF-16 offset 在发送 live 前事务保存，单片段 16 KiB UTF-8、每页 128 记录；页以固定 through 高水位有界补读，游标边界一条 at-least-once 重放以观察 crash-repair 终态。日志属于私有对话，不含 hidden reasoning，也不自动进入跨引擎 handoff。
- `record_tool_side_effect` 先整体脱敏，再将完整正文、摘要和引用原子提交；immutable content ID 保证已发送引用不被改写。
- 每块最多 16 KiB 且是合法 UTF-8；正文最多 64 MiB，超限明确失败，不截断成成功。
- 读取用 product_session_id JOIN engine_attempts 校验归属，一次只读指定块；不接收文件路径，不读取宿主文件。
- 时间线/session detail 只含摘要与引用，不带大段全文；进程重启仍可读取。

## 业务域清单
| 文件 | 职责 |
|---|---|
| mod.rs | HubStore 连接、公共类型与共享 helper |
| db.rs | 私有文件权限、schema 迁移、实例锁及中断恢复 |
| turn.rs | 会话/Turn/Attempt 与 side-effect evidence 合并事务 |
| tool_content.rs | 脱敏全文分块、原子引用与按会话授权读取，含迁移/Unicode/越权回归 |
| message_journal.rs | 完整可见 assistant 文本、游标分页、会话隔离与重启终态，发送前持久化失败不可声称成功 |
| automation.rs | 暂停优先 interval CRUD/revision、固定普通 session/branch/route、原子 claim 与完成/失败/Stop 结算、重启不补跑及协议/运行替身回归 |
| branch.rs | EngineBranch、切换与 fallback policy |
| handoff.rs | Handoff 构造、保存与过滤 |
| team.rs | 团队、worker 生命周期与队列 |
| orchestration.rs | 队列领取的纯策略 |
| projects.rs | 项目登记与会话关联 |
| tests.rs | 产品持久化与恢复测试 |
