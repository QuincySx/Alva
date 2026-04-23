# 工具架构 —— 数据结构与运行模型

---

## Tool 定义对象

每个工具都是一个 `{ spec, fn, preprocessArgs? }` 结构：

```js
L2R = {                                   // Bash 工具示例
  spec: {
    name: Y8,                             // "Bash"
    description: M2R,                     // 完整描述文本（大段）
    inputSchema: {
      type: "object",
      properties: {
        cmd: { type: "string", description: "The shell command to execute" },
        cwd: { type: "string", description: "Absolute path..." }
      },
      required: ["cmd"]
    },
    source: "builtin",
    meta: { disableTimeout: true },      // Bash 不能超时
    executionProfile: {
      serial: true,                       // Bash 全局串行
      resourceKeys: () => []              // 无精细锁
    }
  },
  fn: HzT,                                // 实际 handler
  preprocessArgs: (T, R) => {             // 可选：参数清洗
    if (/(?<!&)\s*&\s*$/u.test(T.cmd)) {
      return { ...T, cmd: T.cmd.replace(/(?<!&)\s*&\s*$/u, "").trim() };
    }
    return T;
  }
}
```

---

## `spec.name`

工具对外暴露的名字。有两种写法：

### 直接字符串

```js
name: "load_skill"
```

### 符号引用（二进制里的常见写法）

```js
name: is,     // is = "load_skill"
// 或
name: H0T,    // H0T = "todo_write"
```

这些符号在 bundler 打包时解析成字面量。Mapping 见 [`../prompts/placeholder-dictionary.md`](../prompts/placeholder-dictionary.md)。

---

## `spec.description`

是一段**长文本**（经常几百到几千 tokens），直接注入到 LLM 的工具描述里。里面可以含：

- 什么时候用 / 什么时候不用
- 参数使用示例（以 JSON 形式）
- 特殊限制（如 Read 要绝对路径）
- 对某些错误的 troubleshooting

示例（`load_skill`）：

```
Load a specialized skill when the task matches one of the skill descriptions 
from the system prompt.

Use this tool to inject that skill's instructions and bundled resources into 
the current conversation. A loaded skill may provide:
- task-specific workflow guidance
- references to scripts, templates, or files in the skill directory
- additional builtin or MCP tools that become available after loading

- the user explicitly asks for a skill by name
- the task clearly matches a skill description from the system prompt

You usually only need to load a skill once per context window. After it is 
loaded, continue following its instructions instead of reloading it.

- name: The name of the skill to load (must match one of the skills listed below)

Example: To use the web-browser skill for interacting with web pages, call 
this tool with name: "web-browser"
```

---

## `spec.inputSchema`

JSON Schema，可以手写也可以从 zod 转：

```js
// 手写
inputSchema: {
  type: "object",
  properties: {
    path: { type: "string", description: "Absolute path" }
  },
  required: ["path"],
  additionalProperties: false
}

// 从 zod 转
inputSchema: X.toJSONSchema(qX)    // X = zod, qX = z.object({...})
```

Amp 混用两种方式。内部复杂工具（如 `edit_file`）用 zod，简单工具直接写 JSON Schema。

---

## `spec.source`

工具来源的类型标签。**用于 UI 分组 + 同名工具覆盖规则**。

```ts
type Source = 
  | "builtin"                           // 二进制内置
  | "mcp-workspace"                     // <workspace>/.amp/settings.json
  | "mcp-global"                        // ~/.amp/settings.json
  | "mcp-flag"                          // --mcp-server CLI flag
  | "mcp-other"                         // 其他 MCP
  | { toolbox: string }                 // .agents/agents/<path>.md
  | "plugin"                            // .amp/plugins/*.ts
  | "other"
```

Toolbox 类型额外带路径信息，便于跟踪来源。

---

## `spec.meta`

可选元数据。已观察到的字段：

| 字段 | 语义 |
|---|---|
| `disableTimeout: true` | 不给 fn 设超时。用于长运行工具（Task / Oracle / Walkthrough / Bash）|

---

## `spec.executionProfile`

**调度器读的东西**。详见 [`execution-scheduler.md`](./execution-scheduler.md)。

```js
executionProfile: {
  resourceKeys: (args) => [{ key: string, mode: "read" | "write" }],
  serial?: boolean
}
```

- `resourceKeys(args)` —— 返回这次调用需要的资源锁。调度器用多读一写语义判断能否并发。
- `serial: true` —— 覆盖所有锁，工具执行时全局独占。Bash 默认打开。

---

## `fn(args, ctx) → Observable`

工具实际逻辑。**不是普通 Promise**，而是返回一个 RxJS-like Observable 流。

### Context 对象（`ctx`）

