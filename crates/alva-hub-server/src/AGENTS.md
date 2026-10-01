# alva-hub-server/src
> 单文件 Axum 产品传输层。

## 地位

复用 `alva-hub-desktop` 的 store、engine、permission、timeline 与 Inspector 服务；
不拥有另一套业务状态机。

## 逻辑

1. `AppState` 初始化私有 HubStore、ProviderCatalog、共享 Alva Inspector repository 与 live engine registry。
2. JSON handlers 把 HTTP 请求转发到稳定产品 API；运行事件通过 SSE 广播。
   SSE 原样保留产品 `message_position`（attempt scope + UTF-16 offset），不另造片段序号或按正文去重。
3. Stop（R51/R65）：cancel 成功后立即持久化取消事实（失败上抛 500，不静默）；SSE 的 Interrupted 终态由 run handler 在 outcome（真实流结束）后统一结算——`stopping` marker 新一轮开始时清理、ack 后 run 已结束则丢弃，不泄漏到同 session 下一轮；引擎自然完成的取消不追加 Interrupted。
4. Inspector handler 按 product session 读取 durable native snapshot，wire shape 与 Tauri command 一致。
   `/api/sessions/:id/tool-content/:content_id?chunk=N` 复用 HubStore，blocking worker 中读取一块 ≤16 KiB 已脱敏正文；越权/缺失均为 404。
5. 未命中 API 的请求落到内嵌 React 静态资源。

## 约束

- handler 不复制 Hub 领域判断。
- 错误响应不泄露 secret、provider response body 或 raw adapter stderr。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| HTTP service | `main.rs` | AppState、API/SSE routes、Inspector endpoint 与静态 fallback |
