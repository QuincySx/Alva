# LLM Codec · Absorbed Magpie conversion
> 固定 yetone/magpie 的 MIT 协议转换源码切片，不是完整 magpie 应用。

## 地位
固定 commit `e085d6e9262786815c0f6c5ff65c5a585916cc9e`。代码只供上级 codec 使用，原始路径/完整上游文件 SHA/保留声明和本地修改见 `provenance.json`；MIT 原文见 `LICENSE.magpie`。普通构建不下载、运行或导入 magpie 的应用模块。

## 逻辑
请求使用实际 Magpie parse → IR → build；custom/namespace/tool-search 逆向映射为每个 Exchange 私有状态。响应使用 Alva 唯一 native decoder 的权威内容，再经 Magpie IR/renderer/encoder 编码；不复制第二套原生 decoder。纯文本可增量投递，工具和后续内容先按 native content index 完整校验/排序，再发给 encoder；普通文本不会变成工具或思考。最终成功由上级实际源终态决定。

## 约束
- 只允许明确的纯标准库 helpers 和 pinned gjson，architecture 实际 import 图强制；不含 HTTP、OS 文件/环境、账号发现、设置、订阅、自动重试/路由、UI 或工具执行。
- SSE sink 同步调用上级 callback，错误 sticky，无额外队列/goroutine、全局媒体 resolver 或磁盘签名 cache；MIME 使用固定映射，不扫描系统 MIME 数据库。
- 保留外部协议的工具命名/参数语义，不提供旧 Alva API 或数据格式 alias。同协议不经过本切片，上级保留原始 JSON/opaque state。
- 上级损耗门禁拒绝无法保真的约束、签名/加密续传、未知控制/内容、参数精度和调用名冲突；不启用 schema→prompt、schema union flatten、文本→toolcall、think-tag 或磁盘 call-ID 隐藏状态降级。
- 本地政策/桥接文件是 Alva 源码，MIT 上游代码保留归属；不得因上游返回成功跳过 native 终态、限额、cache presence 或 usage 复核。

## 业务域清单
| 文件 | 职责 |
|---|---|
| `bridge.go` | Alva 私有入口、同步 framed-event sink、固定 MIME 提示与纯值 helpers |
| `upstream_additional_tools.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_advisor.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_anthropic.go` | Messages 请求解析/构建与响应 renderer/encoder |
| `upstream_anthropic_shapes.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_chat.go` | Chat 请求解析/构建与响应 renderer/encoder |
| `upstream_chat_tool_shape.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_chattidy.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_codeassist.go` | Gemini 纯请求构建、工具结果/JSON schema 和 reasoning 配置 helpers |
| `upstream_codex_backend.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_compacting.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_continuation.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_format.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_gateway.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_gemini.go` | Gemini 请求解析/构建与响应 renderer/encoder |
| `upstream_gemini_signature.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_ir.go` | Magpie 消息/工具/事件/usage IR 与 response collector |
| `upstream_maxoutput.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_message_tools.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_phase.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_prefill.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_responses.go` | Responses、custom tool、namespace、client tool search 的解析/逆向映射与响应编码 |
| `upstream_toolsearch.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_usage_merge.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_vertex.go` | 从同名上游文件抽取的纯协议/工具/格式 helpers；具体保留声明见 provenance.json |
| `upstream_whole_stream.go` | 完整原生响应展开为有界原生事件；只供现有 native decoder 消费 |
| `provenance.json` / `LICENSE.magpie` | 固定来源、上游文件哈希、声明与本地修改、完整 MIT 归属 |
