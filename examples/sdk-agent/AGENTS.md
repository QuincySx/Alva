# External SDK agent
> 独立Go module，用现有公开SDK完成最小扩展闭环。
## 地位
应用消费者，不是SDK、默认harness或Alva CLI。
## 逻辑
程序注入Greeter、LanguageModel与MemorySession；Plugin贡献Tool和单函数BeforeModel。run模式执行真实model→tool→model，cancel模式在实际工具进入后取消并检查结算事实。
## 约束
同一源码在native、JS/WASM与WASIp1运行。无native import、网络、凭据、后台goroutine或新增SDK接口。go.mod相对replace仅绑定当前checkout，v0.0.0不是已发布版本；对外应用需明确pin版本。
## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | 可注入服务、普通Plugin/Tool/Middleware与两种实际执行回执 |
| go.mod | 独立consumer身份与显式checkout绑定 |
| README.md | 本地、两类WASM运行与外部应用接入方法 |
