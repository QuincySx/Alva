# L4 · Builtin Extensions
> 内置能力的 Plugin wrapper 与宿主 Tool 实现。

## 地位

不属于稳定 SDK，app 只装配，不复制本目录实现。

## 逻辑

SecurityPlugin 仅在实际 ToolExecutionContext.ActualTool 与显式 trusted 实例身份相同时信 effect；运行中同名替换或不能证明身份的值按 Unknown 审批；
路径由 Guard 和真实 pinned filesystem 双重约束，而不是词法规则冒充沙箱。
身份比较检查两个动态值的可比较性，interface字段中的map/slice不因外层struct类型可比较而执行会panic的相等比较。

## 约束

文件Tool自行拒绝null、重复键、大小写别名及非法Unicode，未装schema Plugin也不能把null content变为空覆盖。
同名 security Plugin 可替换默认；审批错误必须终止调用，不能改成可恢复业务结果。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Security wrapper | `security.go` | Plugin 装配、调用绑定与 HITL effect 闸门 |
| Native files | `files.go` | pinned roots、引用先行读取、备份后原子写/编辑、有界字面搜索与受管单层目录宿主能力 |
| Write boundaries | `boundary_test.go` | 实际已有文件的null/重复键/Unicode拒绝与原字节不变 |
| Regression | `files_test.go` | 实际文件/引用/备份、敏感路径、symlink、跨会话与默认授权回归 |
| Identity regressions | `identity_test.go` | 动态不可比较值降为Unknown，真实Agent审批后执行或明确拒绝，不panic |
| Checked file snapshots | `snapshot.go` / `snapshot_test.go` | Existing os.Root逐级pin父目录、完整hash与mode检查、保留mode原子替换；不是不合作OS写者的hash-CAS |
