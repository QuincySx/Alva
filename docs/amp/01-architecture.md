# Amp 整体架构

> 一图看懂 Amp 是什么、各个子系统怎么串起来的。

---

## 一句话定位

Amp 是 Sourcegraph 的**闭源 AI coding agent**，形态是 CLI + 云端 orchestrator + Web UI 组合体。

**关键差异化**：它不是单一的 local agent，而是 **分布式 agent 编排系统** —— 一个 agent 指挥多个在云上跑的"execution threads"。

---

## 四个形态

Amp 同一份二进制可以以四种角色跑：

| 形态 | 触发 | 角色 |
|---|---|---|
| **Interactive TUI** | `amp`（默认）| 本地 Executor，带 Ink 风格的终端 UI |
| **Execute mode** | `amp --execute "..."` 或 stdout 非 TTY | 本地 Executor，单次运行后退出，CI 友好 |
| **Stream-JSON mode** | `amp --stream-json` | 本地 Executor，用 NDJSON 做 subprocess IPC |
| **Plugin subprocess** | 被其他 Amp 进程调起 | 跑 `.amp/plugins/*.ts` 插件逻辑 |

云端还有第五种：

| **DTW Worker** | ampcode.com 触发 | **远程 Executor**，跑在 Cloudflare Workers |

---

## 两套 System Prompt（双人设）

Amp 最被忽视的设计是它有**完全不同的两套 persona**：

```
┌─────────────────┐                   ┌─────────────────┐
│  CLI / IDE      │                   │  ampcode.com    │
│  (local user)   │                   │  (web UI)       │
└────────┬────────┘                   └────────┬────────┘
         │                                      │
         ▼                                      ▼
┌─────────────────┐                   ┌─────────────────┐
│ Executor Amp    │                   │ Orchestrator    │
│ "You are Amp,   │                   │ Amp (Agg Man)   │
│  a coding agent"│                   │ "users organize │
│                 │                   │  work into      │
│ ── 真干活 ──    │─── ${Yg}/${$iT}──▶│  projects...    │
│                 │                   │  you primarily  │
│ tools: Bash,    │                   │  do workflow    │
│ Read, Grep,     │                   │  management"    │
│ edit_file,      │                   │                 │
│ Task, Oracle... │                   │ tools: 创建/    │
│                 │                   │ 发送/搜索/      │
│                 │                   │ Slack/GitHub    │
└─────────────────┘                   └─────────────────┘
```

详见 [`prompts/executor-modes.md`](./prompts/executor-modes.md) 和 [`prompts/orchestrator-aggman.md`](./prompts/orchestrator-aggman.md)。

---

## 核心子系统地图

```
┌──────────────────────────────────────────────────────────────────┐
│                        Amp Runtime                                │
│                                                                    │
│  ┌──────────────────┐  ┌──────────────────┐  ┌────────────────┐  │
│  │ System Prompt    │  │  Tool Registry   │  │  Skills Store  │  │
│  │  Assembly (YwR)  │  │                  │  │                │  │
│  │  - base prompt   │  │  sources:        │  │  discovery:    │  │
│  │  - AGENTS.md     │  │  - builtin       │  │  - builtin://  │  │
│  │  - environment   │  │  - mcp-*         │  │  - .agents/    │  │
│  │  - signed-in user│  │  - toolbox       │  │  - ~/.config/  │  │
│  │  - skills index  │  │  - plugin        │  │                │  │
│  │  SHA-256 分片缓存│  │  executionProfile│  │  懒加载 only   │  │
│  └──────────────────┘  └──────────────────┘  └────────────────┘  │
│                                                                    │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  Agent Loop                                                 │  │
│  │  (LLM ⇄ tool_use / tool_result 循环)                       │  │
│  │                                                              │  │
│  │  ┌────────────┐  ┌─────────────┐  ┌──────────────────────┐ │  │
│  │  │ File       │  │ Permission  │  │ Subagents            │ │  │
│  │  │ Change     │  │ Manager     │  │  - Task (junior eng) │ │  │
│  │  │ Tracker    │  │  (ask/allow │  │  - Oracle (senior)   │ │  │
│  │  │            │  │   /deny/    │  │  - finder (concept)  │ │  │
│  │  │ per-tool   │  │   mode)     │  │  - Librarian (cross) │ │  │
│  │  │ edit log   │  │             │  │  - file analyzer     │ │  │
│  │  └────────────┘  └─────────────┘  │    (Gemini)          │ │  │
│  │                                     └──────────────────────┘ │  │
│  └────────────────────────────────────────────────────────────┘  │
│                                                                    │
│  ┌──────────────────┐  ┌──────────────────┐  ┌────────────────┐  │
│  │  Plugin System   │  │  Thread Service  │  │  Transport     │  │
│  │                  │  │  (remote)        │  │                │  │
│  │  .amp/plugins/   │  │  - getThread     │  │  local / DTW   │  │
│  │  - agent.start   │  │  - flushVersion  │  │  WebSocket to  │  │
│  │  - tool.call     │  │  - handoff       │  │  Cloudflare    │  │
│  │  - agent.end     │  │  - archive       │  │  Workers       │  │
│  │  (JSON-RPC IPC)  │  │                  │  │                │  │
│  └──────────────────┘  └──────────────────┘  └────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

---

## 数据流：一次完整的 turn

```
用户输入
  │
  ▼
