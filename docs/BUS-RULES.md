# alva-agent-bus 防破坏规则

> Bus 是跨层协调总线，不是万能通道。本文档定义它的边界，防止退化为 God Object。

## 一、依赖防火墙

### 新增 Rule 0

```
Rule 0:  alva-agent-bus 零 workspace 依赖（与 alva-types 同级基础设施）
Rule 1:  alva-types → alva-agent-bus only（原 "零依赖" 升级为 "仅依赖 bus"）
```

其余 Rule 2-16 不变。bus 通过 alva-types 的传递依赖自动到达所有 crate，无需任何 crate 单独加 `alva-agent-bus` 依赖。

### CI 脚本变更

```bash
# Rule 0: alva-agent-bus has ZERO workspace deps
check_no_workspace_deps "alva-agent-bus"

# Rule 1: alva-types only depends on alva-agent-bus
check_no_workspace_deps "alva-types" "alva-agent-bus"
```

### 禁止反向依赖

```
alva-agent-bus 不得依赖任何 alva-* crate。
任何 crate 不得绕过 alva-types 直接依赖 alva-agent-bus。
```

违反以上两条的 PR 必须被 CI 拦截。

---

## 二、Bus 三件套使用边界

Bus 提供三个机制，每个有明确的使用场景和禁区：

### Caps（能力注册/发现）

**可以用：**
- 跨 crate 能力发现 — tool 需要访问 memory 服务、engine 需要查询 token 统计
- 可选依赖解耦 — 某个能力不存在时优雅降级（`bus.get::<T>()` 返回 None）

**不可以用：**
- 同一 crate 内部模块之间的依赖注入 — 直接传参数或用构造函数
- 替代函数参数 — 如果调用方和实现方在同一层，用正常的 Rust 传参

**注册时机规则：**
- `provide()` 只允许在初始化阶段调用（各层的 `init(bus)` 函数内）
- 运行时（agent loop 执行过程中）禁止动态注册新能力
- 能力一旦注册，生命周期等同于 bus 本身

### Event（事件总线）

**可以用：**
- 跨 crate 通知 — 一个 crate 发生了事情，另一个 crate 需要响应
- 解耦发送方和接收方 — 发送方不需要知道谁在监听
- 广播场景 — 一个事件多个接收方（如 token 超限同时触发压缩和 UI 提示）

**不可以用：**
- 替代函数返回值 — 如果你需要调用后拿到结果，用 Caps 获取能力后直接调方法
- 同一 crate 内的模块通信 — 用 channel 或直接调用
- 高频数据流 — 每个 token 的 streaming delta 不走 bus，走现有的 event_tx channel
- 需要严格顺序保证的控制流 — bus event 是通知，不是命令

**事件定义规则：**
```rust
/// 每个 Event 必须有文档注释说明三件事：
/// - **发送方**: 哪个 crate / 哪个函数发出
/// - **接收方**: 预期哪些 crate 订阅
/// - **语义**: 这个事件意味着什么，接收方应该做什么
#[derive(Clone)]
pub struct TokenBudgetExceeded {
    pub ratio: f32,
    pub session_id: String,
}
impl BusEvent for TokenBudgetExceeded {}
```

### StateCell（共享状态格）

**可以用：**
- 跨层可观测状态 — 如当前 permission mode、当前 token 使用率
- 需要"读最新值"的场景 — UI 层读取 engine 层的状态

**不可以用：**
- 替代函数参数传递 — 不要用 StateCell 传递请求级别的数据
- 高频写入 — StateCell 每次写入会广播通知，不适合每秒几百次的更新
- 存储大对象 — StateCell 内部是 `Arc<RwLock<T>>`，Clone 时拷贝整个 T

---

## 三、绝对禁止

以下用法一旦出现，必须在 Code Review 中拒绝：

### 3.1 Bus 内不放逻辑

```rust
// ❌ 禁止：在 bus 内部实现业务逻辑
impl BusHandle {
    fn compress_context(&self) { /* ... */ }  // 这是业务逻辑，不属于 bus
}

// ✅ 正确：bus 只做分发，逻辑在各层实现
// alva-agent-context 里:
bus.subscribe::<TokenBudgetExceeded>().recv() → 调自己的压缩逻辑
```

### 3.2 不做请求-响应

```rust
// ❌ 禁止：用 event 模拟 RPC
bus.emit(CompressRequest { ... });
let result = bus.subscribe::<CompressResponse>().recv().await;  // 等待回复

// ✅ 正确：需要返回值时，用 Caps 获取能力直接调用
let compressor = bus.require::<dyn ContextCompressor>();
let result = compressor.compress(snapshot).await;
```

### 3.3 不替代现有 trait 接口

```rust
// ❌ 禁止：tool 执行通过 bus 间接调用
bus.emit(ExecuteTool { name: "bash", args: ... });

// ✅ 正确：tool 执行走现有的 Tool trait + ToolExecutionContext
tool.execute(input, ctx).await
```

### 3.4 不在 event handler 里 emit 同类事件

