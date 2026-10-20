# L5 · Durable journal adapter
> 共享事实插口之上的原子检查点和结果消息，不绑定具体数据库。
## 地位
可选harness适配器，只消费storage共享能力子集与SDK log/reference契约。
上层选择事实组件；本包不打开或关闭存储、不导入具体Repository。
## 逻辑
Open验证实际owner/session和事实/字节两项persistent retention；Claim取得同一执行权威的独占claim。
Load读取确切typed stream的最新记录并验证hash/schema/metadata；Commit原子CAS状态与结果消息。
与harness合用时装配SessionViews；Controller已持有claim，不重复安装SessionPlugins的claim。
## 约束
超过4KiB的文本先保留同会话引用；由SessionViews按需读取。脱敏改变request binding时在CAS前拒绝。
ID不授予读权，存储错误不自动重试/切换后端。waiting可重开；running/interrupted需检查后显式Abandon。
测试由程序选择原生provider，运行真实退出/重开，不使用网络或账号。
## 业务域清单
| 文件 | 职责 |
|---|---|
| backend.go | 窄Storage契约、实际log/claim、快照恢复与原子状态+结果提交 |
| backend_test.go | 真实多进程external/approval恢复与写后exit42不重放 |
