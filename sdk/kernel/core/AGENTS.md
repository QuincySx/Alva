# L2 · sdk/kernel/core
> 可替换的通用 Agent loop 与有界事件投递，不接受产品能力扩展。

## 地位
L2；仅消费 kernel ABI/wire，不依赖 agent、harness、Provider 或平台。

## 逻辑
循环先原子提交用户输入，模型请求前提交 model intent；完整响应、model outcome 和全部 tool intents
在一个 transaction 中提交后才执行工具。结果与 tool conversation 同批提交，每个工具绑定实际 run/call ID。
BeforeModel 使用脱离耐久事实的 request copy，保持插件可做投影而不篡改原始记录。
每轮在context/BeforeModel前捕获工具目录，其schema与实际Tool共同绑定；Register/Replace/Remove下一轮生效，执行前权限仍动态检查。
PrepareTool通过ToolPreparationContext取得同一绑定，按outer→inner逐call返回改参或替代结果，替代短路内层prepare；准备结束后计算效果/只读性/资源claims，
准备真正错误时原子提交成功模型事实与全部failed/unexecuted工具证据，不执行整批工具。NoSideEffect不证明插件自己的IO无副作用。
AfterModel改写后仍受wire已有内容块数量上限约束，修复改写绕过stream边界的机制缺陷；不向kernel契约添加后端容量政策。
提交后BeforeTool/WrapTool/AfterTool收到独立copy，修改参数或next传入不同call均硬失败；包装层不能吞掉next改参证据。
AgentState实际持有可变 model/tools/session/extensions；RunConfig只持有本次run的不可变政策配置。
RuntimeContext显式传递真实State与run/workspace/bus，不用context.WithValue或不能生效的snapshot取代。
模型在全部BeforeModel之后、wrapper链之前capture一次；ModelFunc/WrapModel的InvocationModel是同一实际对象，base始终调用它。wrapper/外线程SetModel只影响下次invocation；每层next恢复canonical模型/State/RunID/workspace/bus，InvocationSessionID绑定本轮log。
ModelWrapper next只在本次wrapper开放窗口内准入，return后拒绝late next并等待已进入调用真实退出；允许顺序多next/私有缓冲采样，每层及base拒并发。逻辑流一旦公开chunk，后续base或wrapper失败粘性保留，不能重试混成成功（包括先公开固定前缀）。无公开输出的失败可重试，每个actual Stream独立关闭callback，旧尝试迟到回调不能进入新尝试。
每轮绑定当时实际log；中途更换Session只影响下一轮，当前模型/工具事实始终在原log配对settle；所有触及log均Flush。
log去重检查实际reflect.Value.Comparable，不以Type.Comparable误判含interface字段的值；不能证明身份的合法value log保守保留并Flush，不能在取消收尾时panic。
洋葱顺序保持现行 Rust 语义：OnRunStart/BeforeModel 按注册 outer→inner，成功响应的 AfterModel 和
OnRunEnd 按 inner→outer；WrapTool 按 outer→inner 进入、inner→outer 展开。失败模型流没有完整响应，
不调用 AfterModel（与 Rust after_llm_call 相同），结束 hooks 仍反向执行。
本层仅执行RunConfig给定的middleware顺序，不识别Priority/Configure；LoopDetection等具体实现不再暗带排序，需由调用方装配。
取消/下游失败后，未执行工具得到明确 cancelled outcome；已执行但结果记录失败保持 unknown 并返回错误，绝不假称未执行。
Tool/WrapTool 真正返回 error 表示执行/基础设施/授权中止：先提交当前失败证据，再补齐剩余未执行调用并结束，
不自动再调模型。只有正常返回 `ToolResult{IsError:true}` 的业务结果允许模型继续处理；取消与执行错误同时出现时
保留两者返回证据，不用 context.Canceled 覆盖真实失败。
未知tool是安全IsError反馈；只有BeforeTool明确ToolBlockedError是模型可恢复veto，其他hook/wrapper错误仍中止。
纯veto也统一逆序AfterTool；mixed Join(veto,session/infra/cancel)不是纯veto，保留真正错误并停批。
每个Tool wrapper的next只允许零次或一次，每层与base都拒绝重复/并发进入，违规不能被吞；返回即关闭准入并同步等待已进入next真实退出，拒绝迟到执行。
内层next真正error不可被外层包装吞成成功，但正常result的短路/变换仍保留。ToolFunc不能在窗口外保存后调用。
退出用独立五秒 cleanup context 执行结束 hooks 与 Flush；失败不能发成功终态。

