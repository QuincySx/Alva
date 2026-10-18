# L3 · sdk/agent/analytics
> 可选 Agent telemetry 能力与Plugin；不是耐久审计/模型上下文通道。

## 地位
SDK能力库，仅标准库与agent/kernel/wire；不落OS文件，不选择OTLP/JSONL宿主政策。

## 逻辑
Buffer.Record验证并深拷贝后非阻塞有界入队，满载/无效事件计Dropped；Drain是显式消费者，不隐藏goroutine或IO。
Plugin提供typed AnalyticsSink并注册一个Middleware；session/tool耗时与成功LLM usage由真实hook采集，未知cost/provider留空。
观察顺序在Registrar.Middleware注册处显式5000指定，Middleware本身不带Priority/Configure。
缓存用量保留wire可选计数，未报告与明确0不同；middleware/buffer深拷贝计数指针，不推测cache outcome或cost。
模型记录仅成功流，与Rust原成功路径语义一致；stream失败不伪造成成功LLM事件。
WrapModel身份优先使用实际capture的InvocationModel；outer wrapper即使改State.Model也不能把下一目标误记为当前调用，阶段外nil才fallback State。
同一模型调用的session归属优先使用InvocationSessionID；wrapper修改State.Session仅影响后续轮次，不能把本轮telemetry归到下一会话；阶段外空ID才fallback。

## 约束
自定义sink必须遵守非阻塞契约，不能借telemetry失败中止Agent。事件不携带工具参数/正文，错误摘要仍需宿主按威胁模型脱敏。
记录是best-effort观察，不作为提交/权限/安全重放证据；durable事实仍属于ConversationLog。
Middleware不把run状态留在共享实例map；run-local开始时间存在实际AgentState typed extensions。

## 业务域清单
| 名称 | 文件 | 职责 |
|---|---|---|
| Buffer | `buffer.go` | 有界非阻塞sink、显式drain与丢失计数 |
| Plugin | `plugin.go` | 一份实际middleware与typed能力装配 |
| Regression | `analytics_test.go` | 入队所有权/满载与真实model-tool生命周期 |
