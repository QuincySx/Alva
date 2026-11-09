# Accessible-name audit
> 交互控件上报自己的可访问名称，测试据此断言没有无名控件。

## 地位
「图标按钮必须有名称」的运行期一半（编译期一半是 `IconButton` 的位置参数 label）。只有 `apps/ui/kit/**` 能导入。
## 逻辑
构造函数调用 `Named(kind, name)`；测试经 `Capture` 装 sink 后读取，没有 sink 时是空操作。
## 约束
不导入 MyGo；不记录用户内容，只记录组件种类与名称。
## 业务域清单
| 文件 | 职责 |
|---|---|
| audit.go | Entry、Named、Capture |
