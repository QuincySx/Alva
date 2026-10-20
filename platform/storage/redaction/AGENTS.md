# 横向支撑 · Pure journal redaction
> 内存与持久journal共同使用的纯数据处理，不选择存储。
## 地位
只消费标准库与SDK record契约；不导入SQLite/session实现，不进行IO。
## 逻辑
按已知credential字段与显式immutable EnvironmentSecrets snapshot脱敏。
流式envelope仅处理明确声明的完整frame；过界、不完整配置秘密或编码错误fail-closed。
## 约束
不扫描ambient环境、不声称能发现任意prose秘密；用户上传原字节与日志脱敏分开。
已有safe JSON字段、普通正文和幂等规则保留；不递归解码任意业务base64。
本次只迁移源码归属；原session/redaction_test完整保留为适配器验收。
## 业务域清单
| 文件 | 职责 |
|---|---|
| redaction.go | RedactRecord/JSON/StreamFrame和显式秘密快照 |
