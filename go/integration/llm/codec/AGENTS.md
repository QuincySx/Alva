# 横向支撑 · LLM Codec
> 纯原生 JSON/stream 适配与真实 Magpie 跨协议转换。

## 地位

仅依赖 stdlib、L0 wire 与私有 pinned Magpie 源码切片（内部仅额外消费 gjson）；无 kernel runtime、harness、platform、HTTP、凭据、磁盘或网络权限。Magpie IR 和业务账户概念不进入公开 SDK。Magpie 来源/许可见 internal/magpie，未依赖完整 magpie module。

## 逻辑

`ProtocolAdapter` 是 own wire ↔ 四个 native protocol 的独立薄桥。Agent 同协议输入不绕 Chat，保留已实现的原生签名、opaque provider state、拒绝、内容顺序和 usage。协议转换由单请求 `Exchange` 真正调用 Magpie，暴露 own Result/Steps/Diagnostics；same-protocol 直接保留原始 JSON 字段，仅修改实际 model/客户端 echo。

每个 Exchange 私有 request/namespace/custom/tool-search 状态、一次 Request，随后一次 Response 或 ResponseStream。Model 仅发 upstream actual model；ResponseModel 默认 Model，仅用于下游 model echo；Gemini modelVersion 是真实来源，不伪造别名。source stream bool 必须与显式选项一致。

传输负责 SSE framing；decoder 消费完整 Event，Push 不释放 ChunkFinish，Finish 必须先观察实际合法 native terminal。跨流转换将成功终态留到 source Finish 后，不能以 EOF 或 encoder.Finish 伪造成功；已经收到的普通首文本仍可及时投递；工具及其后的内容按实际 native 内容索引校验后有序发出。Gemini STOP 后允许真实尾 usage-only 帧。

## 约束

