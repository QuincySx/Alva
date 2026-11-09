# L7 · Hub HTTP/SSE transport
> 同源 native 产品服务的有界本地 HTTP 入口，不维护第二份会话或执行权威。

## 地位
L7 transport，依赖 apps/hub/product；不依赖 desktop/MyGo，不读取旧 Rust 配置或嵌入 React。

## 逻辑
显式 loopback IP:port 和带外 256-bit Bearer 认证，精确 Host/Origin 防 rebinding/跨站；
所有会话资源从 committed HubSession/branch 推导 native owner，不能客户端自选 native ID。
Turn 由产品 runtime 认领，SSE 从耐久 Sequence 续读，观察断线不是 Stop。
显式程序路由仍按启动快照冻结，已保存应用连接的动态路由只从同一共享owner的已提交缓存读取；HTTP不新增凭据CRUD入口。

## 约束
JSON 严格字段/Unicode/大小；无 token query/CORS/隐式别名。上传 2 并发、20MiB 原字节、
仅 plain 文件名，经共享 UploadReader 限每会话暂存40MiB/8件且引用优先。turns与Worker新任务必须按回执显式提交references集合（kind/id/sha256），空或省略无附件；未知/重复/跨branch拒绝且不执行，成功只消费同集合。删除以draft_id标识，仅移除引用，保留原件。
SSE 16 路、每页32条、写 deadline；发送turn的prompt上限32KiB（routes.go，请求体64KiB，存储另有64KiB上限）；Inspector 2KiB 文本/4KiB 字节且不启动 Agent/MCP。
子Inspector仅接受≤8层typed link Sequence路径，不能客户端给native/child ID。/inspector的before参数向后翻页（0=快照末尾），与after/q同时给出即400。
SubAgent下一轮开关与真实子审批复用同产品服务，不借父identity/父授权。
设置开关（PUT .../settings/{subagents|memory|media|media-decoder|browser|mcp/ID|aep/ID|commands/hook|diagnostic/ID}）走product.SettingsService，与桌面同一语义：默认写**应用级**（对所有任务生效，不会把本任务变成“仅本会话”）；正文可带`"scope":"session"`才写会话级，且该任务须已选“仅本会话”，否则409。启用宿主项须同一请求带`host_access_approved:true`和`fingerprint`（来自`GET .../settings/app`返回的SettingsView里该项的指纹；在服务锁内与当前配置比对，缺失或不符409且不写任何状态；应用级同时登记该工作区批准，任务无工作区则拒绝）。`GET .../settings/app`是只读的应用级脱敏视图（已存秘值只是占位）。skills恒为会话级。拒绝一律409，正文`{"error","message"}`的message为产品自有中文原因（不含存储/Provider/子进程文本）。`scope`取值非app/session为400。GET .../settings仍返回会话快照（HubService.SessionSettings），不是生效能力集。会话草稿里的MCP项只有scope为session才找得到。
Team仅Lead+Team+Worker已存归属；创建Worker须明确账户/计费同意，入队不执行，Resume才认领。
AEP仅操作native/operator已检查的pin开关，不提供HTTP主机source导入或文件staging。
transport error 不泄露 host path/数据库/provider stderr；真实执行错误仍在已提交事实中。

## 业务域清单
| 文件 | 职责 |
|---|---|
| preferences.go / preferences_test.go | authenticated /v1/preferences GET/PATCH复用共享owner及应用级SQLite，严格JSON/乐观revision/错误脱敏，不创建session或执行模型 |
| routing_test.go | HTTP branch/fallback复用产品route政策，完整账户批准与动态连接失效均零执行；程序路由仍保留装配快照 |
| security.go | 实际 listener authority 与随机 token 校验、Host/Origin/认证 |
| server.go | 冷/v1/engines能力声明、bounded handler、严格 JSON/query、session 发现与创建 |
| gateway.go / gateway_test.go | authenticated /v1/gateway冷status/独立model计费同意start/Stop，一次token仅start返回、不接受Provider/key导入 |
| platform/system/strictjson（共享） | DTO object/null/重复键/精确大小写/深度验证，Hub不再保留第二份解析器 |
| wait.go / wait_test.go | 认证冷等待/opt-in、严格branch/Turn/revision结果提交、显式202继续与审阅放弃；真实HTTP→Agent同Turn回归 |
| settings_scope_test.go | 设置开关默认应用级、显式session级、每请求宿主批准与中文拒绝原因，被拒不改任何状态 |
| routes.go | turns/stop/routes/approvals/files/settings/Skills/Inspector 共用服务（设置开关经SettingsService，见上）；compatibility仅项目相对配置下一Turn启停，无任意绝对主机路径 |
| compatibility_test.go | 认证HTTP冷配置启停、严格body/项目根内路径、错误无主机路径与零执行回归 |
| export.go / export_test.go | /inspector-bundle认证只读JSONL下载，固定高水位/typed路径与显式child scope；两路导出上限、不接受host/native path、不写宿主文件或restore |
| inspection.go / inspection_test.go | /inspector-events绑定实际branch/typed path，通过独立EngineInspector背压SSE；durable Last-Event-ID续读，transport Close/断线仅detach，不restore或Stop |
| events.go | 耐久分页与 sequence/Last-Event-ID SSE，不拥有执行 |
| team.go | 同产品Team持久FIFO/生命周期与显式dispatch，Worker上传以明确references数组引用入队，移除隐式include_draft_attachments参数，事务固定branch；冷读取不重放 |
| tools.go / tools_test.go | 六类预存工具next-turn启用、Shell/Web独立access同意与真实问题/回答，实际HTTP→SDK→Tool→model反例 |
| shell.go / shell_test.go | authenticated已存branch/job冷状态、operator Stop/显式reset；真实两轮SDK后台存活、foreign拒绝与product租约排他回归 |
| worktree.go / worktree_test.go | owned Lead/Team/Worker隔离、冷diff小页与exact evidence/revision+独立同意的原字节采纳及whole-tree删除review/独立confirm/retire，保留branch/history/evidence，无客户端host path |
| media.go | 冷派生metadata/manifest/固定高水位receipt与完整SHA binary/range预览，不自动处理/送模型 |
| server_test.go | 真实 stores/HTTP/native demo 与权限、续读、引用隔离回归 |
| child_test.go / team_test.go | 实际HTTP子审批/typed路径与团队FIFO/account批准/冷观察不重放 |
| media_test.go | 真实原件+派生的冷metadata/二进制range/越权拒绝与无处理/无投递回归 |
| automation.go / automation_test.go | 固定实际路由/乐观revision/默认暂停计划CRUD与显式计费同意；不提供改时钟/追赶/重放HTTP入口，以及保存Handoff冷读归属回归 |
