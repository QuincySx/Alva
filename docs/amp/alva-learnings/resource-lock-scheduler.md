# Resource Lock Scheduler —— 工具并发调度器

> 让"并行默认"从 prompt 的建议升级为调度器的硬保证。

---

## 背景

Amp 的所有 executor prompt 都强调 "parallel by default"。这不是空口说白话 —— 每个 tool 都声明自己的**资源锁**，调度器用多读一写语义严格执行：

- Read / Grep / 其他只读工具 → 全并发
- 同文件 edit → 串行
- Bash → 全局独占

Alva 现在的 `Tool` trait 可能没有这层语义。模型看 prompt 写 "parallel"，但实际调用时如果调度器还是串行，模型会**学习到并发没意义**，下次就不并发了。

---

## 建议的 Rust 实现

### Step 1：扩展 `Tool` trait

```rust
// alva-kernel-abi/src/tool.rs

use std::collections::Vec;

pub trait Tool {
    // ... 已有字段
    
    /// 返回此次调用需要的资源锁。调度器按多读一写语义决定并发 / 串行。
    ///
    /// 默认返回空 vec（完全并发）。
    fn resource_keys(&self, args: &ToolInput) -> Vec<ResourceKey> {
        vec![]
    }
    
    /// 返回此工具的执行模式。默认 Parallel。
    /// 设 SerialGlobal 表示此工具独占（如 Bash），不管什么锁都排队。
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel
    }
    
    /// 可选：参数预处理钩子。在 schema 验证后、真正执行前调用。
    /// 用于"模型总是犯的错"的兜底修正。
    fn preprocess_args(&self, args: ToolInput, _ctx: &ToolContext) -> ToolInput {
        args
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceKey {
    pub key: String,      // 通常是文件的 absolute path 或其他资源 URI
    pub mode: LockMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LockMode {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExecutionMode {
    /// 默认。按 resource_keys 决定并发。
    Parallel,
    
    /// 全局独占。所有其他正在跑的工具等完，自己也独占跑。
    /// 用于 Bash 这种无法精确建模的工具。
    SerialGlobal,
}
```

### Step 2：具体工具实现

```rust
// alva-agent-extension-builtin/src/tools/read_file.rs

impl Tool for ReadFileTool {
    fn resource_keys(&self, args: &ToolInput) -> Vec<ResourceKey> {
        if let Some(path) = args.get_string("path") {
            vec![ResourceKey {
                key: path.to_string(),
                mode: LockMode::Read,
            }]
        } else {
            vec![]
        }
    }
    
    // execution_mode 用默认 Parallel
}
```

```rust
// alva-agent-extension-builtin/src/tools/edit_file.rs

impl Tool for EditFileTool {
    fn resource_keys(&self, args: &ToolInput) -> Vec<ResourceKey> {
        if let Some(path) = args.get_string("path") {
            vec![ResourceKey {
                key: path.to_string(),
                mode: LockMode::Write,
            }]
        } else {
            vec![]
        }
    }
}
```

```rust
// alva-agent-extension-builtin/src/tools/bash.rs

impl Tool for BashTool {
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::SerialGlobal   // Bash 全局独占
    }
    
    fn preprocess_args(&self, mut args: ToolInput, _ctx: &ToolContext) -> ToolInput {
        // 剥离末尾的 &（模型总是手抖在末尾加，会起后台进程）
        if let Some(cmd) = args.get_string_mut("cmd") {
            if let Some(stripped) = cmd.strip_suffix('&') {
                *cmd = stripped.trim_end().to_string();
            }
        }
        args
    }
}
```

### Step 3：调度器

