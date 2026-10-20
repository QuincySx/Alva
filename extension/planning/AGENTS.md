# L4 · Portable Planning Tools
> 独立计划工具、TaskService 任务工具组与显式输出政策。

## 地位
通用可选能力；只依赖标准库、SDK 和纯 strictjson 校验，不依赖原生存储、Guard、工作目录或 Plugin 生命周期。

## 逻辑
NewPlanTool(write) 只安装 todo_write；写入函数可返回 nil 引用表示完整保存但只需小回执，不要求任务服务。
NewTaskTools(TaskToolsConfig) 返回六个任务工具，只消费四方法 TaskService。可直接注册到 SDK Builder.Tool；
默认返回完整有界详情，RetainDetail 显式选择引用优先。Service 是原生完整组合契约，不是普通任务工具的门槛。
Service 拥有业务校验、并发与持久化；工具保留严格参数、实际 Tool/call 绑定、分页及输出限额，不启动执行或改变权限。
通用任务列表不要求计划读取；native/ 完整组合包继续在输出适配中附带既有 plan_notes 引用。

## 约束
绑定由可信宿主提供，不接受模型输入 owner/session 存储授权；typed-nil Service 和 nil 写入函数拒绝。
JSON 回执最多16KiB，详情最多128KiB；无引用模式超界明确失败，不截断；显式引用模式失败不回退全文。
已提交 mutation 后的详情编码/保存/输出/取消失败返回 CommittedError（call/task ID、committed 与恢复提示，原因可 Unwrap），
通过errors.As读取Recovery()的typed DTO；Error文本不作为稳定JSON协议。可选RecoveryRecord提供schema1事实，默认harness RecoveryEvidence在timeout/Join外保存；写入失败仍为强错误，不自动重放。后端返回一般错误不承诺零副作用；已知提交后失败的后端应返回 CommittedError。
普通工具不替代宿主授权。native/ 显式组合六任务与一个计划工具，保持实际原生会话、Guard、CAS、脱敏与引用优先。

## 业务域清单
| 文件/子目录 | 职责 |
|---|---|
| service.go | TaskRecord/TaskPage、请求 DTO、窄 TaskService 与原生完整 Service 契约 |
| tools.go | 六任务工具、严格解析与可选详情引用政策 |
| plan.go | 独立 todo_write，只消费写入回调，无任务 CRUD 或必需引用后端 |
| commit.go | 已知提交后回执失败的 typed error、Recovery DTO/Record、关联标识与有界恢复提示 |
| tools_test.go | 无原生组件的真 SDK create/get、引用、严格输入与保存失败 |
| recovery_test.go | 独立计划、默认循环已提交错误结算、无存储读取恢复和超界不重放 |
| native/ | 原生 Plugin、默认typed-state政策、共享会话插口、授权与详情证据适配；复用父包工具 |
