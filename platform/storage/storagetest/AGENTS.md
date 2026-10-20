# Shared storage conformance
> 对外提供统一插口的测试辅助，不增加运行时层。
## 地位
测试代码显式导入；生产消费者不得依赖。只使用共享契约、公开SDK和标准库。
## 逻辑
RunRepository通过调用方fresh factory验收完整应用profile：复用SDK sessiontest，增加authority、竞争CAS/原子投递、高水位/有界历史、引用归属、执行claim和实际父子因果。RunBindings独立验收create-or-verify与竞争owner。
## 约束
Factory拥有Close，测试等待所有并发调用完成。无具体provider、SQL、文件或网络依赖。SDK-only会话使用sessiontest，字节provider独立使用blobtest。持久化、声明的retention、损坏/中途故障、容量、跨进程锁与Close重试须由后端另行验收；不可将内存测试当作云盘事务证明。
## 业务域清单
| 文件 | 职责 |
|---|---|
| contract.go | RunRepository/RunBindings与小规模非凭据fixtures |
