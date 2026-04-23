---
name: amp-tools
description: Amp 工具系统的完整参考 —— Tool spec 数据结构、Observable fn、executionProfile 资源锁调度、40+ builtin 工具清单、.agents/agents/*.md 自定义子 agent。需要查某个工具的 inputSchema 或理解并发调度时加载。
trigger_words:
  - tool
  - inputSchema
  - executionProfile
  - resource lock
  - parallel execution
  - Bash tool
  - Read tool
  - Grep tool
  - edit_file
  - Task tool
  - Oracle tool
  - finder
  - codebase_search_agent
  - todo_write
  - handoff tool
  - toolbox
  - custom agent
  - repl tool
  - REPL
  - nested LLM loop
  - stop tool
  - inner LLM
  - read_thread
---

# Amp Tools

40+ builtin 工具的 spec、架构、调度机制、自定义 agent。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./architecture.md` | Tool 数据结构 (spec/fn/executionProfile) + Observable 返回 + preprocessArgs 钩子 | 想懂工具定义格式 |
| `./execution-scheduler.md` | 资源锁调度器 + 多读一写语义 + Bash serial:true + HITL 拦截 | 想懂并发怎么做 |
| `./catalog.md` | 全部 40+ builtin 工具按分类列出，含 inputSchema 和关键限制 | 想查某个工具 |
| `./custom-agents.md` | `.agents/agents/*.md` frontmatter + toolPatterns + scripts 目录 | 想做自定义子 agent |
| `./repl-deep-dive.md` | `repl` 工具的嵌套 LLM 循环完整设计 + 子 LLM system prompt + 10 道早停 + stop 工具 | 想做"工具内嵌 LLM 循环"模式 |

## 按工具类型快速定位

- **文件/shell**：Bash / Read / Grep / edit_file / create_file / undo_edit / glob → `catalog.md` 的 "Core" 节
- **子 agent**：Task / Oracle / codebase_search_agent (finder) / Librarian → `catalog.md` 的 "Subagents" 节
- **Skill/Walkthrough/Review**：load_skill / walkthrough / code_review / code_tour → `catalog.md` 的 "Skill 相关" 节
- **任务管理**：todo_write / handoff / read_thread / create_handoff_context → `catalog.md` 的 "Task 管理" 节
- **Web**：read_web_page / web_search → `catalog.md` 的 "Web" 节
- **外部代码**：read_github / Bitbucket Enterprise 全家桶 → `catalog.md` 的 "外部代码" 节
- **可视化**：chart / mermaid / image_generation / walkthrough_diagram → `catalog.md` 的 "Visualization" 节
- **分析**：analyze_file (Gemini Flash) / repl → 同上（REPL 嵌套 LLM 循环深挖看 `./repl-deep-dive.md`）

## 核心数据结构（不用 load 子文件就能用）

```js
ToolDefinition = {
  spec: {
    name, description, inputSchema,     // 基本
    source: "builtin"|"mcp-*"|"plugin"|{toolbox:path},
    meta: { disableTimeout?: boolean },
    executionProfile: {
      resourceKeys: (args) => [{key, mode: "read"|"write"}],
      serial?: boolean    // 全局独占 (Bash)
    }
  },
  fn: (args, ctx) => Observable<{status, progress?, result?, error?}>,
  preprocessArgs?: (args) => args       // 可选兜底
}
```

## 调度规则（速查）

- `serial: true` → 全局独占（等所有 running 完成）
- 多个 tool call 申请同 key 的 write 锁 → 串行
- 多个 tool call 申请同 key 的 read 锁 → 并发
- write 锁阻塞后续 read / write 申请
- 无 resourceKeys → 全并发
- 申请多个 key 时按字典序排序避免死锁（推断）
