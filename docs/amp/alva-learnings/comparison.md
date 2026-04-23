# Alva vs Amp 对照表

> 系统性地对照：Alva 已经有什么、缺什么、哪些值得抄。

---

## 核心架构

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| Agent loop | JavaScript + Bun runtime | Rust + tokio | ✅ Alva 性能优势 |
| Tool trait | `{spec, fn, executionProfile}` | `alva-kernel-abi::Tool` | ✅ 同向 |
| Extension system | Plugin + internal extensions | `Extension` trait + 11 个 wrapper | ✅ Alva 更系统化 |
| Middleware | 洋葱式 stack | `MiddlewareStack` (before/after/wrap) | ✅ 等价 |
| Context management | Context blocks + SHA 分片 | `ContextStore` 四层容器 + 8 钩子 | ✅ Alva 更灵活 |
| Session / Thread | Server-only | `AgentSession` + `InMemorySession` | 🟡 不同哲学 |
| Skills | builtin + `.agents/skills/` + `~/.config/` | `alva-protocol-skill` 三级加载 | ✅ 同向 |

---

## 工具相关

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| Tool spec structure | `{name, description, inputSchema, source, meta, executionProfile}` | 接近（待确认 `executionProfile` 字段）| 🟡 Alva 可能缺 resource lock |
| Tool fn 返回 | Observable 流式 | `RuntimeExecutionContext` → `AgentEvent` | ✅ 同向 |
| Tool 预处理 | `preprocessArgs(T, R)` 钩子 | 没见到 | ❌ 缺 |
| Resource lock 并发 | `resourceKeys(args)` + `serial` | 没见到 | ❌ 缺 |
| 工具来源分层 | builtin/mcp-*/toolbox/plugin | `source` 类似概念 | ✅ 同向 |
| 自定义 subagent | `.agents/agents/*.md` frontmatter | `SubAgentExtension` | ✅ 同向 |
| Custom agent CWD 注入 | Scripts 目录 + Bash cwd 注入 | 待确认 | 🟡 确认支持 |

---

## 上下文管理

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| Read 硬截断 | 500 行 × 4096 字节 | 待确认默认 | 🟡 |
| Tool result 截断 | 按 KB 限制 + notice | 待确认 | 🟡 |
| AGENTS.md 预算 | 32 KiB hard limit | 待确认 | 🟡 |
| File change tracking | `fileChangeTracker` 独立抽象 | 没见到独立 crate | ❌ 缺 |
| `/compact` slash command | 有（手动）| `CompactionExtension` + middleware | ✅ 同向 |
| 自动 compact | 没有（设计选择）| `auto_compact.rs` 有 | 🟡 考虑是否必要 |
| `handoff` 工具（跨线程）| 有，LLM 自决定 | 没见到 | ❌ 缺 |
| Context diagnostics CLI | `amp context` | 没见到 | ❌ 缺 |
| Prompt caching 监控 | SHA 分片 + usage 对比 | 没见到 | ❌ 缺 |

---

## Skills 系统

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| SKILL.md frontmatter | `{name, description, disable-model-invocation}` | 待确认 | 🟡 |
| 发现路径 | builtin / `.agents/` / `~/.config/` | 待确认支持全局路径 | 🟡 |
| 两种渲染模式 | normal XML + deep Markdown | 待确认 | 🟡 |
| `load_skill` 工具 | 有 | 待确认 | 🟡 |
| `includeTools` 过滤 MCP | 有（90%+ 省 token）| 待确认 | 🟡 关键 |
| Skills 激活额外工具 | `builtinTools: []` | 待确认 | 🟡 |
| Scripts 目录 + cwd | 有 | 待确认 | 🟡 |

---

## Plugin / Extension 系统

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| Plugin 语言 | TypeScript（Bun runtime）| Python（AEP SDK）+ JS（规划）| ✅ 同向 |
| 传输协议 | JSON-RPC 2.0 stdout/stdin | AEP (JSON-RPC 2.0) | ✅ 同向 |
| Lifecycle hooks | agent/tool/config 各 2+1 | `HooksExtension` | ✅ 有 |
| `agent.end → continue` | 能强制 continue outer loop | 待确认 | 🟡 确认能力 |
| Plugin `ai.ask` RPC | 有 | 没见到 | ❌ 缺 |
| Plugin `thread.append` | 有 | 待确认 (`BusWriter.provide`?)| 🟡 |
| OTEL span 自动包装 | 每 hook 都有 | 待确认 | 🟡 |
| `plugins list` 命令 | 有，详细输出 | 待确认 | 🟡 |
| `plugins exec` 调试命令 | 有 ⭐ | 没见到 | ❌ 缺 |

