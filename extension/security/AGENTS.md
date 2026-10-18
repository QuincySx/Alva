# L4 · Security plugin
> SDK Guard 的可选 Plugin 与 actual Tool 身份绑定。

## 地位
仅装配安全中间件，不实现文件系统或默认 harness 配方；同名 security Plugin 替换默认项。
## 逻辑
只有 actual Tool 与显式可信实例相同才采用其 effect；同名替换、无法证明身份或动态不可比较值均按 Unknown 审批。
## 约束
审批失败中止调用；不降为可恢复结果。比较两个实际 reflect.Value 的 Comparable，不能因 interface 字段中的 map/slice 而 panic。不借工具名扩权，不自动信任新注册值。
## 业务域清单
| 文件 | 职责 |
|---|---|
| security.go | NewSecurityPlugin、注册与实际调用身份/effect 闸 |
| identity_test.go | 动态不可比较身份与真实 Agent 审批回归 |
| security_test.go | 实际文件工具的未知审批与同名替换信任拒绝 |
