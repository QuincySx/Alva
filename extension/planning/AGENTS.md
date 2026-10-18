# L4 · Portable Planning Tools
> 绑定业务 Service 与详情保存函数的普通任务/计划工具。

## 地位
通用可选能力；只依赖标准库、SDK 和纯 strictjson 校验，不依赖原生存储、Guard、工作目录或 Plugin 生命周期。

## 逻辑
NewTools(Service, retain) 返回七个普通 Tool；可直接注册到 SDK Builder.Tool。Service 拥有业务校验、并发与持久化，retain 保存完整 TaskRecord 并返回宿主可读取的引用。工具消费同一实例、保留严格参数/实际 Tool 与 call 绑定、分页及输出限额；不启动进程/Agent，也不改变权限模式。

## 约束
绑定由可信宿主提供，不接受模型输入 owner/session 存储授权。typed-nil Service/缺失 writer 拒绝；详情编码/保存失败不返回成功，也不重放已提交的业务操作。普通工具不替代宿主授权。原生产品导入 native/，保留原会话 authority、审批、CAS 快照、脱敏引用与 middleware。

## 业务域清单
| 文件/子目录 | 职责 |
|---|---|
| service.go | TaskRecord/TaskPage、请求 DTO 与可替换 Service 契约 |
| tools.go | NewTools、七工具的唯一 schema/解析/业务消费与有界引用回执 |
| tools_test.go | 无原生组件的真 SDK create/get、引用、严格输入与保存失败 |
| native/ | 原生 Plugin、默认持久后端、授权与详情证据适配；复用父包工具 |