```rust
// alva-kernel-core/src/run/tool_scheduler.rs

use tokio::sync::{Mutex, Semaphore};
use std::collections::HashMap;
use std::sync::Arc;

pub struct ToolScheduler {
    /// key → rwlock (多读一写)
    locks: Mutex<HashMap<String, Arc<tokio::sync::RwLock<()>>>>,
    
    /// 全局独占信号量（给 SerialGlobal 工具用）
    global_serial: Arc<Mutex<()>>,
    
    /// 正在运行的工具数（SerialGlobal 需要等到 0）
    running: Arc<AtomicUsize>,
}

impl ToolScheduler {
    pub async fn schedule_batch(
        &self,
        tool_uses: Vec<(ToolUse, Arc<dyn Tool>)>,
        ctx: ToolContext,
    ) -> Vec<ToolResult> {
        let mut handles = vec![];
        
        for (tu, tool) in tool_uses {
            let scheduler = self.clone();
            let ctx = ctx.clone();
            
            let handle = tokio::spawn(async move {
                match tool.execution_mode() {
                    ExecutionMode::SerialGlobal => {
                        // 等全局独占
                        let _guard = scheduler.global_serial.lock().await;
                        // 等所有其他 running 清零
                        scheduler.wait_running_drained().await;
                        execute_tool_with_lifecycle(tu, tool, ctx).await
                    }
                    ExecutionMode::Parallel => {
                        // 获取所有 resource keys 的锁
                        let guards = scheduler.acquire_locks(&tool.resource_keys(&tu.args)).await;
                        scheduler.running.fetch_add(1, Ordering::SeqCst);
                        let result = execute_tool_with_lifecycle(tu, tool, ctx).await;
                        scheduler.running.fetch_sub(1, Ordering::SeqCst);
                        drop(guards);
                        result
                    }
                }
            });
            
            handles.push(handle);
        }
        
        futures::future::join_all(handles).await
            .into_iter()
            .map(|r| r.unwrap())
            .collect()
    }
    
    async fn acquire_locks(&self, keys: &[ResourceKey]) -> Vec<RwLockGuard<'_, ()>> {
        // 按字典序排序避免死锁
        let mut sorted = keys.to_vec();
        sorted.sort_by(|a, b| a.key.cmp(&b.key));
        
        let mut guards = vec![];
        for key in sorted {
            let lock = self.get_or_create_lock(&key.key).await;
            let guard = match key.mode {
                LockMode::Read => lock.read_owned().await,
                LockMode::Write => lock.write_owned().await,
            };
            guards.push(guard);
        }
        guards
    }
    
    // ... 其他辅助方法
}

async fn execute_tool_with_lifecycle(
    tu: ToolUse,
    tool: Arc<dyn Tool>,
    ctx: ToolContext,
) -> ToolResult {
    // 1. Preprocess
    let args = tool.preprocess_args(tu.args, &ctx);
    
    // 2. Plugin tool.call hook
    // ...
    
    // 3. HITL approval
    // ...
    
    // 4. 执行
    let result = tool.call(args, ctx).await;
    
    // 5. Plugin tool.result hook
    // ...
    
    result
}
```

### Step 4：集成到 `run_agent`

```rust
// alva-kernel-core/src/run.rs

async fn run_agent(...) -> ... {
    let scheduler = ToolScheduler::new();
    
    loop {
        let response = llm.call(&messages).await?;
        
        // 解析 tool_use blocks
        let tool_uses: Vec<_> = response.tool_uses()
            .map(|tu| (tu, tool_registry.get(&tu.name)))
            .collect();
        
        if tool_uses.is_empty() {
            break;
        }
        
        // 批量调度（并发/串行由 scheduler 决定）
        let results = scheduler.schedule_batch(tool_uses, ctx.clone()).await;
        
        // 把 results 作为 tool_result 回注 messages
        messages.extend(tool_results_to_messages(results));
    }
}
```

---

## 集成点

- 修改 `alva-kernel-abi::Tool` trait —— 加 3 个默认方法
- 修改所有 `alva-agent-extension-builtin::tools/*` 实现对应 tool —— 补 resource_keys
- 新建 `alva-kernel-core::run::tool_scheduler` 模块
- 修改 `alva-kernel-core::run::run_agent` 主循环集成 scheduler

---

## 潜在风险

### 1. 死锁

多个 tool 同时申请多个 key 的锁可能死锁。**避免方法**：按 key 字典序排序后申请。

### 2. 锁粒度

如果所有 path 都在同一个 key 下，退化成全局锁。**设计原则**：锁的 key 必须是**具体资源**（文件 URI），不是概念（"文件系统"）。

### 3. 工具误声明 `SerialGlobal`

过度保守 → 性能下降。**建议**：只有 Bash 这种"无法精确建模"的才用 SerialGlobal。其他工具尽量用 resource_keys。

### 4. 向后兼容

现有工具没实现 `resource_keys` → 默认空 vec → 全部并发。**风险**：原本隐式串行的工具（如 MCP 写类工具）现在并发了。**解决**：review 所有现有 tool，逐个确认。

---

## 优先级

**中等**。没这个 Alva 能跑，但：

- Prompt 里 "parallel by default" 没实际支撑
- 多 agent 场景下会踩写冲突
- 用户体验感知：agent 看起来很慢（一个一个读文件）

建议在做"并发 agent"或"Tauri GUI 体验优化"时推进这个。

---

## 验证

调度器工作正常的 indicator：

1. **Read 密集任务加速显著**：读 10 个文件应该接近 1 个文件的时间（不是 10 倍）
2. **同文件 edit 观察串行**：quick log 里看到 "EditFile acquired write lock on /foo"
3. **Bash 独占**：Bash 执行时没有其他工具在跑
4. **多 agent 不踩冲突**：两个子 agent 同时改不同文件不互相等
