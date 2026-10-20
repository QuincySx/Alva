# L5 · Native durable wait backend
> 同一 native facts authority 的原子检查点与结果消息。

## 地位
可选 harness 适配器，消费现有 session.Repository，不新建 transcript/数据库或修改 SDK 存储契约。

## 逻辑
Open 校验真实 owner/session；Claim 使用原生 BeginRun 完成未知执行结算并取得独占权。
要求artifact retention为persistent，不能把volatile大结果引用冒充重启耐久等待；临时执行使用durable.MemoryBackend。
Load 只读确切 typed stream 的最新 record 并校验哈希、版本及元数据；Commit 同事务 CAS 状态与 conversation delivery。
与 harness 合用时显式装配 NativeSessionViews；Controller 已持有 claim，不重复安装 NativeSessionPlugins 的执行 claim。

## 约束
超过4KiB的投递正文留同一会话引用，其他content blocks保持；需要NativeSessionViews提供按需读取。脱敏若改变request binding，在CAS前拒绝，避免保存无法恢复的checkpoint。
字符串 ID 不授予读取权。原生脱敏/容量/哈希政策保持；规范化键顺序不改变绑定，正文更改会拒绝恢复。
重启没有自动调模型、批准或重放工具；running/interrupted 要检查事实后明确 Abandon。
测试子进程只运行当前测试二进制、临时私有数据和确定性模型，不使用网络/账号。

## 业务域清单
| 文件 | 职责 |
|---|---|
| backend.go | 实际 log/claim、精确快照恢复和原子状态+结果提交 |
| backend_test.go | 真实退出/重开 external/approval 恢复，以及写入后 exit42 的 unknown/no replay |
