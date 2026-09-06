# 01 Packaged Acceptance Scenario Runner

## 目标

让真实 Tauri command、SQLite store、timeline projection 与 WebView 可以在 packaged app 中重复执行故障和恢复场景，同时保证入口不会进入正式发布包。

## 工作

- 定义场景输入：Engine 结果、stream event、terminal reason、延迟点和 crash point；
- 复用 `EngineRuntime` seam，不在 kernel / `agent-core` 增加测试开关；
- 用独立 compile feature、test binary 或 acceptance-only sidecar 隔离 fixture；
- 每次运行输出版本化 evidence envelope：scenario、输入、关键 IDs、timeline、DB 断言和 artifact identity；
- 提供一键清理的 owner-only 临时数据目录，不接触用户真实 Hub DB；
- 构建检查正式 `.app` 不含 fixture symbol、scenario data 或测试凭证。

## 验收

- 同一场景连续运行结果一致，不依赖网络和真实账号；
- 从 WebView 发起并穿过与生产相同的 Tauri/store/projector 路径；
- 生产 bundle 内容检查证明 acceptance runner 未被打包；
- 场景证据明确标为 P，不冒充 L。

## 依赖

Ticket 00。

