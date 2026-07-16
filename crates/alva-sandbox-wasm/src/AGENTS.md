# alva-sandbox-wasm/src
> Native WASIp1 process runner与版本化 blocking LLM/fetch guest-memory bridge。

## 地位

宿主平台边界源码；只负责 WASI capability/执行和 L0 proxy ABI mechanics，不拥有 provider policy。

## 逻辑

`lib.rs` 创建启用 epoch interruption 的 engine、带 memory limiter 的 store、linker/WASI preopens 并捕获 outcome；`llm_proxy.rs` 注册 callback 驱动的模型桥；`escalation_proxy.rs` 注册升级桥并按 grants 翻译 guest cwd；`log_proxy.rs` 注册 callback 驱动的 audit event 桥；`http_proxy.rs` 注册宿主策略驱动的 fetch 桥。

## 约束

- 每次 run 必须使用 fresh Store/WasiCtx。
- 所有 guest memory range、ABI version 与 byte size 必须先验证再读写。
- callback 不得在本层绑定具体 LanguageModel/provider/key。
- escalation callback 不得在本层绑定 PermissionMode、SecurityGuard 或进程执行；cwd 翻译必须 canonicalize 后验证仍位于 host grant。
- fetch 必须在每次发包前校验当前 URL，HTTP client 不得自动跟随重定向。
- 每个 store 必须挂 `RunLimits` 对应的 epoch deadline 与线性内存 limiter。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|-------------|------|
| Runner | `lib.rs` | preopen、WASI command 执行与 stdout/stderr outcome。 |
| LLM proxy | `llm_proxy.rs` | versioned ptr/len request/response memory bridge。 |
| Escalation proxy | `escalation_proxy.rs` | versioned ptr/len bridge 与 grant-scoped guest cwd 翻译。 |
| Log proxy | `log_proxy.rs` | versioned、bounded、单向 audit event memory bridge。 |
| HTTP proxy | `http_proxy.rs` | versioned fetch bridge、域名白名单与逐跳重定向策略。 |
