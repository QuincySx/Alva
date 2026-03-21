# adapters/llm
> LLM 提供商适配器实现

## 地位
`adapters/llm/` 是 `ports::LLMProvider` trait 的具体实现层，当前唯一实现为基于 rig-core 的 OpenAI 兼容适配器。

## 逻辑
`OpenAICompatProvider` 将 srow-core 的 `LLMRequest` 转换为 rig-core 的 `CompletionRequest`，支持同步完成和流式响应两种模式。通过 `CompletionModelDyn` trait-object 包装解决 rig CompletionModel 非对象安全问题。

## 约束
- 依赖 rig-core crate 进行实际 HTTP 通信
- 流式模式通过 `mpsc::Sender<StreamChunk>` 推送增量事件
- 支持 OpenAI、DeepSeek、Qwen 等兼容 API（通过 `with_base_url`）

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明 openai_compat 子模块 |
| openai_compat | `openai_compat.rs` | OpenAICompatProvider：同步 complete、流式 complete_stream、消息/工具定义格式转换 |
