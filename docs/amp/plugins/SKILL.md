---
name: amp-plugins
description: Amp 的 .amp/plugins/ 子进程插件系统 —— JSON-RPC 2.0 stdio、lifecycle hooks、agent.end → continue 能力、ai.ask RPC、amp plugins exec 调试命令。想做插件系统或理解 Plugin 如何介入 agent loop 时加载。
trigger_words:
  - plugin
  - amp plugins
  - plugin hook
  - agent.end
  - tool.result
  - tool.call
  - configuration.change
  - ai.ask
  - thread.append
  - ui.notify
  - amp plugins exec
  - amp plugins list
  - plugin JSON-RPC
  - plugin subprocess
  - ampcode/plugin
---

# Amp Plugins System

`.amp/plugins/*.ts` 子进程插件的完整设计。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./hooks.md` | 完整 lifecycle hooks 清单 + agent.end 能强制 continue 的实现 + OTEL 集成 | 想懂 plugin 能在哪里介入 |
| `./rpc-api.md` | Plugin → Host RPC 完整表面 (ui / ai / system / thread) | 想懂 plugin 能做什么 |
| `./debugging.md` | `amp plugins list / exec` 命令，stub host 调试模式 | 想给自己产品加 plugin 调试 |

## 架构速查

```
.amp/plugins/foo.ts (Bun TypeScript)
  │ Bun subprocess
  │
  │ JSON-RPC 2.0 over stdout/stdin
  ▼
Amp Host
  │
  ├── Plugin → Host (RPC)
  │     ui.notify / ui.input / ui.confirm
  │     ai.ask
  │     system.open / system.ampURL / system.executor
  │     thread.append
  │
  └── Host → Plugin (事件)
        event (通用自定义)
        configuration.change
        tool.call / tool.result
        agent.start / agent.end
```

## 6 种 lifecycle hook（速查）

| Hook | 时机 | 返回值影响 |
|---|---|---|
| `event` | 任意 emit | — |
| `configuration.change` | settings.json 改动 | — |
| `tool.call` | 调度器选中 tool 但 fn 未跑前 | — |
| `tool.result` | fn 完成后 | — |
| `agent.start` | 新 turn 开始 | — |
| **`agent.end`** ⭐ | turn 结束时 | **`{action: "continue"}` 能让 agent 继续循环** |

## `agent.end → continue` 的用法示例

```ts
plugin({
  name: "auto-test",
  onAgentEnd: async (params, host) => {
    let result = await host.system.exec("npm test");
    if (result.exitCode !== 0) {
      return {
        action: "continue",
        message: `Tests failed. Please fix:\n${result.stderr}`
      };
    }
    return { action: "done" };
  }
});
```

插件可以实现：
- **自动测试守护**（测试挂了让 agent 继续修）
- **自动 lint**（lint 不过强制循环）
- **安全检查**（发现疑似问题，注入 guidance 让 agent 审视）

## `ai.ask` —— plugin 独有能力

不像 LLM 主循环的 tool，plugin 可以**私下**问 LLM 简单问题：

```ts
let judgment = await host.ai.ask("Is this diff a root-cause fix?");
// { result: "yes"|"no"|"uncertain", probability, reason }
```

用于"plugin 自己做判断，不暴露为 agent 的工具"。

## `amp plugins exec` —— 调试 killer feature

**不启动完整 agent**，用 stub host 手动触发 event：

```bash
amp plugins exec ./autolint.ts tool.result --data '{
  "name": "edit_file",
  "input": {"path": "/tmp/a.ts", ...},
  "result": {"diff": "..."},
  "status": "done"
}'
```

输出把 `ui.notify` 等 RPC 都打印到终端，不真执行。**推荐 Alva 抄**，详见 `../alva-learnings/plugin-exec.md`。
