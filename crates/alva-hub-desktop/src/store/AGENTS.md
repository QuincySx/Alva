# Hub private store
> SQLite 产品状态与耐久记录；不依赖 UI，不扩展 Kernel。

## 地位
L7 Hub Desktop/Server 共享持久化边界。

## 逻辑
`mod.rs` 管连接与公共 DTO；领域簇使用事务更新，`db.rs` 统一 schema migration 与启动恢复。

## 约束
- schema 19 增加工具全文 metadata/chunks；schema 20 增加 assistant 可见文本消息日志，旧摘要不伪装全文。
- schema 21 是本地 interval 自动化，最多 100 配置，title 120 字符、prompt 16 KiB、interval 5–10080 分钟。创建/编辑暂停；enable 核对既有 branch/EngineTarget，普通会话专用。认领与 Turn 原子写入，busy/missed 不排队不补跑；失败/Stop/路由变化暂停，保存期间不会执行。删除仅配置，不删 session/history/files。
- schema 22 保存 Hub 全局 ALVA 能力/MCP 与 revision；只开放真实 catalog 的 10 个能力开关，安全 substrate 不可关闭。最多 32 个 stdio MCP，绝对程序路径/逐项 argv、timeout 1–60 秒；create/update 重新禁用，enable 需确认宿主访问。CRUD 绝不 spawn，不导入 env/header 或外部配置；其他 engine 不消费。
- schema 23 保存 Turn 附件原始 BLOB、相对路径、签名类型和处理清单；单文件 20 MiB、8 文件/总 40 MiB，base64 解码前后校验。新副本写入 session cwd 的 `.alva/sessions/<id>/attachments/<id>/<name>`，逐级 no-follow/create-new、0700/0600；禁止宿主输入路径/受保护名，不覆盖现有文件。失败可能留下仅本次创建的副本/目录，不隐式清理。
- schema 24 独立保存派生媒体 BLOB/新文件与 native-root/tool-call 绑定的投递 receipt。原始上传记录不可覆盖；每附件最多 64 产物，JPEG 256 KiB/video 8 MiB。选择最多 2 图片，快照 source/output hash/实际帧时间；提交前原子改变状态，成功响应不等于理解。未投递/未知中断不自动重放，run end/reopen 结算遗留状态；派生失败可留仅本次文件/空目录，不伪造完整记录。
- `context_history` 复用现有 Turn/message_journal/tool_contents/失败原因，固定 HubSession、不扫 Worker/全局。逻辑消息跨片段重组并核验 UTF-16 offset（至多 65536 片段），工具跨 16 KiB 块匹配；legacy_assistant 明示旧摘要。5 来源高水位分页、128 逻辑记录/64 MiB 扫描、3 命中/100-byte preview、512-byte read/4 KiB response，文字检索排除 typed media；引用有 source SHA，失效明确拒绝。
- 时间线/history 仅附件引用；上传 prompt 不查 BLOB、不自动送原文。清单给出 source_sha256/原始与保存大小、未压缩/转码/抽帧及 delivery_scope=upload_prompt；签名探测不冒充解码/分析。UI 按 session JOIN 读耐久副本，共享 cwd 不声称 engine 间 OS 隔离。
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
| input_attachments.rs | 原字节/限额验证、session no-follow 新文件及 BLOB 保存、处理清单、引用 prompt/归属预览、Unicode/20 MiB/媒体/重启/迁移/越权回归 |
| context_history.rs | 跨 branch/engine 的固定产品会话可见历史/失败检索、上传文本有界读取、来源 hash/UTF-8/高水位/隔离与重启回归，无第二份日志 |
| media.rs | schema 24 私有原件/派生字节校验、no-follow 新产物、只读 decoder FD 快照、选择快照与投递 receipt 的原子状态/重启/越权/损坏回归 |
| tool_content.rs | 脱敏全文分块、原子引用与按会话授权读取，含迁移/Unicode/越权回归 |
| message_journal.rs | 完整可见 assistant 文本、游标分页、会话隔离与重启终态，发送前持久化失败不可声称成功 |
| automation.rs | 暂停优先 interval CRUD/revision、固定普通 session/branch/route、原子 claim 与完成/失败/Stop 结算、重启不补跑及协议/运行替身回归 |
| tool_settings.rs | 真实 catalog 能力开关、暂停优先 MCP CRUD/授权、全局 revision、限额/损坏拒绝与 schema-21 迁移回归 |
| branch.rs | EngineBranch、切换与 fallback policy |
| handoff.rs | Handoff 构造、保存与过滤 |
| team.rs | 团队、worker 生命周期与队列 |
| orchestration.rs | 队列领取的纯策略 |
| projects.rs | 项目登记与会话关联 |
| tests.rs | 产品持久化与恢复测试 |
