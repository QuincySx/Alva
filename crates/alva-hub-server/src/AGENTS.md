# alva-hub-server/src
> 单文件 Axum 产品传输层。

## 地位

复用 `alva-hub-desktop` 的 store、engine、permission、timeline 与 Inspector 服务；
不拥有另一套业务状态机。

## 逻辑

1. `AppState` 初始化私有 HubStore、ProviderCatalog、共享 Alva Inspector repository 与 live engine registry。
2. JSON handlers 把 HTTP 请求转发到稳定产品 API；运行事件通过 SSE 广播。
3. Inspector handler 按 product session 读取 durable native snapshot，wire shape 与 Tauri command 一致。
4. 未命中 API 的请求落到内嵌 React 静态资源。

## 约束

- handler 不复制 Hub 领域判断。
- 错误响应不泄露 secret、provider response body 或 raw adapter stderr。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| HTTP service | `main.rs` | AppState、API/SSE routes、Inspector endpoint 与静态 fallback |