---

## Orchestration / 多 Agent

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| Orchestrator vs Executor | 两套 system prompt + 两套工具集 | `SubAgentExtension` 起步 | 🟡 部分有 |
| Callback message kind | 有（通过 `Qg` 工具）| 待确认 `MessageKind` enum | 🟡 检查 |
| "No polling" 硬约束 | prompt 里明确 | 待确认 | 🟡 |
| Canonical workflow prompt | `workflow: "merge_changes"` 固化 | 没见到 | ❌ 缺 |
| Trigger / anti-trigger 词表 | prompt 明确区分 | 没见到 | ❌ 缺 |
| Blackboard / SpawnScope | — | 有 | ✅ Alva 更完整 |

---

## 远程执行

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| Remote executor (DTW) | Cloudflare Workers + DO | 没有（local-first）| ✅ 设计选择 |
| Stream-JSON subprocess | `--execute --stream-json` NDJSON | 待确认 | 🟡 可加 |
| Execute mode workflow 白名单 | `--dangerously-allow-all` 逃生口 | 待确认 PermissionMode 支持 | 🟡 |
| Checkpoint / resume | DTW durable state | `CheckpointExtension` | ✅ 有 |
| Snapshot apply (`--apply`) | 有（云 → 本地）| 没见到 | 🟡 视需求 |

---

## Thread 存储

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| Storage 位置 | Server-only (ampcode.com) | 本地 | ✅ 不同哲学 |
| Version vector | `thread.v` + `flushVersion` | 待确认 | 🟡 |
| Message 带 usage 字段 | 每条 assistant message 独立 usage | 待确认 | 🟡 关键 |
| `discoveredGuidanceFiles` 快照 | 绑定在 user message | 没见到 | ❌ 建议加 |
| `parentToolUseId` 消息树 | 有 | 待确认 | 🟡 |
| Info role message | 有独立 `role: "info"` | 没见到 | ❌ 考虑 |

---

## 诊断工具

| 维度 | Amp | Alva | 评价 |
|---|---|---|---|
| `amp context` 命令 | ✅ | ❌ | 见 `context-diagnostics.md` |
| `amp plugins exec` | ✅ | ❌ | 见 `plugin-exec.md` |
| Prompt caching rate 监控 | ✅（usage 对比）| ❌ | 建议加 |
| SHA 分片变化 log | ✅ | ❌ | 建议加 |
| Token 分段归类（system/tools/history）| ✅ | ❌ | `amp context` 一部分 |

---

## 总体评分

Alva 已经达到的成熟度（按维度，粗估）：

```
Architecture:        ████████░░  80%   (Extension / Middleware / 分层清晰)
Tool system:         ███████░░░  70%   (缺 resource lock + preprocessor)
Context mgmt:        ██████░░░░  60%   (缺 file tracker + handoff + diagnostics)
Skills:              ██████░░░░  60%   (等待确认具体实现)
Plugins:             ██████░░░░  60%   (AEP 已有，缺 exec 调试)
Orchestration:       █████░░░░░  50%   (Blackboard 起步，缺 orchestrator prompt)
Remote runtime:      ███░░░░░░░  30%   (local-first，不追求)
Storage:             ███████░░░  70%   (InMemorySession 够用，缺细节)
Diagnostics:         ██░░░░░░░░  20%   (缺 context/plugins 命令)
```

---

## Next Steps 建议

按 Alva 当前阶段，最该做的是：

1. **补齐 diagnostics**（`alva context` + `alva plugins exec`）—— 成本低，价值高
2. **确认 skills/plugins 细节**（渲染模式 / hooks 返回值 / ai.ask）—— 可能很多已经有，只是没写 doc
3. **加 resource lock scheduler** —— 让 "parallel by default" 可落地
4. **`handoff` 工具** —— 用户体验提升显著
5. **`WorkflowSkill` 类型** —— 解决 high-stakes action 安全性

剩下的按实际 roadmap 决定。
