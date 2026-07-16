# alva-llm-wire
> 零 workspace 依赖、wasm-clean 的 L0 LLM 线格式与跨 runtime 纯值协议。

## 地位

所有 kernel/provider/guest 可共享的 serde 值类型住在这里，不依赖 agent loop、provider HTTP 或宿主能力。

## 逻辑

1. message/content/stream/config/tool 模块定义 protocol-neutral wire 值。
2. adapter 模块负责 Anthropic/OpenAI/Gemini 线格式转换。
3. stream accumulator 把 provider events 统一收敛为 assistant message。
4. wasm proxy 模块定义 guest/host 同源的版本化、限额化 JSON DTO。

## 约束

- 不得依赖其他 workspace crate。
- 公共累加语义以 kernel 已有生产行为为兼容基准。
- proxy DTO 不得包含 provider credential 或宿主路径。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| crate 配置 | `Cargo.toml` | serde/uuid/chrono 等 dependency-light 依赖。 |
| wire 源码 | `src/` | 消息、流、adapter、累加器与 wasm proxy DTO。 |
