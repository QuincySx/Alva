# alva-sandbox-wasm/src
> Native WASIp1 process runner与版本化 blocking LLM guest-memory bridge。

## 地位

宿主平台边界源码；只负责 WASI capability/执行和 L0 proxy ABI mechanics，不拥有 provider policy。

## 逻辑

`lib.rs` 创建 engine/store/linker/WASI preopens 并捕获 outcome；`llm_proxy.rs` 在同一 linker 上注册安全的 request/response memory bridge。

## 约束

- 每次 run 必须使用 fresh Store/WasiCtx。
- 所有 guest memory range、ABI version 与 byte size 必须先验证再读写。
- callback 不得在本层绑定具体 LanguageModel/provider/key。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| Runner | `lib.rs` | preopen、WASI command 执行与 stdout/stderr outcome。 |
| LLM proxy | `llm_proxy.rs` | versioned ptr/len request/response memory bridge。 |
