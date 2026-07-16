# alva-llm-wire/src
> LLM 纯值线格式、协议 adapter 与共享流/wasm 边界实现。

## 地位

本目录是 L0 wire crate 的全部实现，可被 kernel、native host 与 WASI guest 直接消费。

## 逻辑

各值类型模块保持纯 serde；`accumulate.rs` 统一增量事件收敛；`wasm_proxy.rs` 统一 blocking proxy DTO/version/limits。

## 约束

- 每个模块不得引入 runtime、网络或 workspace 高层依赖。
- tool call 的 id/name/arguments 合并顺序必须保持 provider 兼容。
- ABI 版本或限额变更必须同时覆盖 host 与 guest 测试。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| crate 入口 | `lib.rs` | 声明并 re-export wire public API。 |
| 流累加 | `accumulate.rs` | kernel/guest 共用的 StreamEvent → Message 逻辑。 |
| wasm proxy ABI | `wasm_proxy.rs` | request/response DTO、version 与 byte limits。 |
| 协议 adapter | `adapter/` | Anthropic/OpenAI/Gemini 编解码。 |
| 基础值类型 | `message.rs` 等 | message/content/config/stream/tool payload。 |