┌────────────────────────────────────────────────────────────┐
│ 1. YwR() 组装 system prompt                                  │
│    - base prompt (按 mode 选 fwR/kwR/$wR/EwR/MwR/DwR/wwR)   │
│    - AGENTS.md blocks (deep 模式 32 KiB 硬上限)              │
│    - Environment block                                       │
│    - Signed-In User / Workspace Projects                     │
│    - Skills 索引（懒加载 manifest）                           │
│    - SHA-256 分片 → FmT 对比上一轮 → log changes             │
└────────────────────────────────────────────────────────────┘
  │
  ▼
┌────────────────────────────────────────────────────────────┐
│ 2. 工具集准备                                                │
│    - builtin + mcp + toolbox + plugin 全部合并               │
│    - 当前 agent mode / skill 触发的过滤                      │
│    - 每个 tool: { spec, fn, executionProfile }              │
└────────────────────────────────────────────────────────────┘
  │
  ▼
┌────────────────────────────────────────────────────────────┐
│ 3. LLM inference (Anthropic / OpenAI / 按 model 路由)       │
│    - 带 prompt caching (usage.cache_read_input_tokens 报告) │
│    - stream=true，增量解析 content blocks                    │
└────────────────────────────────────────────────────────────┘
  │
  ▼
┌────────────────────────────────────────────────────────────┐
│ 4. Tool 调度器                                               │
│    - 解析 tool_use blocks                                   │
│    - 按 executionProfile.resourceKeys 决定并发 vs 串行      │
│    - 同 path write 串行, Read/Grep 并行, Bash 全局 serial   │
│    - HITL: PermissionMode 拦截不允许的                      │
│    - preprocessArgs 钩子清理输入 (Bash 去 trailing &)       │
└────────────────────────────────────────────────────────────┘
  │
  ▼
┌────────────────────────────────────────────────────────────┐
│ 5. Tool 执行                                                 │
│    - fn(args, ctx) 返回 Observable<ToolRun>                 │
│    - 流式 status: in-progress → done|error|cancelled        │
│    - File edits 同步到 fileChangeTracker                    │
│    - Plugin 的 tool.call / tool.result hook 触发            │
└────────────────────────────────────────────────────────────┘
  │
  ▼
┌────────────────────────────────────────────────────────────┐
│ 6. 回注 tool_result，回到 step 3                            │
│    - 输出 > 阈值 → 截断 + "Tool result truncated..." 提示   │
│    - 错误 → is_error: true 但继续让模型看到                  │
└────────────────────────────────────────────────────────────┘
  │
  ▼ (LLM 决定 stop_reason="end_turn")
┌────────────────────────────────────────────────────────────┐
│ 7. Plugin agent.end hook                                    │
│    - 所有注册 agent.end 的 plugin 被调用                     │
│    - 任一 plugin 返回 {action: "continue"} → 回到 step 3     │
│    - 全部 done → 结束 turn                                   │
└────────────────────────────────────────────────────────────┘
  │
  ▼
┌────────────────────────────────────────────────────────────┐
│ 8. Thread 持久化                                             │
│    - threadService.flushVersion(id, v) 推到 server          │
│    - usage 字段写入最后一条 assistant message               │
└────────────────────────────────────────────────────────────┘
```

---

## 关键架构决策

1. **Prompt caching 为中心** —— 所有架构设计都服务于"让 cache 命中率高"：SHA 分片、静态 system prompt、consistent 工具顺序。
2. **子 agent 必用便宜模型总结** —— Task subagent 完成后用 Gemini 3 Flash 压缩。
3. **Skills 懒加载** —— prompt 只挂名字描述，不挂内容。
4. **Handoff > compaction** —— 当 context 撑不下时**开新线程**，不原地压缩。
5. **资源锁调度器** —— 工具默认并发，写冲突才串行。
6. **双人设** —— Executor 和 Orchestrator 用完全不同的 system prompt 和工具集。
7. **Plugin 子进程化** —— 每个 plugin 是独立 Bun 子进程，JSON-RPC 通信。
8. **远程执行 first-class** —— DTW 不是本地模式的镜像，是独立的 runtime。

---

## 相关文档

- 反编译方法：[`00-methodology.md`](./00-methodology.md)
- 所有系统提示词：[`prompts/`](./prompts/)
- 工具系统细节：[`tools/`](./tools/)
- 上下文管理机制：[`context/`](./context/)
- 对 Alva 的启发：[`alva-learnings/`](./alva-learnings/)
