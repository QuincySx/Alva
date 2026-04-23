---
name: amp-alva-learnings
description: 基于 Amp 反编译结果，对 Alva (本项目) 的具体建议 —— Alva 现状对比 + 6 个推荐实施方案的 Rust 骨架（tool adapter / resource lock scheduler / handoff tool / workflow skill / plugins exec / context diagnostics）。决定"Amp 的哪些设计值得抄到 Alva" 时加载。
trigger_words:
  - alva
  - alva-agent
  - 实施
  - 借鉴
  - 启发
  - 对 Alva 的建议
  - tool adapter
  - ToolAdapter trait
  - schema 修复
  - tool_use id 归一化
  - resource lock scheduler
  - handoff tool 实现
  - workflow skill 实现
  - plugin exec 命令
  - context diagnostics CLI
  - Amp 对比 Alva
---

# Amp → Alva Learnings

Amp 设计中值得 Alva 借鉴的部分，每个都有 Rust 实施骨架。

## 文件索引

| 文件 | 内容 | 优先级 |
|---|---|---|
| `./comparison.md` | Alva 现状 vs Amp 全景对比表 | 先读 |
| `./tool-adapter.md` | `ToolAdapter` trait —— 把 Tool 翻译给各家 LLM（Anthropic/OpenAI Chat/Responses/Gemini）+ schema 修复 + tool_use id 归一化（~2.5 天） | ⭐ Tier 1 |
| `./plugin-exec.md` | `alva plugins exec` 调试命令（1-2 天工作量） | ⭐ Tier 1 |
| `./context-diagnostics.md` | `alva context` CLI 诊断命令（1-2 天） | ⭐ Tier 1 |
| `./workflow-skill.md` | `WorkflowSkill` 类型（canonical prompt 固化） | ⭐ Tier 1 |
| `./handoff-tool.md` | `handoff` 工具（跨 thread recap） | Tier 2 |
| `./resource-lock-scheduler.md` | 工具并发资源锁调度器 | Tier 2 |

## Tier 1（强烈推荐，低成本高收益）

### 1. `alva plugins exec <path> <event>` 调试命令

几百行代码就能实现，让 plugin 开发从"启完整 agent 调试"升级到"本地 stub host 秒级迭代"。

**关键做法**：stub host 只打印 ui.notify / system.open 等 RPC，不真执行。

### 2. `alva context` 诊断 CLI

```
Sections:
  System prompt         1,234  (0.6%)
  AGENTS.md             2,345  (1.2%)
  Tools                12,456  (6.2%)
  Thread history      145,678 (72.8%)
Used:  162,255 tokens (81.1%)

Cache stats:
  Cache read: 144,755  ✓ 95.2% hit rate
```

用户 "token 费用为什么这么贵" 问题秒回答。

### 3. `WorkflowSkill` 类型

把 `/commit` / `/merge` / `/deploy` 做成 canonical workflow。LLM 只决定触发，具体 prompt 固化在磁盘。

**关键做法**：`trigger_words` + `anti_trigger_words` 双写进 system prompt。

### 4. `ToolAdapter` trait

当前 `alva-llm-provider` 有 3 个 free function (`to_anthropic_tools` / `to_oai_tools` / `to_responses_tools`) 散在各 provider 文件里，没抽象、不去重、没 schema 修复、没 tool_use id 归一化，而且不支持 Vertex Gemini。

**关键做法**：统一 trait + `YLR` 等价的 schema 修复 + `toolu_` 前缀归一化 + Gemini 递归 schema 改写 + 区分 regular / structured output 两种模式。**直接影响"接第 4 个 provider 时的成本"**。

## Tier 2（中等成本，显著收益）

### 4. `handoff` 工具（替代自动 compact）

```rust
pub struct HandoffTool { thread_service, handoff_context_tool }
// 1. 调 handoff_context_tool 生成 first-person recap + top 10 files
// 2. 开新 thread，parent_thread_id 指向当前
// 3. 把 goal + recap + files 作为新 thread 的首条 user message
```

比自动压缩更自然，用户可追溯（老 thread 还在）。

### 5. 资源锁调度器

```rust
trait Tool {
    fn resource_keys(&self, args) -> Vec<ResourceKey> { vec![] }
    fn execution_mode(&self) -> ExecutionMode { ExecutionMode::Parallel }
}
```

让 "parallel by default" 从 prompt 建议升级为调度器硬保证。

## Alva 现状（速查 comparison.md）

```
Architecture:        ████████░░  80%
Tool system:         ███████░░░  70%   (缺 resource lock + preprocessor)
Context mgmt:        ██████░░░░  60%   (缺 file tracker + handoff + diagnostics)
Skills:              ██████░░░░  60%   (等待确认具体实现)
Plugins:             ██████░░░░  60%   (AEP 已有，缺 exec 调试)
Orchestration:       █████░░░░░  50%   (Blackboard 起步，缺 orchestrator prompt)
Remote runtime:      ███░░░░░░░  30%   (local-first，不追求)
Storage:             ███████░░░  70%
Diagnostics:         ██░░░░░░░░  20%   ← 补齐收益最大
```

## 不建议抄的

- **Thread 全部存 server** —— Alva 是 local-first
- **深度依赖 prompt caching 特性** —— Alva 要支持多 provider
- **Aggman 实现细节** —— 耦合 Amp server 架构，抄概念就行
- **符号 minifier 命名** —— Amp 二进制里 `${Y8}` 只是 bundler 产物，Alva 直接用 `"Bash"` 这种可读名字
