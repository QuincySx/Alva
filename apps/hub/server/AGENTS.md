# L7 · Hub HTTP/SSE transport
> 同源 native 产品服务的有界本地 HTTP 入口，不维护第二份会话或执行权威。

## 地位
L7 transport，依赖 apps/hub/product；不依赖 desktop/MyGo，不读取旧 Rust 配置或嵌入 React。

## 逻辑
显式 loopback IP:port 和带外 256-bit Bearer 认证，精确 Host/Origin 防 rebinding/跨站；
所有会话资源从 committed HubSession/branch 推导 native owner，不能客户端自选 native ID。
Turn 由产品 runtime 认领，SSE 从耐久 Sequence 续读，观察断线不是 Stop。

## 约束
JSON 严格字段/Unicode/大小；无 token query/CORS/隐式别名。上传 2 并发、20MiB 原字节、
仅 plain 文件名，经共享 UploadReader 继续限每轮 40MiB/8件且引用优先。
SSE 16 路、每页32条、写 deadline；Inspector 2KiB 文本/4KiB 字节且不启动 Agent/MCP。
子Inspector仅接受≤8层typed link Sequence路径，不能客户端给native/child ID。
SubAgent下一轮开关与真实子审批复用同产品服务，不借父identity/父授权。
Team仅Lead+Team+Worker已存归属；创建Worker须明确账户/计费同意，入队不执行，Resume才认领。
AEP仅操作native/operator已检查的pin开关，不提供HTTP主机source导入或文件staging。
transport error 不泄露 host path/数据库/provider stderr；真实执行错误仍在已提交事实中。

## 业务域清单
| 文件 | 职责 |
|---|---|
| security.go | 实际 listener authority 与随机 token 校验、Host/Origin/认证 |
| server.go | 冷/v1/engines能力声明、bounded handler、严格 JSON/query、session 发现与创建 |
| gateway.go / gateway_test.go | authenticated /v1/gateway冷status/独立model计费同意start/Stop，一次token仅start返回、不接受Provider/key导入 |
| platform/system/strictjson（共享） | DTO object/null/重复键/精确大小写/深度验证，Hub不再保留第二份解析器 |
| routes.go | turns/stop/routes/approvals/files/settings/Skills/Inspector 共用服务 |
| export.go / export_test.go | /inspector-bundle认证只读JSONL下载，固定高水位/typed路径与显式child scope；两路导出上限、不接受host/native path、不写宿主文件或restore |
| inspection.go / inspection_test.go | /inspector-events绑定实际branch/typed path，通过独立EngineInspector背压SSE；durable Last-Event-ID续读，transport Close/断线仅detach，不restore或Stop |
| events.go | 耐久分页与 sequence/Last-Event-ID SSE，不拥有执行 |
| team.go | 同产品Team持久FIFO/生命周期与显式dispatch，Worker上传可explicit include_draft_attachments引用入队，事务固定branch；冷读取不重放 |
| tools.go / tools_test.go | 六类预存工具next-turn启用、Shell/Web独立access同意与真实问题/回答，实际HTTP→SDK→Tool→model反例 |
| shell.go / shell_test.go | authenticated已存branch/job冷状态、operator Stop/显式reset；真实两轮SDK后台存活、foreign拒绝与product租约排他回归 |
| worktree.go / worktree_test.go | owned Lead/Team/Worker隔离、冷diff小页与exact evidence/revision+独立同意的原字节采纳及whole-tree删除review/独立confirm/retire，保留branch/history/evidence，无客户端host path |
| media.go | 冷派生metadata/manifest/固定高水位receipt与完整SHA binary/range预览，不自动处理/送模型 |
| server_test.go | 真实 stores/HTTP/native demo 与权限、续读、引用隔离回归 |
| child_test.go / team_test.go | 实际HTTP子审批/typed路径与团队FIFO/account批准/冷观察不重放 |
| media_test.go | 真实原件+派生的冷metadata/二进制range/越权拒绝与无处理/无投递回归 |
| automation.go / automation_test.go | 固定实际路由/乐观revision/默认暂停计划CRUD与显式计费同意；不提供改时钟/追赶/重放HTTP入口，以及保存Handoff冷读归属回归 |
