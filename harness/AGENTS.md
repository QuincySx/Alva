# L5 · Harness 默认配方
> Replaceable default harness policies over the standalone Agent SDK.

## 地位
Equivalent responsibility to BaseAgent, not kernel execution or Hub product-state authority.
## 逻辑
Builder registers explicit user Plugins and skips same-name defaults before SDK assembly. Memory consumes sdk/agent/memory.Service; security consumes sdk/agent/security through extension/security; product reference projection consumes sdk/agent/context capabilities. SessionPlugins consumes the supplied shared storage capability ports for exclusive run claims, interrupted-intent recovery, durable references and bounded history. Workspace flows into actual SDK execution context. No old Rust data/config import occurs.
Builder delegates generic middleware/context/iteration/prompt mechanisms to the SDK; DeclaredTools returns only actual explicit instances for host security/rebinding, not synthesized Plugin metadata. NativeChildFactory atomically commits an actual child and causal intent link, creates child-owned native history/references, and uses explicit per-instance binders for only the granted direct-parent tools. Child has no inherited Bus, transcript or parent MCP handles; recursive tools and model-stream budgets are separately installed.
Middleware的可选order原样交给SDK注册入口；native claim在装配处显式-10000先取实际log执行权，Pending以-500先于普通request投影。实现不再声明Priority或Configure。
Default tool-validation Plugin validates prepared arguments and frozen actual execution; same-name replacement follows the existing default policy. Its schema engine stays in extension/toolvalidation, with explicit numeric/dialect/size limits; the pure SDK remains policy-free and can explicitly install it.
显式配置ContextSystem时，按名默认context-budget Plugin在最终模型接口检查完整输入估计（含tools/后置注入），用户可同名替换。无Counter的JSON rune/4仅粗略估计，不是真实provider/多模态token保证；无有限预算不宣称硬限制。纯SDK显式注册BudgetAdmission。
ContextSystem setter复制配置描述符，使SDK上下文准备和默认最终准入共享同一份已接受政策；调用方之后修改原struct不能提高实际准入预算。服务实现本身仍按显式所有权共享。

