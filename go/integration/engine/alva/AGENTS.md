# 横向支撑 · Alva Engine adapter
> 通过完整 harness 执行的 EngineRuntime，不直接绕过默认Plugin或内核。
## 地位
产品桥接：Hub 外层装配 Factory，adapter 依赖完整 L5 harness/SDK；不是 SDK 层，Hub core 只看 integration/engine 根契约。
## 逻辑
新native ID随机生成，resume只加载指定现存会话；Factory绑定实际native store/工作区/route。tool结果保留native事实，engine事件只给qualified引用。取消只请求，真实SDK返回和清理决定终态。
真实ProviderError按完整error tree保守分类；外层所有叶子必须同类，SessionError/审批/cleanup旁因或已见response一律execution。Factory失败也记录真实fatal+Completed，不伪造SessionStarted；observer落盘失败直接失败不再发送或重放。
完整Message只来自SDK实际committed assistant事实，使用native引用，不拼delta/思考/工具中间preface；每个model响应独立稳定ID。Factory取得资源的runner实现可选OwnedRunner.Close，初始SessionStarted观察失败也回收，强cleanup故障仍归execution。
## 约束
未知resume不得创建。审批能力未接入时明确unsupported。副作用分类未知不得擅自声称read-only。model/provider配置属于Factory，不读取旧用户配置。
## 业务域清单
| 文件 | 职责 |
|---|---|
| adapter.go | 完整harness执行、事实引用、工具配对和取消；option-aware FactoryOptions/独立Descriptor保留普通factory保守拒绝 |
| inspector.go / inspector_test.go | 独立RepositoryInspector：冷分页/搜索/2KiB原文、typed link和causal index双验证、fixed-water JSONL与背压live metadata；观察取消不释放真实run claim |
| adapter_test.go | 实际harness多轮/身份、取消终态与factory Activity落盘失败不启动 |
| failure.go / failure_test.go | typed provider整树分类、真实local HTTP无重试、factory终态及mixed强失败回归 |
| options_test.go | 实际SDK单轮model/tool后iteration耗尽为失败、错误边界与ordinary factory前置unsupported/超限拒绝 |
| message_test.go | effective AfterModel实际commit再发完整消息，不全量重读历史、不把中间preface当Worker结果 |
