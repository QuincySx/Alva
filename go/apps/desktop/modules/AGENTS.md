# L6 · Native message modules
> 版本化纯数据消息模块与可信Go renderer注册，不是Agent Plugin或任意脚本入口。
## 地位
独立于Hub/native execution事实；基础NativeView可按需使用，直接harness不依赖Hub。
## 逻辑
Markdown的alva-module代码块携带kind/version/data。内建table与bar-chart真实自绘；未知版本/类型/坏数据/超预算保留原始代码块fallback。未来受信任宿主Go代码可注册renderer。
## 约束
16KiB模块，table最多12列/200行，chart最多64项。消息无法下载/注册/执行代码，不自动读取原始媒体。未闭合流式代码块只显示源文。
## 业务域清单
| 文件 | 职责 |
|---|---|
| module.go | 纯数据封装与限制 |
| renderer_desktop.go | 原生renderer registry、table/chart与原文fallback |
| module_test.go | 数据/版本/预算与不可执行配置回归 |
| renderer_desktop_test.go | MyGo无窗口table/chart与未知模块原文fallback回归 |
