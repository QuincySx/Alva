# L0 · sdk/llm/wire
> 无运行时政策的模型消息、工具调用和流式值类型。

## 地位
L0；仅依赖标准库，不认识 Agent、产品会话、Provider 或 UI。
ToolDefinition是真实L0 schema值，与kernel的可执行Tool区分；kernel ModelRequest/Registry与sandbox代理直接消费wire类型，无kernel/sandbox同名alias或复制契约。

## 逻辑
`Message` 使用互斥 `ContentBlock` union；工具调用参数保留原始 JSON，工具结果绑定 call ID。
引用只说明存储位置与真实处理信息，不触发内容读取。Provider 先有界组装 tool delta，再产出完整调用。
StreamAccumulator按实际流顺序保留完整内容块；ReasoningDelta仅瞬态进度，完整Reasoning的签名/隐藏数据与ProviderState必须原样留存。
有ContentIndex的authority按canonical slot排序/同slot text拼接，不按并发arrival order重排；混用indexed/unindexed内容明确失败。
slot索引<1<<20、每响应最多4096个不同slot/完整block；usage/progress/finish不创建authority槽，拒绝的超预算正文不进入partial snapshot。
ToolCall.ProviderState可将Gemini等续接数据绑定实际call ID；Message保留ResponseID/FinishReason/Usage。
Usage缓存读取/写入计数为可选值，nil是服务端未报告、显式0是报告零；JSON/stream/clone保留presence，不能从有损归一化的0猜造。
Usage规范化输入包含cache/read/write与已报告tool-use prompt；输出包含reasoning，ReasoningTokens是子集而非额外相加，跨协议回原生需要反向拆分已知部分，不伪造未知split。
Image/Audio/Video是显式选中投递的bytes或HTTP(S) URL，与Reference隔离；MediaProvenance标明原来源与真正操作/帧时间。

## 约束
成功模型流必须显式 `ChunkFinish`；纯 EOF 不代表完整响应。所有跨持有边界的消息/参数/嵌套结果
用 deep clone 隔离。引用优先策略不属于本层；不提供自动读文件、媒体转换/理解、URL fetch或历史兼容parser。
StreamChunk按Kind独占text/call/content/usage载荷，ResponseID/FinishReason只在finish携带；额外非空载荷硬失败，不能静默丢弃调用、usage或续接身份。
ModelConfig区分nil/空allowlist，typed可选采样/上限/推理与object ExtraBody；provider需明确支持或拒绝内容/选项，不能静默丢弃。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Wire contract | `message.go` | Role/content/tool/ref/usage/chunk、校验与 deep clone |
| Tool schema | `tool.go` | 模型可见纯ToolDefinition值，不包含执行授权/资源或注册实例 |
| Regression | `message_test.go` | Union 互斥、嵌套协议拒绝与所有权 |
| Usage presence | `usage_test.go` | 未报告/显式零的JSON与所有消息/stream拷贝边界 |
| Ordered stream | `accumulate.go`, `accumulate_test.go` | 有界assembly、终态校验、所有完整blocks与续接state |
| Media types | `media.go`, `media_test.go` | 显式payload/帧投递、真实处理来源、deep clone |
| Request config | `config.go`, `config_test.go` | per-request可选设置、reasoning与allowlist语义 |
| Indexed stream regression | `index_test.go` | 迟到call顺序、mixed mode、完整slot唯一、空chunk硬限与index clone |
