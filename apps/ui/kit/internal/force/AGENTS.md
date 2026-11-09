# Forced interaction state
> 让画廊与测试把控件渲染成 Hover、Pressed 或 Focus 的作用域。

## 地位
K1 技术 spike 的结论：运行时状态无法在变体矩阵里靠真实指针摆拍，所以构建期读一个作用域值；它不进任何 Props，公开 API 不被污染。只有 `apps/ui/kit/**` 能导入。
## 逻辑
`With(state, build)` 在构建期间强制状态并还原；组件用 `IsHovered/IsPressed/IsFocused(real)` 把真实状态与强制状态取并集。构建在 UI 线程同步完成，作用域不会漏到后续帧。
## 约束
不导入 MyGo；使用它的测试不得并行。
## 业务域清单
| 文件 | 职责 |
|---|---|
| force.go | State、Current、With 与三个并集判断 |
