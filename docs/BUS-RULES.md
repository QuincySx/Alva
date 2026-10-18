# Bus 使用边界

当前实现为 [sdk/kernel/bus](../sdk/kernel/bus/AGENTS.md)，只依赖标准库。
公开面以 `tests/architecture/testdata/public-surface/kernel-bus.txt` 为准。

## 类型化能力

New 创建 Writer/Handle；Provide 按显式类型注册，Resolve 按同一类型发现。
拒绝 nil、typed-nil、重复类型与 Seal 后写入；运行期不动态注册或隐式替换。
每次 Agent Build 拥有独立注册集，多轮 Run 复用；服务对象仅按宿主显式选择共享。
Handle 只限制注册集写入，不授予执行权限，也不使返回的服务只读。

组件内已知依赖优先构造传参。仅可选跨插件发现走 Bus，定义处说明提供者、实际消费者、
为何不用构造传参以及缺失行为。服务启动、关闭与取消/join 由其实际 owner 管理。
替换 Session 不自动重绑服务，child 也不自动获得父能力。

## 独立 Topic

Topic 显式创建、持有或注入，不通过全局/按类型的隐式发现创建。
有界广播可报告 LaggedError，不是 lossless 命令通道；不承载 RPC、模型 token、
工具执行授权、耐久事实或必须有序的控制流。clone 纯且拥有引用字段，在状态锁外执行。

Blackboard 具有 Scope、身份、历史与授权语义；不与 Bus 合并。
当前 Blackboard 暴露订阅接口，但仓内协作走历史/context hooks，没有实际生产订阅者。
不能把封装存在或测试消费者当作生产通知闭环。

## 禁止扩张

Bus 不存产品业务，不模拟请求/响应，不替代 Tool/Middleware/执行契约，不成为万能上下文。
已批准删除 StateCell、BusPlugin 启动器与 TopicFor，不保留同名兼容入口。
未来需要新的状态或生命周期能力时先落实外层 owner/实际消费者，再评估公开契约。
实际能力 key 与消费者清单见本包 AGENTS.md；公开变更须审阅快照并说明源码兼容影响。
