# alva-hub-server
> Hub Desktop 产品能力的本地 headless HTTP/SSE 入口。

## 地位

位于 L7，与 Tauri Desktop 共用 `alva-hub-desktop` 业务库。它只替换传输层，
不复制 HubStore、engine assembly、permission 或 Inspector 领域逻辑。

## 逻辑

1. HTTP uplink 提供 session、provider、run、switch、permission、memory 与 Inspector 请求。
2. SSE downlink 使用与 Tauri `hub_timeline` 相同的稳定 TimelineEvent wire shape。
3. `HubInspectorService` 与 Alva runtime 共用 native repository；`GET /api/sessions/:id/inspection` 返回和 Tauri command 相同的稳定 DTO。
4. 静态 fallback 提供构建后的 Hub React UI，使浏览器和 Desktop 使用同一前端。
5. ALVA 装配使用已认领产品会话的共同 context/media Plugin 配方；检索/派生/选择/receipt 复用共享 HubStore 和既有审批，服务不复制业务逻辑或控制外部 engine 模型循环。

## 约束

- 不向浏览器返回 provider secret 或 raw adapter stream。
- 浏览器与 Tauri 的 HubSession、TimelineEvent、Inspector DTO 必须同构。
- 服务默认只绑定 loopback；改变绑定范围需要单独安全审查。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Server source | `src/` | HTTP/SSE 路由、Hub product state 与静态资源服务 |
| Package manifest | `Cargo.toml` | headless server binary 与传输依赖 |
