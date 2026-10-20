# WASIp1 AEP author example
> 独立Go module生成可单独加载的stdio插件。
## 地位
外部作者的纯标准库协议示例，不是SDK guest、宿主或第二个Agent实现。
## 逻辑
实现AEP initialize/事件/tool/shutdown与同步reverse callback，小队列保存竞态通知。真实loader/Hub测试单独编译和执行。
## 约束
无root module replace、平台依赖、宿主目录/环境/网络grant。内存/IO/期限由host执行peer决定；stdout仅协议。hang/fail/probe明确是验收参数，不隐式运行。
## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | standalone command模块、echo/hook/callback及故障验收分支 |
| go.mod | 独立module与Go版本，无第三方依赖 |
| alva.toml | runtime=wasm与明确模块/callback请求 |
| README.md | 编译、检查批准、加载限制和实际验收入口 |
