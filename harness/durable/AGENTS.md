# L5 · Optional durable waits
> 可插拔存储的挂起、外部结果交付与明确继续，不给内核添加业务状态机。

## 地位
可选 harness Controller/Plugin；复用普通 Tool、typed middleware 与 SDK Agent。direct CLI 可由 `--durable` 显式接入，默认仍不启用；
Hub 等待 UI/HTTP 尚未接入本组件。只 `MemoryBackend` 的运行不具备重启耐久性。

## 逻辑
Backend 四个端口：实际 Log、独占 Claim、Load、原子 CAS Commit（状态与结果消息同事务）。
`Start` 从 idle 开始；提交外部请求或遇到耐久审批/问题后，等实际 Agent hooks/Flush 退出才提交 waiting，
返回 `Outcome.Suspended`，不发成功/失败终态。`Resolve` 核对当前会话请求 ID、完整 payload binding、期限和类型。
`Resume` 原子记录一次结果投递及 running，再以真实 SDK 继续；提交后崩溃保持 running/interrupted，拒绝自动重放。
`Abandon` 是检查事实后的明确宿主操作，不证明外部工作已取消或副作用已回滚。

审批由正常 `security.ApprovalHandler` 消费，同会话/工具/原参数 SHA256/效果严格匹配，在实际执行前持久消费一次。
恢复不直接重跑旧工具，模型可以明确发起新的调用；新参数必须重新审批。问题实现普通 `interaction.Handler`，
选项/自由文本在 Resolve 时验证，和权限审批完全分开。`await_external` 只提交请求数据，不启动外部任务或授予能力。
已知依赖构造注入；Bus 的 `*Controller` 只证明 Agent 实际注册的是同一个 Controller，缺失时在 IO 前拒绝。

## 约束
快照 schema/version 1；最多16个待办和16个一次性决定、256KiB状态、32KiB请求、16KiB结果，期限最多24小时。
未知版本、绑定损坏、过期、陈旧 revision、不同答案或不完整收尾显式失败。存储错误不自动重试。
等待可正常退出进程，不持有 goroutine；外部任务的启动、幂等和物理取消由显式宿主 owner 负责。
有状态字段/结果采用自有 namespace 的 extension facts，不改变 L1 的 outcome enum 或默认循环。
传入的 Agent 必须使用 Backend 的实际 log 和本 Plugin；产品入口必须消费 `Outcome` 才能展示挂起。

## 业务域清单
| 文件/子目录 | 职责 |
|---|---|
| state.go | 窄 Backend、版本化状态/请求/结果、规范化绑定与暂停信号分类 |
| controller.go | 独占 Start/Resume、实际运行绑定、结果交付、Resolve/Abandon |
| adapters.go | 有效期/选项校验、一次性精确审批与问题恢复 |
| tool.go | 无外部副作用的 await_external 数据提交工具 |
| memory.go | 明确 volatile 的同日志原子内存 reference backend |
| controller_test.go | 真 SDK 等待/继续、审批边界、问题、过期与未知版本验收 |
| journal/ | 共享事实插口之上的耐久适配与真实多进程崩溃验收；不选择数据库 |
