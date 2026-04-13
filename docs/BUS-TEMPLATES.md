# Bus 扩展模板

> 三个模板覆盖所有场景。选一个，复制，填空。

---

## 模板 A：跨层服务

场景：一个 crate 提供服务，其他 crate 消费。

### 第一步：定义 trait

位置：`crates/alva-kernel-abi/src/` 对应模块

```rust
/// [一句话说明这个服务做什么]
///
/// 注册方: [谁注册]
/// 消费方: [谁使用]
pub trait MyService: Send + Sync {
    fn do_something(&self, input: &str) -> Result<Output, Error>;
}
```

在 `crates/alva-kernel-abi/src/lib.rs` 添加 re-export：

```rust
pub use 模块路径::MyService;
```

### 第二步：实现

位置：实现所在的 crate

```rust
pub struct MyServiceImpl { /* 字段 */ }

impl MyService for MyServiceImpl {
    fn do_something(&self, input: &str) -> Result<Output, Error> {
        // 实现
    }
}
```

### 第三步：注册

位置：`crates/alva-app-core/src/base_agent.rs` 的 `build()` 方法

```rust
bus_handle.provide::<dyn MyService>(Arc::new(MyServiceImpl::new()));
```

### 第四步：消费

Tool 里：

```rust
async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
    let svc = ctx.bus()
        .and_then(|b| b.get::<dyn MyService>())
        .ok_or_else(|| AgentError::Other("MyService not available".into()))?;
    let result = svc.do_something("input")?;
    Ok(ToolOutput::text(result))
}
```

Middleware 里：

```rust
async fn before_llm_call(&self, state: &mut AgentState, messages: &mut Vec<Message>) -> Result<(), MiddlewareError> {
    if let Some(ref bus) = self.bus {
        if let Some(svc) = bus.get::<dyn MyService>() {
            // 使用服务
        }
    }
    Ok(())
}
```

优雅降级（服务可选时）：

```rust
// get() 返回 None → 跳过，不报错
if let Some(svc) = ctx.bus().and_then(|b| b.get::<dyn MyService>()) {
    // 有服务就用
}
// 没有就继续走，功能降级
```

### 检查清单

- [ ] trait 定义在 alva-kernel-abi（不在实现 crate）
- [ ] re-export 到 alva-kernel-abi/src/lib.rs
- [ ] provide() 在 BaseAgent::build() 里
- [ ] 消费方用 get() 不用 require()（除非服务必须存在）
- [ ] 没有改任何函数签名

---

## 模板 B：跨层事件

场景：某处发生了事情，其他地方需要知道但不需要返回值。

### 第一步：定义事件

位置：`crates/alva-kernel-abi/src/` 对应模块

```rust
/// [一句话说明这个事件代表什么]
///
/// 发送方: [哪个 crate / 哪个函数]
/// 接收方: [预期哪些 crate 订阅]
/// 语义: [接收方应该做什么]
#[derive(Clone, Debug)]
pub struct SomethingHappened {
    pub session_id: String,
    pub detail: String,
}
impl alva_kernel_bus::BusEvent for SomethingHappened {}
```

在 `crates/alva-kernel-abi/src/lib.rs` 添加 re-export：

```rust
pub use 模块路径::SomethingHappened;
```

### 第二步：发送

```rust
if let Some(ref bus) = self.bus {
    bus.emit(SomethingHappened {
        session_id: session_id.to_string(),
        detail: "what happened".into(),
    });
}
```

### 第三步：接收

```rust
let mut rx = bus.subscribe::<SomethingHappened>();
tokio::spawn(async move {
    while let Ok(evt) = rx.recv().await {
        tracing::info!(detail = %evt.detail, "something happened");
        // 处理事件
    }
});
```

### 检查清单

- [ ] 事件 struct 实现 `Clone + Debug`
- [ ] 实现 `BusEvent` trait
- [ ] 文档注释写了 发送方 / 接收方 / 语义
- [ ] emit() 是非阻塞的（不等接收方处理完）
- [ ] 没有用事件模拟请求-响应（需要返回值用模板 A）
- [ ] 没有在 event handler 里 emit 同类事件

---

## 模板 C：替换实现

场景：把某个能力换成新实现（测试替身、升级方案、A/B 切换）。

### 步骤

```rust
// 新实现
struct BetterTokenCounter { /* ... */ }
impl TokenCounter for BetterTokenCounter { /* ... */ }

// 替换：一行
bus_handle.provide::<dyn TokenCounter>(Arc::new(BetterTokenCounter::new()));
```

所有 `bus.get::<dyn TokenCounter>()` 的消费方自动使用新实现。零改动。

### 测试中替换

```rust
#[tokio::test]
async fn test_with_mock_service() {
    let bus = Bus::new();
    let handle = bus.handle();

    // 注册 mock
    handle.provide::<dyn MyService>(Arc::new(MockMyService::new()));

    // 被测代码使用 bus，自动拿到 mock
    let result = do_something_with_bus(&handle);
    assert_eq!(result, expected);
}
```

### 检查清单

- [ ] 新实现和旧实现满足相同的 trait
- [ ] provide() 会覆盖旧注册（同一类型只有一个）
- [ ] 不需要通知消费方（它们下次 get() 自动拿到新的）

---

## 反模式（不要这么做）

### 不要用事件模拟 RPC

```rust
// ❌
bus.emit(CompressRequest { ... });
let result = bus.subscribe::<CompressResponse>().recv().await; // 等回复

// ✅ 用模板 A
let compressor = bus.require::<dyn Compressor>();
let result = compressor.compress(data).await;
```

### 不要在同一 crate 内用 bus

```rust
// ❌ 同一 crate 的两个模块
// module_a.rs
bus.emit(InternalEvent { ... });
// module_b.rs
bus.subscribe::<InternalEvent>();

// ✅ 直接调函数
module_a::do_thing();
```

### 不要运行时动态注册能力

```rust
// ❌ 在 tool 执行过程中
async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) {
    ctx.bus().unwrap().provide(Arc::new(SomeNewService)); // 运行时注册
}

// ✅ 只在初始化阶段注册（BaseAgent::build）
```

### 不要持锁 emit

```rust
// ❌ 可能死锁
let guard = mutex.lock().await;
bus.emit(SomeEvent { data: guard.clone() });

// ✅ 先释放锁
let data = { mutex.lock().await.clone() };
bus.emit(SomeEvent { data });
```

---

## 快速对照表

| 我要做什么 | 用哪个模板 | 关键 API |
|-----------|-----------|---------|
| 提供一个服务让别人用 | A | `provide()` + `get()` |
| 通知别人某事发生了 | B | `emit()` + `subscribe()` |
| 换掉某个服务的实现 | C | `provide()`（覆盖） |
| 监听某个值的变化 | StateCell | `set()` + `watch()` |
| 同一 crate 内调用 | 不用 bus | 直接函数调用 |
| agent 之间通信 | 不用 bus | Blackboard |
| middleware 内部状态 | 不用 bus | Extensions |
