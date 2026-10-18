# L6 · Independent model gateway
> 使用真实Magpie的显式模型协议入口，不是第二套Agent或Hub执行器。

## 地位
L6 application HTTP facade；仅codec可接私有Magpie IR，HTTP/auth由共享native边界与provider拥有。
不依赖Hub、desktop、旧Rust服务或new-api主模块。
## 逻辑
固定实际loopback与带外256-bit Bearer；1..16显式alias→actual upstream Model/endpoint/account。
每请求独立Exchange/Meta，真实转换Steps/Quality及metadata SHA可送可信host observer，不记录正文/key。
请求/非流式响应/流式事件均由codec转换；实际源终态后才Finalize。断流/损耗不补成功，不重试。
## 约束
8并发、8MiB request/response/stream、30s单次write deadline、10min请求owner timeout。
Close取消并等待真实handler，超时保留owner可重试。无外部tool执行、凭据发现、上传或媒体处理。
Gateway是认证调用方显式提交的模型API，不是会话上传入口；不会自动读取/解析host文件或URL。
不跟随重定向、不转发客户端headers/key；provider固定route决定唯一上游认证。
model/stream控制字段严格大小写，不接受DTO大小写别名导致准入后切换操作。
原始流错误不泄露私有body、endpoint、key或转换诊断正文；无隐式CORS/query token/forward信任。
## 业务域清单
| 文件 | 职责 |
|---|---|
| server.go | 显式route装配、request准入/JSON/Magpie Exchange与nonstream response |
| stream.go | 有界背压SSE、真实终态与协议error envelope，不用EOF造DONE |
| server_test.go | 真实local upstream/Magpie/HTTP，auth/转换/调用身份/取消/损耗/断流回归 |
| local.go / local_test.go | 可嵌入的cold route快照、显式同意后真实loopback监听与一次token、独立Stop/重启rotate、真实请求取消/join和超时保留owner |
