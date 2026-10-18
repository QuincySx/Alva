# L3 · sdk/agent/context
> 上下文能力库：引用检索契约及独立于持久事实的上下文管理实现。

## 地位
SDK 能力，不持有产品文件归属、上传政策或 durable session authority。

## 逻辑
ReferenceBackend 由固定 session 的宿主实现，同会话原始 facts/blob 是唯一存储权威。
ReferenceReader 校验 UTF-8 byte cursor、4KiB 页、20 条 literal matches 与 256-byte snippets，不能通过 hash 全局越权读。
Store实现八方法工作集ContextHandle；Rules/Default/Chain七hooks通过该公开接口调用，不依赖具体Store类型。文件/记忆/rewrite/summary/externalize能力属于本层；DefaultApply显式接收Summarizer，不从kernel Handle获取重能力。
Summary消费被选中的实际detached工作集，不能用工作集index去误读store另一段内容；压缩保留整个tool协议组，尤其critical条目。
HooksChain逐层保留已设置的priority和原message identity，后续未指定priority不清空前层决定；返回message/priority不借用callback原值。各budget hook获得独立snapshot，不能改写调用方或兄弟政策的输入。
MemoryReferences是明确volatile、有界且不静默逐出的backend；宿主持久实现复用同一session artifact authority。

## 约束
原始内容是否投递模型、媒体转换、跨 branch 归属属于产品政策，不藏入 kernel 或该接口。
不自动读取文件/媒体或压缩抽帧；外部化必须显式ReferenceBackend或授权Externalizer，说明JSON处理与原事实保留。
Store显式文件导入走本层host-bound ContextFileLoader，传实际agent/session identity、选定行与硬字节预算；path字符串本身不授权。
StoreServices是一次装配能力snapshot，可绑定MemoryFactsBackend；外置Query返回后重查取消，不消费迟到facts；默认fact map仍只是进程内memory，不冒称宿主持久实现。
RewriteBatch整批预验证并原子修改工作集；Externalize先存实际artifact，再比较工作集snapshot，冲突/取消保留artifact并明确不替换原工作集。
外置TokenCounter在Store写锁外执行，允许只读观察Store；写入与整批rewrite仍在锁内提交。压缩summary检查callback结束时的deadline/cancel，外部化验证真实reference并保留原processing说明，取消不替换工作视图；不能强制终止忽略context的Go callback。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| References | `reference.go` | Backend/分页/搜索 DTO、边界验证 |
| Volatile references | `reference_memory.go` | 固定session、去重、UTF-8字节分页与literal search |
| Capability tools | `reference_tools.go` | 实际context_read/search工具、严格参数与4KiB编码结果 |
| Store | `store.go` | 四层entries、快照、预算、metadata、fact与summary callback |
| Handle capabilities | `services.go`, `services_test.go` | 显式导入/后端memory迟到取消/原子rewrite/冲突安全外部化 |
| Policies | `hooks.go` | Rules/Default/Chain七hooks组合与确定性fallback |
| Final admission | `admission.go`, `admission_test.go` | 可选最终ModelRequest预算政策，覆盖tools和后置注入；不是provider真实token保证 |
| Run lifecycle | `lifecycle_test.go` | Bootstrap失败/复用与跨session启动，Dispose每Run一次且不假设逐session关闭 |
| Hook composition | `composition_test.go` | priority/identity逐层组合、返回值所有权、budget snapshot隔离与真实压缩保留Critical输入 |
| Callback settlement | `callback_test.go` | 四条Store写路径的counter可重入观察、取消/无效reference不替换工作集与原始processing保留 |
| Actions | `apply.go` | 注入、safe window/summary/priority/tool-result/externalize |
| Scope collaboration | `scope/` | direct-parent生命周期、黑板、只读父视图、spawn通信 |
| Regressions | `context_test.go`, `compression_test.go` | 实际工作集与耐久事实分离、引用分页、协议组压缩 |
