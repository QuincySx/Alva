# L7 · Hub product supervisor
> Durable HubSession/EngineBranch/Turn/Attempt 产品监管，不是 native Agent records 的第二权威。
## 地位
本包只依赖 engine contract 与标准库，具体 adapter/SQLite/UI 在外层装配。
## 逻辑

AEP产品快照仍由程序选择组件：显式interpreter供JS/Python，空interpreter供manifest验证后的嵌入式WASIp1；核心只做有界纯值验证，不读取manifest或启动runtime。
产品先认领耐久 Turn/Attempt，再执行引擎；自动 fallback 串行、固定同一Turn，显式账户/计费授权快照且只有 startup/rate-limit、无可能副作用才可用。Stop 不重放。EngineBranch 保留 native identity，手动切换可返回旧分支；handoff 仅索引/引用、不复制全部原始历史。
## 约束
Runtime仅消费六方法ExecutionStore；设置/浏览由实际产品consumer要求，SQLite实现及事务不拆分。
产品 ID 永远不等于 native ID。UI 只观察已提交产品历史，不拥有执行取消。可恢复 Error 不释放 active；唯一 Completed 收尾。SQLite产品store在子包，与native session.sqlite隔离，不读旧Rust DB。
引擎回调在产品层串行提交；首次校验/持久化错误锁存，adapter忽略错误或后补成功也必须失败且不fallback。Execute返回后关闭回调准入，迟到回调不能修改facts。
Completed后的真实Execute返回错误也不能让Turn成功或fallback；原始engine事件保留，产品记录execution_owner_failure并失败收尾，不能用早到事件覆盖owner错误。
NativeOwnerResolver是产品存储的显式只读路由契约；ErrNotFound统一缺失对象语义，不泄漏SQL实现。Runtime仍仅要求ExecutionStore六方法，不因Service的完整profile扩大执行依赖。

## 业务域清单
| 文件/目录 | 职责 |
|---|---|
| model.go | 产品事实、分开的ExecutionStore/SessionStore/SettingsStore与产品Store组合接口与版本化交接索引 |
| runtime.go | 独立Descriptor registry/冷能力查询及engine registry、串行执行、取消/审批路由与授权fallback |
| descriptor_test.go | 第五engine声明/不可替换、纯冷读与snapshot切片隔离、关闭后拒绝注册 |
| settings.go / aep.go | next-turn私有MCP/AEP进程snapshot、native SubAgent opt-in纯值与预算，不引入protocol/进程依赖 |
| tools.go | Shell/Web显式配置、独立access批准与Planning/Notebook/Interaction/Utility下一Turn可选开关纯值 |
| commands.go | lifecycle Hooks/diagnostics明确argv/env/timeout与独立host批准纯值，禁止内核进程政策 |
| media.go | 默认关闭媒体工具、两条显式decoder路径和独立host批准纯值，不推测模型视觉能力 |
| browser.go | 默认关闭browser的明确exe/env/headless/proxy/timeout与独立host/network批准纯值 |
| workspace.go / workspace_test.go | 有界公平workspace共享/独占租约、actual admitted lease可显式retain至后台physical reap；seal拒绝late retain，不重排reader造成writer死锁 |
| workspace_action.go / workspace_action_test.go | 显式文件mutation复用双方workspace独占租约、Runtime关闭真实取消/join、重叠路径去重 |
| team.go | 完整产品Team/Worker/Task、附件草稿branch pin、资源意图与分离TeamStore/WorkerDraftStore合约 |
| team_snapshot.go | handoff的真实Team/Worker/head/last outcome小索引，不转移正文或native日志权限 |
| worktree.go | Worker私有工作树binding、固定SHA/path evidence、revision/adoption/明确删除审阅及removed/unknown纯产品事实合约 |
| team_runtime.go | FIFO任务同事务认领并复用唯一Hub Runtime执行owner |
| automation.go | 默认暂停、固定branch/route的本地计划事实与独立AutomationStore，同普通Turn原子准入而非第二runtime |
| sqlite | 私有耐久产品store、事务认领、历史与崩溃修复 |
| product | UI无关L7共享服务、真实文件/历史/Inspector/Skill与SDK配方 |
| bootstrap | Hub共用实际装配和安全关闭，MCP/AEP等exact attempt资源重试与facts释放顺序，desktop/server不各造recipe |
| desktop | 独立Hub自绘界面，消费共享产品服务与基础desktop控件，不拥有执行 |
| worktree | 显式approved Git、no-checkout直读blob、pin父目录原字节diff/采纳；不运行filter/checkout hook或自动commit/reset |
| server | 认证 loopback HTTP/SSE，同源 product 服务与耐久游标，不接客户端 host path |
| devtools | 独立只读stdio MCP诊断；显式Hub/token fd，真实bounded历史index/typed Inspector/2KiB读取，不启动或修改Agent |
