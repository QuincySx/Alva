# L4 · Web Extension
> 显式网络授权、真实 HTTP 来源和按需正文引用。

## 地位

可选 Plugin；不自动启用网络、服务商、代理、凭据或媒体理解。

## 逻辑

run取消从实际准入起覆盖审批、risk callback与请求，Close等待原gate内的事实提交结束；参数拒绝重复键/非法Unicode，CallID非空。实际 Tool/session/cwd/run 与 Guard 审批通过后，risk callback 和原生 intent 先落，再请求网络。DNS 全答校验并连接固定 IP；初始私网仅宿主明确 origin 授权，所有 redirect 严格公网。完整 10 MiB 内响应、HTML 文本和实际来源各自留同会话 artifact；模型只拿小 receipt/ref。

## 约束

默认无 Plugin；搜索只注册显式配置的 DuckDuckGo Instant Answer/RelatedTopics 服务，不冒充全网搜索。HTML5 tokenizer 仅文本提取，不称 Markdown/reader-mode。无 cookie、ambient proxy/key、自动 retry 或正文自动入模。HTTP 非 2xx 保留真实来源并 IsError，大小/记录/取消失败不能成功截断。单请求一分钟、五次 redirect、64 次/轮、10 次/host/分钟、50 项仅 metadata/ref 缓存；缓存与 rate 不跨 session。OS 网络 sandbox 与完整任意 secret 检测不在承诺内。

存储只消费本包按需组合的共享storage能力插口；会话/引用/DTO/错误从同一契约返回，不导入具体session.Repository或SQL实现。后端由上层程序/插件装配选择，组件不回落默认库、不downcast实现，不持有存储Close。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 装配 | `plugin.go` | 显式服务/权限、实际生命周期与证据存储 |
| 网络 | `network.go` | 私网/redirect/DNS-pinning/响应硬界 |
| 内容 | `content.go` | 完整来源脱敏、HTML5 文本、实际 API 解析 |
| 工具 | `tools.go` | read_url/可选 internet_search 的真实消费 |
| 准入边界回归 | `boundary_test.go` | 实际审批/risk取消、Close超时保留及取消后零网络请求 |
| 回归 | `plugin_test.go` | 实际 HTTP/SDK 引用、真实 API/嵌套 HTML、私网/redirect/溢出/取消与 risk 前置 |
