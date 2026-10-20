# L4 · Explicit media extension
> 原件引用、处理清单、显式选择与单模型请求投递分别记录。

## 地位
可选 native Plugin，不给 kernel/agent-core 增加媒体政策；可冷读 Library 不恢复Agent。

## 逻辑
真实owner/native绑定的input-attachment才能处理，派生字节/manifest/selection/receipt事件
均复用native artifact authority；模型收到小索引/引用，读取清单与媒体投递是独立动作。
未知route不猜vision；选择最多2个已验证JPEG，只在下次WrapModel请求副本附加，永不写入conversation。

## 约束
Tool、模型调用及最后receipt都登记在现有Plugin owner内，run/Close取消传播到审批和模型；Close必须join全部实际调用后才释放facts，超时保持未完成，且不额外序列化模型与Tool。显式Close永久封闭，普通run end仍可复用无Processor配方；配置Processor保持原attempt-owned terminal策略。参数和耐久证据拒绝重复键/大小写别名/非法Unicode。
不接受主机路径/URL，不改原件、不自动处理、不自动投递、不跨native/child借日志。
每原件最多64个manifest，每native最多128个selection，固定高水位小页，完整字节hash检查。
receipt prepared不代表发送；model_chain_invocation_intent仅表示调用前的耐久意图，不证明调用/网络接受或视觉理解；
错误/断流保留acceptance unknown。结束/重启不重放selection；取消只允许未提交选择。
图片在普通引用/压缩之后注入，512KiB媒体/16KiB provenance不等于精确multimodal token预算。
Plugin在注册处显式指定最大order，Finalize核对自己是最后middleware，否则拒绝装配；实现无Priority方法。外层retry无法得到注入后的request，
再次进入本层只拿无图请求。实际model/transport自己的执行实现仍是可信宿主代码，不是其重试沙箱。

存储只消费本包按需组合的共享storage能力插口；会话/引用/DTO/错误从同一契约返回，不导入具体session.Repository或SQL实现。后端由上层程序/插件装配选择，组件不回落默认库、不downcast实现，不持有存储Close。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| lifetime.go / lifetime_test.go | Tool/model/receipt取消与active-call join、超时保留、真实选图模型调用结束后才完成清理 |
| library.go | 同native权威的真实manifest/derivative与selection/receipt不可变事件 |
| plugin.go | Guard Tool/实际run绑定、显式audio/video派生与单请求副本JPEG注入，不把原件送模型 |
| audio_test.go | 真实ffmpeg Tool/Guard→WAV→同native manifest闭环，音频拒绝选图、伪造时长拒绝preview |
| plugin_test.go | 真SDK处理/选图/下一请求零重放、审批/foreign/同ID伪造拒绝、真实错误与取消回执 |
