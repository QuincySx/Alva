# 工具执行调度器

> Amp 默认**所有工具并发跑**，只有写冲突才串行。这不是 prompt 的"努力建议"，是调度器的**硬保证**。

---

## 调度输入：`executionProfile`

每个工具声明自己的资源需求：

```js
executionProfile: {
  resourceKeys: (args) => [{ key: string, mode: "read" | "write" }],
  serial?: boolean    // true = 全局独占
}
```

---

## 示例：各工具的资源锁

### Bash —— 全局串行

```js
Y8Bash = {
  executionProfile: {
    serial: true,          // 全局独占
    resourceKeys: () => []  // 不申请具体锁
  }
}
```

**原因**：shell 命令可能修改任何东西（环境变量、后台进程、文件系统），无法精确建模。默认最保守。

### Read —— 完全并发

```js
ReadTool = {
  executionProfile: {
    resourceKeys: (T) => {
      if (T && typeof T === "object" && "path" in T && typeof T.path === "string") {
        return [{ key: T.path, mode: "read" }];
      }
      return [];
    }
  }
}
```

**效果**：
- 同一文件可以被多个 Read 并发读（多读一写语义）
- 不阻塞其他文件的 edit

### edit_file —— 同文件串行

```js
EditFileTool = {
  executionProfile: {
    resourceKeys: (T) => {
      if (T && typeof T === "object" && "path" in T && typeof T.path === "string") {
        return [{ key: T.path, mode: "write" }];
      }
      return [];
    }
  }
}
```

**效果**：
- 同文件的 edit 必须串行（write 锁）
- 其他文件的 edit 可以并发
- Read 同一文件时也会等 edit 完成（write 阻塞 read）

### Grep / glob / codebase_search_agent —— 完全并发

```js
executionProfile: {
  resourceKeys: () => []
}
```

只读搜索，没锁。

### Task / Oracle —— 无锁但标记 disableTimeout

```js
executionProfile: {
  resourceKeys: () => []
},
meta: { disableTimeout: true }
```

子 agent 可能跑很久，不设超时。

---

## 锁语义

经典的**多读一写**（multi-reader, single-writer）：

```
一个 key 的锁状态：
  - 空闲           → 新申请 read 或 write 都能拿
  - 被 read 占有   → 再来 read 可共享；write 要等所有 read 释放
  - 被 write 占有  → 任何新申请都要等
```

Amp 用这个语义决定**多个 tool_use 能否在同一 turn 内并发执行**。

---

## 实际调度算法（推断）

LLM 一次可以输出多个 tool_use。调度器对这批调用：

```python
# 伪代码
def schedule(tool_uses):
    queues = {}   # key → deque of waiters
    running = []
    
    for tu in tool_uses:
        tool = registry.get(tu.name)
        keys = tool.executionProfile.resourceKeys(tu.args)
        
        if tool.executionProfile.serial:
            # 全局独占：等所有当前 running 完成
            wait_for_all(running)
            execute_alone(tu)
        else:
            # 按 keys 申请锁
            for k in keys:
                acquire_lock(k, mode=k.mode)   # 阻塞等待
            start_async(tu)
    
    await_all(running)
```

---

## `kA(path).acquire()` / `.release()` 机制

从代码里看到的具体调用模式：

```js
// undo_edit 工具里
let uri = parseURI(args.path);
await kA(uri).acquire();     // 获取 path 的全局锁
try {
  let lastEdit = await tracker.getLastEdit(uri);
  await writeFile(uri, lastEdit.oldContent);
  await lastEdit.revert();
  return { status: "done", result: diff };
} finally {
  kA(uri).release();          // 释放
}
```

`kA` 是一个全局的 path → Mutex 映射。Edit 类工具通过 `kA(path).acquire()` 拿独占锁。

**观察**：`kA` 是**比 resourceKeys 更细粒度的第二道保险**。`resourceKeys` 声明调度意图，`kA` 在 fn 里做实际互斥（防止多个 fn 内部的异步操作互相踩踏）。

---

## Plugin 的 tool.call / tool.result hook 插入点

工具执行流程里，plugin 能介入两个点：

```
lock acquired
  │
  ▼
[plugin.tool.call hook]          ← 所有订阅的 plugin 被通知
  │
  │ plugin 可以：
  │   - logger 记录
  │   - 修改参数（通过 thread.append 注入新 user message）
  │   - 调用 ai.ask 决定要不要干预
  │
  ▼
HITL 审批
  │
  ▼
fn(args, ctx)
  │
  ▼
[plugin.tool.result hook]        ← 结果出来后通知
  │
  ▼
lock released
```

详见 [`../plugins/hooks.md`](../plugins/hooks.md)。

---

## HITL 审批（SecurityGuard / PermissionMode）

`PermissionMode` 有四种：

| Mode | 行为 |
|---|---|
| `Ask` | 每次弹窗问用户 |
| `AcceptEdits` | Edit 类自动允许，危险类仍问 |
| `Plan` | **任何 edit 都拒绝**（只能读 / 搜索） |
| `AllowAll` | 一律允许（CI / `--dangerously-allow-all`） |

还有 session 级缓存 `PermissionCache`：

- `always-allow(cmd)` —— 这个 cmd 本 session 不用再问
- `always-deny(cmd)` —— 本 session 不用问直接拒

**Execute mode 特别**：
```
Error: The Bash tool tried to run a command that isn't allowlisted. 
Rerun with --dangerously-allow-all to bypass, or add to the command 
allowlist in permissions.
```

execute mode 下**任何 ask 都自动视为 reject**，不等弹窗（headless 环境没法弹）。

---

## 取消传播（CancellationToken）

每个 tool 调用得到一个 `abortSignal`（或等价的 `CancellationToken`）。用户 Ctrl+C：

```
user Ctrl+C
  │
  ▼
abortController.abort()
  │
  ├──▶ 所有 in-flight fn 的 ctx.signal.aborted = true
  │    fn 应该检查 signal 并尽快退出
  │
  ├──▶ 调度队列清空（pending tool_use 标记为 cancelled）
  │
  └──▶ LLM stream 关闭
```

工具结束状态变成 `status: "cancelled"`。回注 LLM 的 tool_result 会标记 `is_error: true`。

---

## 对 Alva 的启发

**Alva 需要这个的核心理由**：prompt 里写 "parallel by default" 如果没有调度器支撑，模型会知道，下次就不并发了（因为它观察到并发没收益）。

建议实现方案：

```rust
// alva-kernel-abi/src/tool.rs
pub trait Tool {
    fn resource_keys(&self, args: &ToolInput) -> Vec<ResourceKey> {
        vec![]  // 默认无锁
    }
    
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Parallel  // 默认并行
    }
}

pub struct ResourceKey {
    pub key: String,
    pub mode: LockMode,
}

pub enum LockMode { Read, Write }
pub enum ExecutionMode { Parallel, SerialGlobal }

// alva-kernel-core/src/run.rs 里的 tool 调度器
async fn schedule_tool_batch(tools: Vec<ToolUse>) -> Vec<ToolResult> {
    let mut handles = vec![];
    for tu in tools {
        let locks = acquire_locks(&tu.resource_keys()).await;
        if tu.serial_global {
            wait_all_running().await;
        }
        handles.push(tokio::spawn(async move {
            let result = tu.fn(tu.args, ctx).await;
            release_locks(locks);
            result
        }));
    }
    futures::future::join_all(handles).await
}
```

具体设计见 [`../alva-learnings/resource-lock-scheduler.md`](../alva-learnings/resource-lock-scheduler.md)。