## 约束
ToolBatchCoordinator 将显式ConcurrentSafe且无规范化资源冲突的工具放入有界wave；SerialGlobal为屏障，Coordinator可参与wave。
开始事件/intent与outcome按模型顺序，执行可并行；等待所有已开始调用真正退出，失败兄弟也各自记录实际结果。
取消不等于执行停止：工具忽略取消并正常返回时保留成功结果，run仍返回取消；真正失败+取消是Failed，保留两者证据。
模型响应有显式字节上限和 finish 验证；吞掉 callback error 也不会授权工具。
Run callback 必须有界/支持调用方取消；本层不偷偷启动异步回调 goroutine。
取消时 Run 的返回值是终态权威，不能靠取消的订阅强行投递终态。`BoundedEventSink` 的成功发送不丢弃，
队列满背压，取消/owner Close 可解除阻塞，Close 后已接受事件仍可 drain。
完整assistant响应事务成功后发MessageCommitted的独立deepcopy及实际RecordReference；不读全量历史、不把stream delta/partial/failure冒充完整答复。投递失败先补齐未执行工具outcome再结束。
ContextRuntime协调七hooks和独立ContextApply，只改变provider工作集；Load接独立ContextSource并传真正reader-only绑定，来源错误不静默fallback。无source才调用完整Messages；不修改ConversationLog耐久事实。
通用预算prepare是soft projection。可选ModelAdmissionHook在每次captured LanguageModel.Stream前检查全部wrapper后的实际request，各检查器得到独立深拷贝；拒绝不触发本次模型IO，修改请求后必须再次准入。核心不持有token估算/媒体/权限政策，此边界不覆盖组合器内部opaque transport或缓存数据访问授权。
模型wrapper的Tools/Config传输投影不自动改写logical RunConfig+BeforeModel的工具执行许可；多尝试输出来源不通过现有任意Go回调自动证明。
ToolTimeout同步等待实际退出；Dangling只补工作集未知结果、不重放；LoopDetection使用AgentState typed extensions，实例可共享。
ModelConfig每次调用deep clone；disable/allowlist既过滤schema也限制实际工具调用。模型完整块通过共享StreamAccumulator保持顺序/签名/媒体元数据。
middleware各实际hook以阶段打包为一条alva.kernel.audit/middleware_hook_batch，保留run/call、实际layer index、进入/展开顺序、inclusive耗时及固定错误类别；无prompt/result/原始private错误正文。before/after/start/input/end与prepare/wrap都覆盖，未实现的可选Go接口不虚构调用。完整tool outcome先按模型顺序真实settle，再append其阶段审计；audit失败保留typed SessionError并补齐剩余调用，绝不重执行或标NoSideEffect。取消用独立5秒cleanup保存证据；已有tool/end cleanup不重置deadline。
LLM审计在BeforeModel之后capture实际模型、与intent同事务提交versioned cache-base/tool-schema指纹；base仅configured稳定system+实际identity+有序effective schemas，实际prepared system另存hash，动态system/history不混入稳定base。wrapper仍可retry/改transport，审计明确是logical request的model接口观测。每个响应首输出/流摘要最多各一条，尾部合批提交，不逐token写盘；只含计数/时间/固定outcome与真实usage的hit/write/miss/unreported，不含正文/私有错误。记录失败保留SessionError、结算model并拒绝Tool执行。Phase其余timeline observation仍未逐项等价，不宣称完整Rust表面已迁完。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Execution | `loop.go` | AgentLoop、默认循环、耐久 intent/outcome 与 settlement |
| Log identity | `log_identity_test.go` | 含map/slice动态配置的value log可正常取消/Flush，可比较实际handle继续去重 |
| Model invocation | `model_invoke.go` | canonical身份、顺序next/逐尝试callback lease、公开失败保护与最终只读准入 |
| Tool waves | `tool_batch.go` | 冲突感知并行与模型顺序settlement |
| Tool preparation | `tool_prepare.go` | intent前改参/替代结果与失败批次原子结算 |
| Frozen execution | `tool_execute.go` | post-intent参数不可变与真实execution authority绑定 |
| Hook audit | `hook_audit.go` | typed阶段批次、实际顺序/inclusive耗时、固定错误类别与真实settle后耐久审计 |
| LLM audit | `llm_audit.go`, `llm_audit_test.go` | versioned稳定base/实际schema与prepared system指纹、cache presence分类与无正文有界stream观测 |
| Context lifecycle | `context_runtime.go` / `context_runtime_test.go` | 七hooks/prepare/budget、只读Source选择/归属/失败不回退与deepcopy |
| Hygiene | `builtins.go`, `loop_detection.go` | 同步超时、dangling修复、运行态循环检测 |
| Event transport | `events.go` | BoundedEventSink 的 lossless/backpressure/cancel/close |
| Event regression | `events_test.go` | 1000 事件、满队列取消与并发 shutdown |

直接DefaultAgentLoop入口在任何lifecycle/emit之前拒绝未初始化state、nil/typed-nil middleware和无效Context。ContextRuntime所有生命周期入口（含Bootstrap失败后的Dispose）统一拒绝typed-nil required hooks/handle及已提供的optional apply/counter，不依赖Agent装配层代为兜底。context_runtime_test.go以实际修前panic和side-effect计数反例覆盖，避免在清理阶段再次调用已拒绝的missing hook。
