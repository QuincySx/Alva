# Plugin 系统目录

> `.amp/plugins/*.ts` 子进程插件系统。和你们 `alva-app-extension-loader` (AEP) 方向相同。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`hooks.md`](./hooks.md) | Agent / tool / configuration lifecycle hooks 完整清单 |
| [`rpc-api.md`](./rpc-api.md) | Plugin ↔ Host 双向 RPC 表面 |
| [`debugging.md`](./debugging.md) | `amp plugins list / exec` 调试命令 |

## 核心特征

```
.amp/plugins/foo.ts (Bun TypeScript)
  │ Bun subprocess
  │
  │ JSON-RPC 2.0 over stdout/stdin
  ▼
Amp Host
  │
  ├── Plugin → Host:  ui.notify / ai.ask / system.open / thread.append ...
  └── Host → Plugin:  agent.start / agent.end / tool.call / tool.result / configuration.change
```

## 最独特的 feature：`agent.end` 能强制 continue

```js
async function J(Y) {
  let handlers = V("agent.end");
  let results = await Promise.all(handlers.map(h => h.process.requestAgentEnd(Y, spanID)));
  for (let yT of results) {
    if (yT.action === "continue") return yT;   // 任一插件要求继续 → outer loop 继续
  }
  return { action: "done" };
}
```

这让插件能实现 "自动测试 loop"、"验证守护" 这类守护模式。

## OTEL 一等公民

每个 plugin hook 都自动包在 `startActiveSpan("plugin", {label: "..."})` 里。调试性能 / 故障排查直接看 trace。
