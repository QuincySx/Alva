# alva-hub-server/src
> 单文件 Axum 产品传输层。

## 地位

复用 `alva-hub-desktop` 的 store、engine、permission、timeline 与 Inspector 服务；
不拥有另一套业务状态机。

## 逻辑

1. `AppState` 初始化私有 HubStore、ProviderCatalog、共享 Alva Inspector repository 与 live engine registry。
2. JSON handlers 把 HTTP 请求转发到稳定产品 API；运行事件通过 SSE 广播。
   SSE 原样保留产品 `message_position`（attempt scope + UTF-16 offset），不另造片段序号或按正文去重。
3. Stop（R51/R65）：cancel 成功后立即持久化取消事实（失败上抛 500，不静默）；SSE 的 Interrupted 终态由 run handler 在 outcome（真实流结束）后统一结算——`stopping` marker 新一轮开始时清理、ack 后 run 已结束则丢弃，不泄漏到同 session 下一轮；引擎自然完成的取消不追加 Interrupted。
4. Inspector handler 按 product session 读取 durable native snapshot，wire shape 与 Tauri command 一致。
   `/api/sessions/:id/tool-content/:content_id?chunk=N` 复用 HubStore，blocking worker 中读取一块 ≤16 KiB 已脱敏正文；越权/缺失均为 404。
5. 未命中 API 的请求落到内嵌 React 静态资源。
6. `/api/sessions/:id/workspace/files?path=` 与 `/workspace/file?path=` 在 blocking worker 内调用共享只读文件服务；cwd 仅从 session 取，错误不泄露宿主路径，敏感文件和链接不可穿透。
7. `/api/sessions/:id/workspace/git` 与 `/workspace/git/diff?path=&area=staged|unstaged|untracked` 调用共享有界 Git reader，HTTP 不接受任意 cwd，也不提供写入操作。
8. `/api/sessions/:id/messages?after=&through=` 是会话隔离的 128 条消息页；through 固定一次追赶范围，after 边界重放一条用于终态更新，不是独立事件身份。前端按 message_position 去重，不将 SSE 丢包当不可恢复历史。
9. `/api/sessions/:id/workspace/skills` GET/POST 复用共享 WorkspaceSkillsManager，blocking worker 中读目录/源文档或执行用户明确的创建/编辑/归档/恢复；不接受任意 workspace，不调用 engine。测试构造器显式关闭共享/legacy catalog 读取，只使用临时假配置。
10. `/api/automations` GET/POST 复用共享 schema-21 CRUD/授权判断；只有 production main 在监听启动后运行 10 秒 pump，最多 4 并行，并沿同一个 execute_run/SSE/审批/Stop 通道认领执行。paused-first、不自动 fallback、不补跑，不另起领域状态机；HTTP fixture 不自动启动 pump。认领前拒绝不能消费另一活动 Attempt 的 stopping marker。
11. `/api/run` 专用 64 MiB HTTP 请求上限，复用共享附件 validation（原字节 20 MiB/40 MiB/8 文件）和引用 prompt；其他 routes 不扩大 body 上限。`/api/sessions/:id/attachments/:attachment_id` 在 blocking worker 中按 session JOIN 读耐久 BLOB；缺失/越权 404，不接收宿主文件路径。live/history 仅引用及处理清单，媒体预览不自动成为模型输入；真实 HTTP 20 MiB 二进制边界回归使用本地替身。
12. 认领后的 engine builder 传同一个 HubStore/product_session_id 给共同 ALVA Plugin 配方，覆盖分支恢复与 fallback；`hub_context` 检索本会话跨 engine 可见记录，`hub_media` 仅显式处理/选择并走现有审批与固定模型声明。其他 engine 不消费 ALVA Plugin；没有第二套 HTTP 检索数据库或媒体状态机。
13. 附件下 `/media?after=`、`/media/:artifact_id`、`/media/:artifact_id/receipts?after=&through=` 三个 GET 在 blocking worker 复用共享归属/4 项索引/哈希 BLOB/5 项回执 reader，DTO 与 Tauri 同构。不接收宿主路径，索引/回执拒绝未知 query；越权/缺失/损坏/非法 cursor 一律通用 404，解析失败 400；不执行 decoder/engine/重放选择。真实 HTTP 回归只在临时私有 DB 放置合成产物/未知接受 receipt，验读/损坏/越权，不冒充 provider 测试。

## 约束
- `/api/tool-settings` GET/POST 复用 schema-22 capability/MCP CRUD/revision；每轮固定 snapshot 注入共同 assembly。`/api/tool-settings/mcp-test` 确认后绑定已存 session cwd 做一次 stdio 测试与回收，无模型调用；不接受 arbitrary cwd、env/header。

- handler 不复制 Hub 领域判断。
- 错误响应不泄露 secret、provider response body 或 raw adapter stderr。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| HTTP service | `main.rs` | AppState、API/SSE、Inspector/工作区 Skills/自动化 pump、只读附件/派生媒体/回执 endpoint、独立测试配置与静态 fallback |
