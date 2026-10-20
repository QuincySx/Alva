# L0 · sdk/kernel/bus
> typed能力注册与独立有界广播原语。

## 地位
L0；仅标准库，不认识Agent或产品服务。

## 逻辑
New创建Writer/Handle，Provide[T]按显式类型注册，Resolve[T]只读发现；Seal后注册集固定。
Topic由能力实现显式创建/持有，提供有界通知；不挂在registry中，不引入另一套Plugin生命周期。

## 约束
拒绝nil、重复类型和seal后写入。每次Agent Build拥有独立registry，多轮Run复用；服务对象仅按宿主显式选择共享，Handle不授予执行权限或自动重绑session。
组件内已知依赖优先构造传参；可选跨插件能力才通过typed发现，缺失行为由消费者明确决定。
Topic通知允许lag并明确报错，不承载工具命令、RPC、模型token或耐久事实。clone纯且独立串行，在状态锁外运行。
服务启动与关闭由外层owner负责；Agent装配由agent/core Plugin/Register/Finalize负责。

### 当前装配与采用范围（2026-10-18）

Agent每次Build创建独立registry，同一Agent的多轮Run复用；不是进程全局单例。切换State.Session不会自动重绑已注册服务；child只获得外层显式注入的对象。不同registry可被宿主显式注入同一个锁等共享服务，registry隔离不等于服务对象隔离。
Handle只限制注册集写入，不使取出的服务只读，也不授予工具执行权限。组件内已知依赖优先构造传参；可选跨插件发现才使用Resolve，所需capability、类型key和缺失行为应由消费者明确。

| 能力key | 提供者 | 仓内生产消费者 |
|---|---|---|
| `*core.ToolLockRegistry` | Hub SharedLocksPlugin | Shell/Notebook核对实际共享实例与执行lease |
| `*harness.PendingService` | PendingPlugin | host.Controller的显式Steer/PendingMessages |
| `kernel/core.DiagnosticObserver` | 可选telemetry Plugin（harness默认按名可替换） | DefaultAgentLoop；缺失不写诊断，失败显式报告、执行事实不经此口 |
| `*durable.Controller` | 可选durable Plugin | Controller.Start/Resume验证Agent实际装配同一owner，缺失在IO前拒绝 |
| `agentabi.AnalyticsSink` | Analytics Plugin | 独立Middleware观察实际生命周期 |
| `*memory.Service` | harness默认/可替换Memory Plugin | 暂无生产Resolve；工具直接持有已配置服务，测试验证对外发现/替换 |
| `planning.Service` | Planning Plugin | 暂无生产Resolve；实际工具持有同一服务 |
| `tooling.Manager` | diagnostics Plugin | 暂无生产Resolve；实际工具直接持有manager |

实际调用点以源码/静态import闸为准，不把词法调用点数当运行次数。Topic由Blackboard封装发布/订阅接口，当前协作走历史读取和context hooks，仓内暂无实际生产订阅者；不能把封装存在当成通知驱动闭环。
StateCell、BusPlugin/StartPlugins和TopicFor的裁剪决策与对外源码兼容损失见根MIGRATION；本包不另留同名兼容入口。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Capabilities | `bus.go` | Writer/Handle、typed registry与封闭注册 |
| Regression | `bus_test.go` | 类型身份、重复/封闭注册和nil拒绝 |
| Broadcast | `events.go` | 显式独立Topic、bounded ring、lag/cancel/close |
| Broadcast tests | `events_test.go` | ring/lag、重订阅、复制和并发close |
| Callback tests | `boundary_test.go` | clone可观察topic metadata而不持状态锁 |
