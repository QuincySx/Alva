# L0 · LLM 纯线格式
> 模型无关的自有值类型，不是模型服务实现或协议转换器。

## 地位

可移植 SDK 最底层；kernel、provider 与 WASM guest 共用同一份 wire。

## 逻辑

只表达 Message/ContentBlock/ToolDefinition/Usage、引用与媒体来源、流式累加和有界 LLM proxy DTO。原生协议适配及 Magpie 转换位于 integration/llm，不能反向进入此目录。

## 约束

无 HTTP、文件读取、凭据、授权或产品存储；Reference 仅是引用，不意味着自动加载。缓存 absent/null 与明确零不混淆，reasoning 是 inclusive output 的子集。

## 业务域清单

| 目录 | 职责 |
|---|---|
| `wire/` | L0 自有消息、工具定义、模型配置、媒体/处理来源、stream/proxy 纯契约 |
