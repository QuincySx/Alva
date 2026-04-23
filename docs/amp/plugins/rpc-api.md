# Plugin ↔ Host RPC API

> Plugin 能调用的 Host 能力完整清单。

---

## 传输协议

JSON-RPC 2.0 over stdout/stdin（NDJSON）：

```
Plugin → Host (request):
{"type": "request", "id": "p-1", "method": "ui.notify", "params": {"message": "Hello"}}

Host → Plugin (response):
{"type": "response", "id": "p-1", "result": null}

Host → Plugin (request):
{"type": "request", "id": "h-5", "method": "agent.end", "params": {...}}

Plugin → Host (response):
{"type": "response", "id": "h-5", "result": {"action": "continue"}}
```

---

## Host 提供给 Plugin 的 API

```ts
// 简化版（从反编译还原）
interface HostAPI {
  // UI 交互
  ui: {
    notify(message: string): Promise<void>;
    input(options: InputOptions): Promise<string>;
    confirm(options: ConfirmOptions): Promise<boolean>;
  };
  
  // AI 调用（plugin 内用 LLM）
  ai: {
    ask(question: string): Promise<{
      result: "yes" | "no" | "uncertain",
      probability: number,
      reason: string
    }>;
  };
  
  // 系统交互
  system: {
    open(url: string | URL): Promise<void>;
    get ampURL(): URL;
    get executor(): { kind: "local" | "dtw" | "unknown" };
  };
  
  // Thread 操作
  thread: {
    append(messages: Message[]): Promise<void>;
  };
}
```

---

## `ui.notify`

弹底部通知（类似 VS Code 的 statusbar message）。

```ts
await host.ui.notify("Tests passed ✓");
```

**headless 环境**：写到 stderr。

---

## `ui.input`

弹输入框等待用户输入。

```ts
let answer = await host.ui.input({
  title: "Custom commit message?",
  placeholder: "feat: ...",
  default: "",
  multiline: false
});
```

**headless 环境**：写 "Input dialogs are not available outside the TUI" 到 stderr，返回 default 或抛错。

---

## `ui.confirm`

弹确认对话框。

```ts
let proceed = await host.ui.confirm({
  title: "Really delete?",
  message: "This will delete 12 files.",
  confirmLabel: "Delete",
  cancelLabel: "Cancel"
});
```

**headless 环境**：默认 `false` + 警告。

---

## `ai.ask` ⭐

Plugin 专用的 LLM 调用入口。**不走 tool 机制**，plugin 自己可以向模型提问。

```ts
let decision = await host.ai.ask("Does this diff look like it's addressing the real root cause of the bug?");
// decision.result: "yes" | "no" | "uncertain"
// decision.probability: 0.0-1.0
// decision.reason: string（人类可读解释）
```

**关键用法**：plugin 想基于 LLM 判断但**不需要把这个判断变成 LLM 主循环里的 tool**。例：

```ts
plugin({
  onToolResult: async (params, host) => {
    if (params.name === "edit_file") {
      // 判断这次 edit 是不是修了个真实的 bug，还是在绕过 compiler error
      let analysis = await host.ai.ask(
        `Did this edit address a root cause, or suppress an error? 
        Edit: ${JSON.stringify(params.input)}`
      );
      if (analysis.result === "no" && analysis.probability > 0.7) {
        await host.ui.notify(`⚠️  Possible error suppression: ${analysis.reason}`);
      }
    }
  }
});
```

---

## `system.open`

打开 URL / 文件（调系统 `open` / `start` / `xdg-open`）。

```ts
await host.system.open("https://ampcode.com/threads/T-xxx");
await host.system.open(new URL("file:///Users/alice/file.txt"));
```

---

## `system.ampURL` / `system.executor`

只读属性。

```ts
let url = host.system.ampURL;           // URL("https://ampcode.com")
let exec = host.system.executor;         // { kind: "local" | "dtw" | "unknown" }
```

