# 横向支撑 · Alva Engine adapter
> 通过完整 harness 执行的 EngineRuntime，不直接绕过默认Plugin或内核。
## 地位
产品桥接：Hub 外层装配 Factory，adapter 依赖完整 L5 harness/SDK；不是 SDK 层，Hub core 只看 integration/engine 根契约。
## 逻辑
新native ID随机生成，resume只加载指定现存会话；Factory绑定实际native store/工作区/route。tool结果保留native事实：ToolStart只复制≤16KiB的有效参数JSON（超出不放，经start引用读intent），ToolEnd只复制结果文本的8KiB UTF-8前缀（超出Truncated，经end引用读outcome；引用块渲染为小JSON定位符，媒体/不透明块只写占位说明），效果仍Unknown（kernel事件不带ToolIntent.Effect，不按工具名猜）。ToolStart/ToolEnd带本引擎自己的Display：tool_display.go按Alva内置工具名从参数与完整结果（不是8KiB前缀）归一，未知工具为other。产品事件与原生记录同规则脱敏（platform/storage/redaction.Redactor：键名+文本标记）：参数先JSON脱敏再判16KiB，结果逐内容块脱敏再取8KiB前缀，Display由脱敏后的参数/结果派生并逐字段再脱敏后才Bound，对话与轨迹看到的一致。execute_shell的回执（job_id/status）转成Display的状态、退出码和OutputInTrace，不把回执当输出。取消只请求，真实SDK返回和清理决定终态。
真实ProviderError按完整error tree保守分类；外层所有叶子必须同类，SessionError/审批/cleanup旁因或已见response一律execution。Factory失败也记录真实fatal+Completed，不伪造SessionStarted；observer落盘失败直接失败不再发送或重放。
完整Message只来自SDK实际committed assistant事实，使用native引用，不拼delta/思考/工具中间preface；每个model响应独立稳定ID。Factory取得资源的runner实现可选OwnedRunner.Close，初始SessionStarted观察失败也回收，强cleanup故障仍归execution。
## 约束
未知resume不得创建。审批能力未接入时明确unsupported。副作用分类未知不得擅自声称read-only。model/provider配置属于Factory，不读取旧用户配置。
存储只消费本包按需组合的共享storage能力插口；会话/引用/DTO/错误从同一契约返回，不导入具体session.Repository或SQL实现。后端由上层程序/插件装配选择，组件不回落默认库、不downcast实现，不持有存储Close。

## 业务域清单
| 文件 | 职责 |
|---|---|
| adapter.go | 完整harness执行、事实引用、工具配对和取消；kernel用量按wire.Usage指针语义累加缓存读/写计数（每次响应都报告才累加，任一次未报告即为未报告；不一致时只丢缓存数）；最终消息60KiB/内容块16KiB/工具结果8KiB的UTF-8前缀以Truncated标记，工具参数≤16KiB；option-aware FactoryOptions/独立Descriptor保留普通factory保守拒绝；FactoryOptions.ContextCompaction 才声明 Capabilities.ContextCompaction 并接受 CompactContext，Request.ReportContext 绑定到同一 sink 发 ContextUpdated，摘要请求的 usage 并入本 attempt 的 Completed Usage |
| tool_display.go | Alva内置工具名（execute_shell/read_file/write_file/edit_file/search_files/agent/todo_write）→ 未截断engine.ToolDisplay：参数给命令/路径/diff与增删行/关键词/任务/待办，完整结果给shell回执状态/退出码/OutputInTrace、Output引用、命中数与结果、子会话与outcome；finishDisplay先逐字段脱敏再Bound |
| redaction_test.go | 真实脱敏原生store：命令Authorization、搜索命中API_KEY=、编辑client_secret=在原生记录与ToolStart/ToolEnd的Input/Text/Display里同为[REDACTED]且普通内容保留；shell回执转状态、非回执不误判 |
| tool_events_test.go | 实际harness工具波次：参数/结果原样带出、超16KiB参数只留引用、8KiB结果UTF-8前缀+Truncated且引用可读全文、错误结果、引用块定位符与媒体不复制；各内置工具的Display（截断结果下仍按完整结果计数）、未知工具为other |
| inspector.go / inspector_test.go | 独立RepositoryInspector：声明BackwardPages，冷向前/向后（Before与After/Literal互斥，InspectionEnd取尾页）分页并带出配对字段与工具时间、向前字面搜索、2KiB原文、typed link和causal index双验证、fixed-water JSONL与背压live metadata；观察取消不释放真实run claim |
| wait_test.go | 真实Close后才读取waiting回执、invalid/未配置/取消/收尾失败不得waiting、继续不重传prompt |
| adapter_test.go | 实际harness多轮/身份、取消终态与factory Activity落盘失败不启动；用量缓存未报告/报告0/全部报告累加/部分未报告视为未报告/超过输入时降为未知；上下文报告按序转发、摘要用量计入、未声明能力时拒绝 CompactContext |
| failure.go / failure_test.go | typed provider整树分类、真实local HTTP无重试、factory终态及mixed强失败回归 |
| options_test.go | 实际SDK单轮model/tool后iteration耗尽为失败、错误边界与ordinary factory前置unsupported/超限拒绝 |
| message_test.go | effective AfterModel实际commit再发完整消息，不全量重读历史、不把中间preface当Worker结果；超限消息/内容块为带Truncated的UTF-8前缀 |
| interject.go / interject_test.go | InterjectionPlugin：BeforeModel中间件（序-500）在工具结果之后提交队列里的用户消息（消息ID前缀`interject:`），写入原生日志与本次请求后才由Adapter发InputDelivered回执；Adapter每次Execute持有至多8条的信箱，结束即关闭，未取走的消息不送达（产品放回队首）；FactoryOptions.Interjection/Fork声明能力，Request.Fork须声明后才接受且与续接互斥 |
