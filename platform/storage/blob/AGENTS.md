# Immutable object storage
> 可插拔的不透明字节存储端口，不合并日志事务、文件系统或执行授权。

## 地位
纯标准库根契约；native session 是消费者，directory/memory 是适配器。SDK 不反向依赖本目录。
## 逻辑
Store 只有不可变 Put 和有界 Read。64位十六进制随机 key、32MiB 对象、64KiB 单次读取上限；
同 key 同内容幂等，不同内容冲突；成功写入要求随后读取可见。返回自有 buffer，不保留输入 buffer。
## 约束
调用方持有已授权 namespace；key 不等于模型访问权。错误/取消可能发生于发布之后，不证明回滚。
不提供分布式事务、CAS、扫描/删除、自动重试、网络凭据发现或任何执行能力。实际持久性由适配器及宿主明确声明。
云盘 API 可以实现本端口；仅具备最终一致性或非原子覆盖的实现必须补齐发布保障，否则不能宣称符合契约。
## 业务域清单
| 文件/目录 | 职责 |
|---|---|
| store.go | 不可变对象端口、边界验证、opaque key 与显式 retention 值 |
| conformance_test.go | 同一验收覆盖内存/真实目录、并发发布、取消、所有权和边界 |
| memory/ | 有对象数/字节上限的纯内存临时存储 |
| directory/ | pinned directory 的原子发布、本地或合规挂载目录适配 |
