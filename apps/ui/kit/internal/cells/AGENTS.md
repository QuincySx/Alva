# Gallery cell state
> 画廊页与其测试的跨帧状态：键入的文字、打开的菜单、切换。

## 地位
只服务示例页（kit、blocks、gallery）：全局 map 放在 `internal`，生产代码导入不了；`kit_test` 还限定只有示例页文件与测试能导入它。
## 逻辑
`Slot[T](page, sel, slot, init)` 按页面、轴值与槽名返回持久指针；`Text` 读字符串槽供测试断言；`Reset` 清空。
## 约束
不导入 MyGo；不用于任何产品或组件的真实状态。
## 业务域清单
| 文件 | 职责 |
|---|---|
| cells.go | Slot、Text、Reset |
