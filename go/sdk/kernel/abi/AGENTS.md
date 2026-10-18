# L1 · sdk/kernel/abi
> 通用循环的最小接口与可扩展的耐久事实，不携带 Agent 产品概念。

## 地位
L1；只依赖 `sdk/llm/wire`、sdk/kernel/bus 和标准库，不得依赖 sdk/agent、harness、platform、apps、integration 或 MyGo。

## 逻辑
`LanguageModel` 产出序列化 chunks；`Tool` 的调度、效果与只读性针对实际调用判断。
ModelIdentity是可选只读metadata，不强制第三方模型声明provider/model；缺失时不能从route猜造。
工具显式接收 `ToolExecutionContext`，不从 `context.WithValue` 偷渡会话服务。
`ConversationReader` 仅有 ID/messages；`ConversationLog` 嵌入 reader 并拥有 atomic append 与可失败 flush；可选 `SessionStore` 仅提供额外
records 读取，`PersistentSession` 只是组合，循环和 SDK 不强制拥有查询接口。

## 约束
AppendRecords 必须整批全有或全无，ID/sequence/time/causal identity 由存储验证和分配。
conversation Message.ID为空时存储绑定record.ID，返回与Messages投影必须一致；context身份不能依赖当前列表位置。
内置 envelope 使用 `BuiltinRecord` 构造当前 `alva.kernel`/type/schema=1；扩展使用独立 namespace/type/
非零 schema 与原始 JSON，保留未知有效扩展数据，不通过增加 kernel enum 承载每个产品功能。
Conversation records 才进入模型 transcript；执行 intent/outcome 不隐式进入上下文。
Optional middleware hooks 是机制；权限、日志检索、模型投递策略必须由上层贡献。
Middleware只描述身份和按需实现的typed hooks；装配顺序由agent/core注册入口处理，kernel不定义Priority或Configure契约。
PrepareToolHook通过ToolPreparationContext取得本轮ActualTool（nil为未知/不许可），在intent前处理typed参数/结果union，name/ID固定；原assistant声明不改，ToolIntent.Call为有效参数，DeclaredCall仅在改参时保留原值。
NoSideEffect只证明该Tool.Execute/执行hooks未调用，不证明prepare/native插件IO无副作用，不能据此安全重放Attempt。
SessionError 保留实际 messages/append_records/flush 操作及 session identity；即使其原因是超时也不能降级为普通任务取消。
OnlyCancellation 递归检查 Join 的全部叶子，任何真实失败或 typed session failure 都不是纯取消。
EventMessageCommitted仅为完整assistant响应的实际耐久事实（含effective AfterModel与record identity），中间toolcall响应也可能出现；不是Run成功终态，partial失败正文不发此事件。
RuntimeContext.InvocationModel仅在ModelFunc/WrapModel/AdmitModel带本次实际模型snapshot，不是policy；BeforeModel修改State.Model影响本次，wrapper期间修改仅影响下次。InvocationSessionID绑定本轮log，不从可变State.Session推断在途归属。
ToolFunc每层零或一次；ModelFunc保留无公开输出的顺序多次尝试，每层拒并发/迟到且join真实工作。公开流后的base/wrapper错误不能被吞成成功。ModelAdmissionHook只读检查真正传入captured模型接口的请求，不拥有token或产品政策。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Loop contracts | `contracts.go` | Model/tool/log/store/event/middleware、prepare union 与 typed records |
| Record integrity | `records.go` | 当前 envelope 构造、union 校验与完整 deep clone |
| Error semantics | `errors.go` | 类型化会话操作失败与无损取消分类 |
| Event ownership | `events.go` | 队列/订阅者的 event 深拷贝 |
| Runtime state | `state.go` / `state_test.go` | typed-nil model/log/tool拒绝且失败替换不改变live state/registry version； 实际mutable model/session/tool registry与typed Extensions |
| Context contracts | `context.go` | 七hooks/窄工作集Handle/Apply/Counter，以及独立只读ContextSource |
| Scheduling | `scheduling.go` | 词法资源identity、描述符校验与冲突机制（不宣称路径授权） |
| Backend conformance | `sessiontest/` | Memory/native/第三方共用原子性、身份、投影与协议；容量政策由后端独立验证 |

ContextSource是显式装配的模型工作视图，与ConversationLog完整事实投影独立；失败不得回退全量Messages，配置的source必须保留最新输入和完整tool协议组。ContextHandle只拥有工作集快照/预算/枚举/读入/标记/推进turn八方法；文件IO、记忆服务、改写和摘要/外部化操作由L3显式实现与注入。MemoryFact仍由Injection.Memory实际消费，不做无人消费的类型搬运。