Plugin 可以根据 executor kind 做不同行为（例如 DTW 下不能 open 本地 URL）。

---

## `thread.append`

往 thread 里追加 message。

```ts
await host.thread.append([
  { role: "user", content: [{ type: "text", text: "Wait, also check X" }] }
]);
```

**用途**：
- Plugin 发现需要干预，注入 guidance
- 把 plugin 的 side-effect 结果（如 lint output）作为 `info` message 记录
- 追加 assistant context（让后续对话知道 plugin 做了什么）

**注意**：不能修改已有 messages，只能 append。

---

## 内置 plugin 的 API 扩展

Amp 还有**内置 plugin**（`q3R` / `internalPlugins`），它们可能有额外 API（二进制里没完全暴露）。

---

## Plugin SDK（`@ampcode/plugin`）

Plugin 作者通过 Bun 虚拟模块使用 SDK：

```ts
// plugin.ts
import { plugin } from "@ampcode/plugin";

plugin({
  name: "my-plugin",
  ...
});
```

**SDK 实现**（从反编译提取，简化）：

```js
// Bun plugin 注册虚拟模块
Bun.plugin({
  name: "ampcode-plugin-resolver",
  setup(r) {
    r.onResolve({ filter: /^@ampcode\/plugin(?:\/.*)?$/ }, (v) => {
      return { path: v.path, namespace: "ampcode-plugin-types" };
    });
    r.onLoad({ filter: /.*/, namespace: "ampcode-plugin-types" }, (v) => {
      return { contents: "export {}", loader: "ts" };
    });
  }
});

// 在 plugin 子进程里实际 API 通过 stdout/stdin RPC 实现
let responseMap = new Map();
let seq = 0;

function requestToHost(method, params) {
  let id = `p-${++seq}`;
  return new Promise((resolve, reject) => {
    responseMap.set(id, { resolve, reject });
    console.log(JSON.stringify({ type: "request", id, method, params }));
  });
}

// 读 stdin 处理 host 的 request + response
process.stdin.on("data", (chunk) => {
  for (let line of chunk.split("\n")) {
    if (!line.trim()) continue;
    let msg = JSON.parse(line);
    if (msg.type === "response") {
      let handler = responseMap.get(msg.id);
      if (msg.error) handler.reject(msg.error);
      else handler.resolve(msg.result);
      responseMap.delete(msg.id);
    } else if (msg.type === "request") {
      dispatchHostRequest(msg);
    }
  }
});
```

---

## Plugin Context

Plugin 初始化时收到 context 对象：

```ts
{
  name: string,                     // plugin name
  $: (cwd: string) => Shell,        // 便利的 shell runner（带 cwd）
  ui: { notify, input, confirm },
  ai: { ask },
  system: { open, ampURL, executor },
  thread: { append },
  // hook registration methods
  onAgentStart(handler),
  onAgentEnd(handler),
  onToolCall(handler),
  onToolResult(handler),
  onConfigChange(handler),
  // ...
}
```

---

## 对 Alva 的启发

你们的 `alva-app-extension-loader` (AEP) 是 JSON-RPC 方向。对照 Amp 的 API 表面：

1. **`ai.ask` ⭐** —— 这是你们可能没有的能力。Plugin 能问 LLM 简单问题，不用暴露成 main agent 的 tool。加一个 `AiAskService` 放 bus 上。

2. **`ui.notify / input / confirm` 标准三件套** —— 确认你们 headless 场景的降级行为（Amp 是写 stderr + 默认拒绝）。

3. **`thread.append`** —— Plugin 能注入 message 到对话。你们 `BusWriter.provide()` 机制能支持吗？

4. **`system.executor` 告诉 plugin 当前环境** —— local vs remote，让 plugin 行为适应。

5. **Bun virtual module 技巧** —— SDK 用 `@scope/plugin` 作 import 名，实际运行时走 RPC。类型声明 + runtime 实现分离的漂亮做法。你们 Python SDK 也可以类似。