- 原始/派生输入有界：8 MiB 请求/响应、1 MiB event、128 tool calls、256 KiB/call arguments；显式 Limits 只能收紧。JSON 验证 Unicode、重复 key、128 层深度，错误 sticky、取消/consumer failure 不丢失。
- 本层在实际 Magpie parse/build 后复核 request intent、工具/schema/控制/预算与已知变体，未保留语义返回 LossError；签名/加密续传/refusal 另有门禁。工具名扁平化冲突明确拒绝，同协议不因此降级原生状态。
- 无 global media resolver、URL fetch、引用自动展开。需要 resolver 的跨媒体路径明确不可用；own-wire 明确选中的媒体由 native adapter 按既有来源说明编码，不代表 codec 做过压缩/抽帧。
- 绝不从 normalized numeric zero 推断 cache 是否报告；原生 raw absent/null vs explicit0 保留，Responses cache-write 唯一字段 `usage.input_tokens_details.cache_write_tokens`。不支持 cache-write 的目标明确拒绝；Gemini 尾帧更新 held terminal 的最后 usage。
- 原生 complete gate 验证对象、成功 status/finish、单 candidate/choice、完整 function arguments；不是所有供应商 schema 的完整验证器。未知原生字段 identity 保留，cross 能力以真实转换及 fail-closed 支持范围为准，不宣称任意 provider 内容全兼容。
- 无全局日志/codec/media hook；公开转换错误固定文本，私因仅 Unwrap，不做正文日志。响应只通过唯一 native decoder，再投影到 Magpie IR；不把文本标记当作思考/工具，不重复维护四份响应 decoder。
- 无旧 provider decoder/serializer 的生产 compatibility shim；serializer/media 回归已归纯层，HTTP 回归仍在 provider。
- 跨协议工具参数含不能经 float64 JSON roundtrip 精确保真的整数/高精度小数时明确 `LossError`，不构造舍入后的调用；request、complete 与流中实际完整工具调用均检查。DTO schema 大小写 alias 明确不支持，不能绕 continuation/control gate；工具 input/schema/metadata 的任意业务 key 不折叠。
- Native Gemini 用量转 own wire 时 Input=prompt+toolUsePrompt、Output=candidates+thoughts，Reasoning 是 Output 子集；均保留 presence 的零覆盖并检查加法溢出。跨回 Gemini 才还原 candidates=Output-Reasoning，不伪造未知 tool-prompt 拆分。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| 纯契约 | `types.go` | ProtocolAdapter、Request、Event/Limits/decoder、own diagnostics/Result |
| 原生适配工厂 | `adapter.go` | 四协议独立 native 编码、工具 schema Unicode 预检 |
| Chat 编码 | `encode_chat.go` | 原生 message/tool/reference、拒绝/思考顺序 |
| 其他原生编码 | `encode_native.go` | Responses/Anthropic/Gemini 独立请求；Responses opaque reasoning 仅验证 canonical type 后克隆 RawMessage，未知精确数字不经过 any |
| 配置编码 | `encode_config.go` | output/sampling/thinking recipes、显式 shallow overrides |
| 显式媒体编码 | `encode_media.go` | 选定图像/音频/视频、精确帧时刻和处理来源，不读取/转换字节 |
| Decoder 基础 | `decoder.go` | 限额、sticky error、真实终态、唯一 Finish、内容/调用小身份验证 |
| Chat Decoder | `decode_chat.go` | delta/tool arguments、[DONE]、完整 finish/usage |
| Responses Decoder | `decode_responses.go` | item/call 身份、原始 reasoning、completed envelope |
| Anthropic Decoder | `decode_anthropic.go` | block/thinking signature、累计 usage/cache |
| Gemini Decoder | `decode_gemini.go` | STOP、签名→实际 call、尾 usage-only 与 response identity |
| Exchange | `exchange.go` | 每请求私有 Magpie request 与逆向工具映射、request/complete response、model/echo 分工 |
| Stream converter | `stream.go` | 唯一 native decode → Magpie encoder、同步背压和终态暂存 |
| 转换公共 helpers | `conversion.go` | 真实两阶段转换证据、固定错误、原生 model/foreign provenance 投影 |
| 请求损耗政策 | `magpie_policy.go` | 原请求/目标 intent、工具/内容变体、精确预算/约束与命名冲突 |
| 响应 IR 桥 | `magpie_response.go` | native 权威内容/顺序/用量 → Magpie IR；增量首文本、完整已验证工具与尾内容 |
| 吸收实现 | `internal/` | 私有 Magpie 源码、MIT 许可/完整来源与修改清单，无应用 runtime |
| Magpie 迁移验收 | `magpie_test.go` | custom/namespace/tool search 完整与流式逆向映射、并行调用、无 ID Gemini 历史单次绑定、字面文本、状态隔离与损耗反例 |
| 严格 JSON | `json.go` | 重复 key/Unicode/深度/尾值校验 |
| 完成证明 | `complete.go` | 非流原生成功状态、完整函数调用和限额 |
| 状态损耗门禁 | `continuation.go` | 库未报告的 signed/encrypted continuation/structured refusal 预检 |
| 原始参数门禁 | `request_tools.go` | 跨协议历史 arguments 不允许被库重写成空对象 |
| 参数数值保真 | `numbers.go` | 递归精确十进制 roundtrip 检查，无法保真明确 loss |
| Schema key 门禁 | `keys.go` | 实际 DTO 字段与 native polymorphic content 的非规范大小写拒绝，不改业务 map |
| Cache presence | `cache.go` | 原生 raw presence→目标唯一原生字段 |
| 最后用量 | `usage.go` | 真 source usage→held terminal，cache-inclusive 输入换算 |
| Native 回归 | `decoder_test.go` | 四协议终态、bounds、Unicode、缓存 presence |
| Native 深度回归 | `decode_native_test.go` | 签名/排序/迟到帧、回调/取消、真实 cumulative usage |
| 配置回归 | `encode_config_test.go` | output/thinking/sampling/allowlist/shallow override |
| 媒体回归 | `encode_media_test.go` | 精确选中字节/帧/来源、不支持媒体明确失败 |
| 编码 fixture | `encode_fixture_test.go` | 纯测试 defaults 合并，无生产兼容入口 |
| Exchange 回归 | `exchange_test.go` | 16 native complete 路由、真 converter/Steps、opaque/别名/损耗/cache |
| Stream 回归 | `stream_test.go` | 16 路目的 decoder 真完成、截断不成功、private state/尾 cache0 |
| 保真反例回归 | `fidelity_test.go` | 超大整数/小数、原生 opaque reasoning raw 精度与克隆、split tool arguments、case alias/refusal、Gemini thinking/tool-prompt/zero/overflow 与反向子集 |
