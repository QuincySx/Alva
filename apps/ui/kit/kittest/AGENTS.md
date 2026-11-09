# Kit test helpers
> 像素读取、居中 harness、截图保存与名称审计，供 kit 与画廊测试共用。

## 地位
测试辅助包，允许依赖 `testing`（架构测试把它列为 test-support）；借 lobe-mygo testkit 的 API 形状，未复制代码。
## 逻辑
`PixelAt/Near/Within/Over/Ink` 读渲染结果；`Centered/Row` 套设计主题；`Names` 收集构造函数上报的名称；`Screenshot` 仅在设置 `ALVA_KIT_SHOTS` 时写 PNG。
## 约束
不被生产代码导入；不提交截图。
## 业务域清单
| 文件 | 职责 |
|---|---|
| kittest_desktop.go | 上述全部辅助 |
