# Plugin Lifecycle Hooks

> Plugin 能订阅的 Host 事件完整清单。

---

## 事件分发器

```js
// 简化的 dispatcher（从反编译重构）
function g5R(T) {
  switch (T.method) {
    case "event":                  return handleGenericEvent(T);
    case "configuration.change":   return handleConfigChange(T);
    case "tool.call":              return handleToolCall(T);
    case "tool.result":            return handleToolResult(T);
    case "agent.start":            return handleAgentStart(T);
    case "agent.end":              return handleAgentEnd(T);
  }
}
```

---

## `agent.start`

**触发时机**：用户发 message 或 agent 开始新 turn 时。

**参数（推断）**：
```ts
{
  method: "agent.start",
  params: {
    threadID: string,
    turnID: string,
    userInput: string,
    timestamp: number
  }
}
```

**典型用途**：
- 开始 tracing span
- 验证环境（有没有 lock 文件、有没有未提交改动）
- 自动加 context（如 git branch info）

---

## `agent.end` ⭐

**触发时机**：agent 决定 `stop_reason: "end_turn"` 时。

**参数**：
```ts
{
  method: "agent.end",
  params: {
    threadID: string,
    turnID: string,
    result: "success" | "error" | "cancelled",
    duration: number
  }
}
```

**返回值** ⭐：

```ts
{
  action: "done" | "continue",
  reason?: string,    // 可选：给 log 说明
  message?: string    // 可选：要追加到 thread 的 message
}
```

**关键设计**：**任一** plugin 返回 `continue` 就让 agent **继续新一轮**。这让插件能实现：

```
auto-test plugin: agent.end hook
  → 跑 npm test
  → 如果 fail
      → 返回 { action: "continue", message: "Tests failed: ..." }
      → agent 继续修
  → 如果 pass
      → 返回 { action: "done" }
```

**实现细节**：

```js
async function J(Y, iT = Yo) {
  let handlers = V("agent.end");
  if (handlers.length === 0) return { action: "done" };
  
  let results = await Promise.all(handlers.map(h => {
    let pluginName = R_(h.uri);
    return iT.startActiveSpan("plugin", {
      label: `${pluginName}#agent.end`,
      attributes: { plugin: { pluginName, hook: "agent.end" } }
    }, async (span) => {
      C.set(span.id, span);
      try {
        return await h.process.requestAgentEnd(Y, span.id);
      } catch (e) {
        logger.debug("Failed to request agent.end from plugin", { uri: h.uri, error: e });
        return { action: "done" };    // 失败降级为 done
      } finally {
        C.delete(span.id);
      }
    });
  }));
  
  for (let r of results) {
    if (r.action === "continue") return r;  // 短路返回
  }
  return { action: "done" };
}
```

**容错**：某个 plugin 抛异常不影响其他 plugin / agent 自己。失败的 plugin 视为 `done`。

---

## `tool.call`

**触发时机**：工具被调度器选中准备执行前。HITL 审批**之后**、`fn` 调用**之前**。

**参数**：
```ts
{
  method: "tool.call",
  params: {
    threadID: string,
    toolUseID: string,
    name: string,
    input: any
  }
}
```

**典型用途**：
- 审计日志
- 记录敏感操作
- 注入上下文（通过 `thread.append`）

---

## `tool.result`

**触发时机**：工具执行完成（`done` / `error` / `cancelled`）。

**参数**：
```ts
{
  method: "tool.result",
  params: {
    threadID: string,
    toolUseID: string,
    name: string,
    input: any,
    result: any,        // 或 null
    error: Error | null,
    status: string,
    duration: number
  }
}
```

**典型用途**：
- 统计工具成功率
- 错误聚合 / 告警
- 写 diff 到外部审计系统

---

## `configuration.change`

**触发时机**：`~/.amp/settings.json` 或 `<workspace>/.amp/settings.json` 改动。

**参数**：
```ts
{
  method: "configuration.change",
  params: {
    config: AmpConfig,
    changedKeys: string[]
  }
}
```

**典型用途**：
- 热加载配置
- 重新初始化依赖该配置的资源
- 通知用户配置生效

---

## `event`（通用自定义）

**触发时机**：任意 plugin 或 host 代码调 `emitEvent(name, data)` 时。

**参数**：
```ts
{
  method: "event",
  params: {
    event: string,
    data: any,
    span?: string
  }
}
```

**典型用途**：Plugin 间通信（不走 tool 机制）、自定义 UI 事件。

---

## Plugin 注册

Plugin 代码里用 SDK 注册 hook：

```ts
import { plugin } from "@ampcode/plugin";

plugin({
  name: "auto-lint",
  
  // 订阅 tool.result
  onToolResult: async (params, host) => {
    if (params.name === "edit_file") {
      await host.system.exec("npm run lint");
    }
  },
  
  // 订阅 agent.end
  onAgentEnd: async (params, host) => {
    let lintResult = await runLint();
    if (lintResult.hasErrors) {
      return {
        action: "continue",
        message: `Lint errors found:\n${lintResult.errors}`
      };
    }
    return { action: "done" };
  },
  
  // 注册工具
  tools: [{
    name: "run_linter",
    description: "...",
    inputSchema: { ... },
    handler: async (args) => { ... }
  }],
  
  // 注册命令（给用户在 TUI 输入 /command 调用）
  commands: [{
    category: "dev",
    title: "Toggle linter",
    handler: async () => { ... }
  }]
});
```

---

## OpenTelemetry 集成

每个 plugin hook 自动包 span：

```js
iT.startActiveSpan("plugin", {
  label: `${pluginName}#${hookName}`,
  attributes: {
    plugin: { pluginName, hook: hookName }
  }
}, async (span) => { ... })
```

Plugin 可以通过 `host.span.event(...)` 在自己的 span 里加事件，方便 Jaeger / Tempo / Honeycomb 等可视化。

**Plugin 接收 `spanID` 参数**，可以创建 child span：

```js
plugin.requestAgentEnd(params, spanID)
// plugin 内部：
host.tracing.child(spanID, { name: "inside plugin" }, async (childSpan) => {
  // ...
});
```

---

## 对 Alva 的启发

你们 `HooksExtension` 已经是这个方向。对照 Amp 的关键细节：

1. **`agent.end` 返回 `{action: "continue"}` 能影响 outer loop** —— 你们的 `on("agent.end", handler)` 返回值要能控制 `PendingMessageQueue`。如果目前是 fire-and-forget，补上。

2. **短路返回** —— 任一 plugin 要 continue 就 continue，不用等全部。减少延迟。

3. **失败降级** —— plugin 抛异常不影响其他。

4. **OTEL 一等公民** —— 每个 hook 自动有 span。生产环境排查问题的关键。

5. **`emitEvent` 通用事件** —— 不只是固定的几个 lifecycle hook，允许 plugin 间通信。

6. **`tool.call` + `tool.result` 两点插入** —— 你们有没有确认都能拦？只有 `tool.call` 拦不住"动作已经发生"的场景。
