# 横向支撑 · LLM Integration
> 原生模型薄适配、真实跨协议转换与 HTTP 实现，不混入 L0 wire。

## 地位

SDK 外侧的模型实现；纯消息/媒体/Usage 契约唯一位于 sdk/llm/wire。codec/provider 不能成为 kernel 或 Agent SDK 的直接/传递依赖。

## 逻辑

ProtocolAdapter 直接编码/解码 native，跨协议才走 Magpie；HTTP/auth 只由 provider 的共享 RawTransport 拥有。harness/Gateway 显式选择实际 route；不复制模型协议 match 或默认发现用户凭据。

## 约束

仅 codec 可以引用私有 Magpie 转换切片；禁止第三方 DTO 进入 SDK，禁止 codec 获得 HTTP/文件权限。same-protocol 不经 Chat 降级，损耗/截断/取消明确失败。媒体输入必须由上层显式选中，引用不自动展开。

## 业务域清单

| 目录 | 职责 |
|---|---|
| `codec/` | 四 native 薄桥、真实 Magpie 跨协议、精度/续传/终态/cache presence 检查 |
| `provider/` | 显式端点/认证与完整有界 SSE framing，供 SDK model 与独立 Gateway 共用 |