SessionPlugins通过独立native-context-source Plugin贡献近期turn的有界工作视图；选择及省略提示属于本层，storage只做固定高水位的有界metadata/hash读取。SessionViews供已有显式执行claim的owner只安装context/history/refs。默认telemetry Plugin拥有可选诊断政策，按名可替换；诊断失败显式报告后继续，执行/恢复事实及Flush仍严格失败。原生工作视图不覆盖用户ContextSystem或child通信hooks。完整事实仍由原Session读取；工作视图超限或损坏时显式失败，不回退全量读取，不把省略notice冒充摘要。
SessionPlugins读取实际组件的record/artifact retention，提示词分别如实说明生命周期。SessionStorage/ChildStorage只组合共享插口，Session/References/ConversationTail返回值继续是接口；不导入具体存储、不选择默认库、不持有存储Close。
## 约束
Pending is opt-in, binds one actual session/run, and holds at most 128 messages/1 MiB with 16 KiB per message. Cancellation only retracts unread entries; committing/committed entries cannot be retracted. Commit failure is explicitly uncertain and never automatically retried. Queue itself is process-lifetime; only committed messages are durable through the supplied session. It injects after general ContextRuntime prepare, so its hard queue bounds are not a claim of a second whole-request context-window compression pass. No automatic completion follow-up loop is implied.
Default demo/live factories remain explicitly volatile unless supplied an actual session and SessionPlugins. References use the same supplied facts/artifact authority, not a second transcript. Large tool text and matched, already-executed historical arguments above 4 KiB become references only in model request copies; unresolved/current calls and original execution facts are unchanged. Native/volatile backends deduplicate repeated identical projection. SDK read/search tools have actual-session gates and bounded encoded output. Native metadata browsing never restores runtime; native_history requires the exact dispatched Tool/call and strict 4KiB JSON with exact schema fields, duplicate/Unicode rejection. Default security trusts only actual explicitly registered tools and harness-owned reference/history contributions; unknown contributions require explicit host approval. Host filesystem resolution/tool capabilities require explicit granted workspace and extension/files assembly, not a fake sandbox claim. API keys stay in model transport memory, never native store metadata. Media bytes may be retained but no processing or model delivery is implied. Full runtime history reads retain explicit budgets; bounded Inspector APIs are separate.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| BaseAgent recipe | base.go | Default-by-name Plugin assembly and retained Runner factories |
| Default roles | defaults.go | Distinct memory capability wrapper and honest system context |
| Reference policy | context.go | Product large-text/unselected-media DTO projection against explicitly bound SDK backends/tools |
| Media projection regressions | media_test.go | Tool media never auto-delivers or fetches URLs; complete owned references and fail-closed budgets |
| Session recipe | native.go | Actual-session run claim/recovery、独立bounded ContextSource、durable backend和bounded history |
| Native context policy | native_context.go / native_context_policy_test.go | Whole recent-turn selection and omitted-history wording over bounded storage primitives |
| Typed recovery evidence | recovery.go / recovery_test.go | Persist post-operation recovery facts independently of errors.Join text; failure stays fatal |
| Optional durable waits | durable/ | Pluggable CAS/claim storage, persisted waits/approvals/questions, explicit continuation and native cross-process no replay |
| Child resources | child_resources.go | Setup/recursive child resource ownership and physical cleanup retry |
| Native child recipe | child.go | Actual durable child lifecycle, exact-instance capability rebinding and fresh recursive assembly |
| Child regressions | child_test.go | Real three-generation SDK/SQLite loops, reduced grants, HITL, actual source identities and reference isolation |
| Pending recipe | pending.go | Opt-in bounded interjections, atomic next-model commit and explicit commit uncertainty |
| Pending regressions | pending_test.go | Atomic receipts, cancel, uncertainty/no retry, actual binding and byte budget |
| Demo fixture | demo.go | Real local model/tool/model loop, explicitly not a live provider |
| Native/recipe boundaries | boundary_test.go | Same-ID外部log拒绝、失败并行run不偷释放claim、identity callback的工具修改只影响下一Build |
| Harness regressions | base_test.go | Plugin override, shared middleware order/copy ownership, multi-turn facts and reference isolation |
| Validation regressions | validation_test.go | 默认坏参数不执行与同名默认Plugin替换 |
| Final budget acceptance | context_budget_test.go | 有限预算baseline、late wrapper拒绝实际IO、按名替换及配置描述符所有权 |
| Native regressions | native_test.go | Reopened durable turns, metadata browsing and actual-session ownership |
| Storage composition | storage_test.go | 同一实际SDK model/tool/model通过目录/临时内存取有界引用，事实保持及retention真实提示 |
| Native context regressions | `native_context_test.go` | Source与显式ContextSystem同时装配，保留AlwaysPresent政策并跑真model/tool/model |
| Argument regressions | arguments_test.go | Matched historical-call projection, exact current execution, unchanged facts, full retrieval and idempotent prepares |

Build先复制完整配方，再读取ID/Name；回调对原Builder追加工具/插件或修改政策只用于下一Build。NativeRunClaim核对实际repository/owner/session及run，失败并行run的OnRunEnd不得释放另一个run的claim；ContextSource仍保持窄ConversationReader契约，不为产品授权扩充SDK。
Builder在读取ID、插件name或默认替换前拒绝typed-nil model/log/plugin，identified log为空也不注册任何Plugin；base_test.go验证不进入实际默认装配。
NativeChildFactory仅接受动态值实际可比较的parent binding；grant比较检查两值，含interface map/slice的struct不能凭外层类型可比较获得身份或触发panic；identity_test.go验证拒绝边界。

Direct CLI及Hub的Alva配方可显式装配 durable Controller 并消费 Suspended；Hub将等待保留在同一个产品Turn，UI/HTTP不拥有第二执行器；等待是产品可选路径，普通 Runner 的 nil error 不代表等待已完成。