```rust
// ❌ 禁止：可能导致无限循环
bus.subscribe::<CompactCompleted>(|evt| {
    bus.emit(CompactCompleted { ... });  // 自己触发自己
});
```

### 3.5 不持锁发送

```rust
// ❌ 禁止：可能死锁
let guard = some_mutex.lock().await;
bus.emit(SomeEvent { ... });  // 如果 subscriber 也要拿这个锁 → 死锁

// ✅ 正确：先释放锁，再发送
let data = {
    let guard = some_mutex.lock().await;
    guard.clone()
};
bus.emit(SomeEvent { data });
```

---

## 四、与现有机制的关系

### Extensions — 逐步迁移，不急

| 情况 | 用哪个 |
|------|--------|
| 中间件之间传递本轮次数据 | 继续用 `Extensions`（请求级，生命周期短） |
| 跨 crate 的长生命周期能力 | 用 `bus.caps`（session 级，跨层可见） |

**迁移策略**：新写的跨层能力用 Caps，已有的 Extensions 用法不动。不做大规模重写。

### Blackboard — 保持独立

Blackboard 是 scope 内的多 agent 通信板，语义跟 bus 不同：

| | Bus | Blackboard |
|---|---|---|
| 范围 | 全局，整个进程 | 局部，某个 spawn scope 内 |
| 参与者 | crate（层与层） | agent（agent 与 agent） |
| 消息模型 | 类型化事件，无历史 | 带 ID 的消息，有历史记录 |

**不合并。** Blackboard 继续做 agent 间通信，bus 做 crate 间协调。

### AgentLoopHook — 可选替代

现有的 `AgentLoopHook`（从外部注入 steering/follow-up 消息）可以用 bus event 替代，但不是必须：

```rust
// 现有方式 — 继续工作
config.loop_hook = Some(Arc::new(PendingMessageQueue::new()));

// bus 方式 — 新功能可以选择用这个
bus.subscribe::<SteeringMessage>()
```

两种方式并存，逐步自然迁移。

---

## 五、alva-agent-bus 自身的约束

### 代码量上限

bus crate 的 src/ 目录总代码量**不超过 800 行**。如果超过，说明有逻辑泄漏进了 bus。

### 公开 API 上限

bus 对外只暴露以下类型，不多不少：

```rust
pub struct Bus;           // 创建和持有
pub struct BusHandle;     // Clone 后分发给各层
pub trait BusEvent;       // 事件标记 trait
pub struct StateCell<T>;  // 可观测状态格
```

### 不依赖 serde

bus 内部不做序列化。事件和能力都是内存中的 Rust 类型，不跨进程。如果未来需要跨进程（远程 sandbox），在 bus 外面加一个 adapter 层，不改 bus 本身。

### 不依赖 async-trait

bus 的公开 API 只用同步方法 + tokio::broadcast channel。不引入 `#[async_trait]`，保持接口简单。

```rust
impl BusHandle {
    pub fn provide<T>(&self, val: Arc<T>);       // 同步
    pub fn get<T>(&self) -> Option<Arc<T>>;       // 同步
    pub fn require<T>(&self) -> Arc<T>;           // 同步，panic if missing
    pub fn emit<E: BusEvent>(&self, event: E);    // 同步（非阻塞 send）
    pub fn subscribe<E: BusEvent>(&self) -> broadcast::Receiver<E>;  // 同步，返回 async receiver
}
```

### 测试要求

bus crate 自身必须包含以下测试：

1. `provide` + `get` 基本注册/发现
2. `require` 在未注册时 panic 并携带类型名
3. `emit` + `subscribe` 一对一
4. `emit` + 多个 `subscribe` 广播
5. subscriber drop 后不影响其他 subscriber
6. `StateCell` 读写 + watch 通知
7. 并发安全 — 多线程同时 provide/get/emit

---

## 六、Review 检查清单

每个涉及 bus 的 PR，reviewer 必须检查：

- [ ] 是否是跨 crate 通信？同一 crate 内不应使用 bus
- [ ] Event 是否有文档注释（发送方 / 接收方 / 语义）？
- [ ] 是否用 event 模拟了 RPC（发请求等回复）？如果是，改用 Caps
- [ ] 新增的 Caps trait 是否定义在 alva-types 中？不允许定义在 bus crate 里
- [ ] event handler 里是否有 emit 同类事件的风险？
- [ ] 是否持锁调用 emit？
- [ ] 是否在运行时动态 provide 能力？只允许初始化阶段
- [ ] bus crate 总代码量是否仍在 800 行以内？

---

## 七、演进路径

```
P0  创建 alva-agent-bus crate（~330行），更新 CI 脚本
    ↓
P1  alva-types 依赖 bus，ToolExecutionContext 加 bus() 方法
    AgentConfig 加 bus 字段，RuntimeExecutionContext 透传
    ↓
P2  用 bus 实现一个跨层功能验证（如 token 监控 + 压缩通知）
    ↓
P3  新功能优先用 bus 写，旧功能按需自然迁移
    ↓
    永远不做 "大规模迁移"，bus 和现有机制长期共存
```