```js
{
  configService,                  // 读 config
  filesystem,                     // 文件系统抽象
  dir,                            // working directory
  toolMessages,                   // 历史 tool calls
  thread,                         // 当前 thread 引用
  toolService,                    // 工具注册表
  fileChangeTracker,              // 文件改动追踪器
  mcpService,                     // MCP 客户端
  skillService,                   // Skills 注册表
  threadService,                  // 远程 thread API
  toolUseID,                      // 当前 tool_use 的 ID
  userInput,                      // 触发本次 turn 的用户输入
  // ...
}
```

### 返回的 Observable 流

每条事件形如：

```ts
type ToolRun =
  | { status: "in-progress", progress?: any }
  | { status: "done",        result: any }
  | { status: "error",       error: { message: string, errorCode?: string } }
  | { status: "cancelled" }
  | { status: "rejected-by-user" }
  | { status: "blocked-on-user" }
```

### 典型用法（Read 工具的简化版）

```js
fn: ({ args: { path, read_range } }, { filesystem }) => {
  return R8(async (subscriber) => {
    // R8 = Observable 包装器，把 async function 转成 Observable
    try {
      let contents = await filesystem.readFile(path);
      // ...
      return {
        status: "done",
        progress: {},
        result: { absolutePath: path, content: ..., trackFiles: [uri] }
      };
    } catch (e) {
      return { status: "error", error: { message: e.message } };
    }
  });
}
```

### 流式进度

子 agent 工具（Task / Oracle）会在执行中不断发 `in-progress` 事件：

```js
subscriber.next({
  status: "in-progress",
  progress: {
    threadID: subThreadID,
    iteration: 3,
    input: "...",
    output: "...",
    transcript: [...]
  }
});
```

UI 实时渲染 transcript。这让用户能看到"子 agent 正在查找 auth 流"这种中间状态。

---

## `preprocessArgs` 钩子

可选的参数预处理器，在 schema 验证之后、`fn` 调用之前执行。

**Bash 的例子**：

```js
preprocessArgs: (T, R) => {
  // 模型经常手抖在命令末尾加 & 想后台跑
  // 但我们明确告诉过它不要用 &（见 Rush Mode prompt）
  // 所以这里兜底剥离，不让它真的起后台进程
  if (/(?<!&)\s*&\s*$/u.test(T.cmd)) {
    return {
      ...T,
      cmd: T.cmd.replace(/(?<!&)\s*&\s*$/u, "").trim()
    };
  }
  return T;
}
```

**设计哲学**：**"模型总是犯同一个错" 的地方，用 preprocessor 兜底而不是靠 prompt 说教。** prompt 治教育失败，preprocessor 治不听话。

---

## Tool 调用的完整生命周期

```
1. LLM 输出 tool_use block { id, name, input }
   │
   ▼
2. 根据 name 查 tool registry（builtin + mcp + plugin + toolbox 合并表）
   │
   ▼
3. inputSchema 验证（zod.parse 或 ajv）—— 失败直接返回 error tool_result
   │
   ▼
4. preprocessArgs (如果有) 对 input 做清洗
   │
   ▼
5. executionProfile.resourceKeys(input) 申请锁
   │
   ├── 锁到手 → 进入 6
   └── 锁等别人 → 排队
   │
   ▼
6. 触发 plugin.tool.call hook（所有订阅的 plugin 都被通知）
   │
   ▼
7. HITL 审批（PermissionMode / SecurityGuard 判断是否需要用户 approve）
   │
   ├── allow / 已缓存 → 进入 8
   ├── ask → 发 status: "blocked-on-user"，等用户 approve
   └── deny → 发 status: "rejected-by-user"
   │
   ▼
8. fn(args, ctx) 调用，订阅返回的 Observable
   │
   ▼
9. 每个 in-progress 事件通过 AgentEvent → 更新 UI
   │
   ▼
10. 最终事件 (done | error | cancelled)
    │
    ▼
11. 触发 plugin.tool.result hook
    │
    ▼
12. 释放锁
    │
    ▼
13. 把结果包装成 tool_result block 回注到 LLM 对话
    - 输出 > 阈值 → 截断 + "Tool result truncated..." 提示
    - is_error = (status === "error" || "cancelled" || "rejected-by-user")
```

---

## 设计启发

1. **`spec.description` 不省 token** —— 描述越详细，模型用得越好。Amp 的 Task 工具描述将近 1500 tokens。
2. **执行轨迹用 Observable 而不是 Promise** —— 支持流式 progress，支持取消，支持多订阅。
3. **preprocessArgs 是 prompt 失效的兜底** —— 不要迷信 prompt engineering，重要的 invariant 用代码强制。
4. **Source 分层 + 同名覆盖** —— 让用户可以在 workspace 里 override builtin 工具行为。
