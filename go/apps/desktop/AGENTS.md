# L6 · Go Native Desktop
> MyGo 自绘桌面入口的临时呈现状态与视图，不拥有 Agent 执行或耐久历史。

## 地位

产品入口消费 `host.Controller`，显式data-dir模式另消费独立apps/direct native应用owner；不向kernel/agent SDK注入桌面框架依赖。MyGo相关文件受desktop build tag保护，普通SDK/headless测试不链接桌面库。

## 逻辑

显式 Send 后 runtime 接受请求，UI 才清空草稿并增加稳定 ID 消息行。后台订阅只通过 `Window.Update` 更新 UI，每个 observer 至多一个待执行 UI closure，慢订阅通过快照恢复而不重放执行；订阅容量不足时报告错误并退到 250ms、可取消的快照轮询，仍以 runtime 真正终态解除活跃状态。Stop 是显式 runtime 操作，关闭订阅不取消执行。列表虚拟化并保留用户阅读位置；Markdown 直接自绘，不创建 WebView 或 npm 前端。

## 约束

- 此入口标明 Go native foundation 与 demo/live 路由，不冒充已完成 Hub 迁移。
- 当前 SDK session 保留进程内多轮上下文，展示内存临时保留 64 行；只传新输入，不重复发送展示历史，不自动恢复/读取旧 Rust 存储。
- 不自动发送、不自动调用付费模型/MCP，不下载图片或执行 Markdown 中的内容。
- 无data-dir仍为独立易失foundation；显式data-dir的PersistentPage拥有native会话列表/原workspace绑定、引用上传、逐调用审批和冷typed Inspector。分支/团队属于独立Hub，媒体派生/MCP等可选direct配置仍另验。Hub可以消费本包控件，本包不得反向import Hub/engine。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Presentation | `model.go` | 草稿、稳定消息行、有限展示、运行状态与迟到结果防护 |
| State tests | `model_test.go` | 无窗口的输入、完成、停止、错误与边界回归 |
| Native view | `view_desktop.go` | MyGo 原生虚拟消息列表、Markdown、输入框与 Send/Stop |
| Native view tests | `view_desktop_test.go` | MyGo Tester 无窗口输入/中文 IME、显式发送/停止、一万行虚拟化与阅读锚点 |
| Native app | `app_desktop.go` | Window 生命周期、UI-thread 更新与可刷新但不重放执行的订阅 |
| UI diagnostics | `debug_desktop.go` / `debug_desktop_test.go` | 注册实际model/composer/Send/Stop与own-window PNG，控件禁用状态一致；真实HTTP+MyGo Tester验证中文草稿/不重复执行/Stop等待，不冒充OS窗口验收 |
| Persistent direct owner view | `persistent_desktop.go` | apps/direct实际会话/历史/附件/Controller订阅与独立权限观察，不用绘制当执行时钟 |
| Persistent controls | `persistent_view_desktop.go` | MyGo会话列表/分页composer、跨pane审批、固定高水位metadata/显式2KiB原文Inspector |
| Persistent diagnostics | `persistent_debug_desktop.go` | 实际native语义树与正常创建/选择/草稿/Send/Stop，current Build前拒绝旧scene |
| Inspector categories | `inspection.go` | 当前metadata页的LLM/Tools/Middleware/Other分类，不改query cursor或模型上下文 |
| Persistent export | `persistent_export_desktop.go` | 实际snapshot的私有JSONL保存，Close等待实际写入owner；不是按UI过滤器删改原事实 |
| Persistent regressions | `persistent_desktop_test.go` | 实际控件创建/SDK demo/冷Inspector/附件移除、跨Inspector可见actual input和真实once批准写入 |
| Observer | `observer.go` | 无框架依赖的有限订阅与快照恢复，不拥有执行生命周期 |
| Observer tests | `observer_test.go` | 订阅释放不取消、过期窗口/容量不足后的真实终态与 Stop 恢复 |
| Shared modules | `modules/` | 版本化纯数据 native table/chart renderer 与原文 fallback |
