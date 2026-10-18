# 横向支撑 · Provider
> 显式 HTTP transport 与 SDK native model 薄桥，不拥有协议转换政策。

## 地位

LLM 平台能力层；消费 portable codec 与 kernel contract，不向 SDK 注入产品/认证/重试政策。通过 codec 使用吸收的 Magpie 转换切片，无 Magpie 依赖。

## 逻辑

Canonical `Build(kind, config)` 与 `BuildTransport(kind, config)` 共用一个显式配置、端点与认证路径。前者用对应 native ProtocolAdapter 编码/解码；后者供实际 Gateway raw request/response/stream 消费，不经 own-wire roundtrip。旧重复 serializer/四 native decoder 已移除，无生产兼容层。

RawTransport.CompleteRaw 返回有界 JSON 对象，StreamRaw 返回完整 SSE Event；bool callback 可在调用方确认合法终态后停止，任何已观察的 read/callback/cancel error 仍失败。transport 不生成模型 Finish；截断判定留 codec。Gemini SDK 路径在 STOP 后继续到有界 EOF，保留真实尾 usage-only 帧。标准 Gemini operation 在同一 pinned origin/model path 切换 generateContent/streamGenerateContent，stream 加 alt=sse；custom endpoint 不猜路径。

## 约束

默认transport禁用ambient HTTP(S)_PROXY；HTTPClient未指定Transport时也沿用该默认；代理仅由显式HTTPClient.Transport提供。大小写折叠后重复的header拒绝，避免认证随机覆盖。无 ambient credential/config、无 redirects，HTTP 仅 loopback，其他端点必须 HTTPS。显式 API key 不可序列化。响应和错误不公开私有 body/URL；取消关闭 HTTP 请求，无自动 replay/retry。调用方若显式提供 HTTPClient，仍使用其 transport，但强制禁止 redirect。

传输限额：请求/完整响应/stream 原始字节各 8 MiB，SSE event 最大 1 MiB，可收紧；SSE LF/CRLF/standalone CR 正确分帧，comment/CRLF 原始字节同计预算，EOF 未关闭帧明确失败。codec 另拥有工具参数/内容/原生终态验证。

媒体适配不等于产品处理：这里不读文件、不展开引用、不压缩/抽帧/转码、不上传。8 MiB 模型请求限额独立于 20 MiB 原始附件限额；产品须先做预算/投递回执。所有测试只用离线/local httptest，无真实账户兼容承诺。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Native model 薄桥 | `openai.go` | NewOpenAIChat、真实 native codec→RawTransport→decoder、SDK唯一Finish |
| Canonical 工厂 | `protocols.go` | Kind/Build/四协议 constructors，独立 adapter 不降级 |
| 原始 HTTP 传输 | `transport.go` | Config/BuildTransport、CompleteRaw/StreamRaw、认证、严格有界 SSE/取消 |
| Typed HTTP 失败 | `errors.go` | status/credential/rate-limit/network 证据、bounded Retry-After，无私有 body/URL |
| Chat HTTP 回归 | `openai_test.go` | 真 local POST、错误/redirect/取消、完整碎片工具调用 |
| 多协议 HTTP 回归 | `protocols_test.go` | 三 native 实际请求/签名 continuation/终态错误 |
| 原始传输回归 | `transport_test.go` | Gemini 双 operation、complete JSON/限额、done+双错误、精确 CRLF 预算 |
| Test-only codec 桥 | `codec_fixture_test.go` | 旧测试 fixture 直接消费真实提取后的 adapter/decoder，无生产 shim |
| 顺序回归 | `ordering_test.go` | 原始 block/call 顺序、unknown 内容明确失败、无成功前执行 |
| Cache presence 回归 | `usage_presence_test.go` | 四协议 raw absent/null vs explicit0/nonzero、尾0覆盖 |
| HTTP 错误回归 | `errors_test.go` | 分类/Retry-After/隐私/无自动重试 |
