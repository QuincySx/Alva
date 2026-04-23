# 对 Alva 的启发 —— 汇总目录

> 把 Amp 的每项值得借鉴的设计，对照 Alva 现状给出**具体建议**。

## 文件清单

| 文件 | 内容 |
|---|---|
| [`comparison.md`](./comparison.md) | 现状 vs Amp 全景对照表 |
| [`tool-adapter.md`](./tool-adapter.md) | `ToolAdapter` trait（把 Tool 翻译给各家 LLM API）|
| [`resource-lock-scheduler.md`](./resource-lock-scheduler.md) | 工具并发调度器（资源锁）|
| [`handoff-tool.md`](./handoff-tool.md) | Handoff 工具（跨线程 recap）|
| [`workflow-skill.md`](./workflow-skill.md) | WorkflowSkill 类型（canonical prompt）|
| [`plugin-exec.md`](./plugin-exec.md) | `alva plugins exec` 调试命令 |
| [`context-diagnostics.md`](./context-diagnostics.md) | `alva context` CLI 诊断命令 |

---

## 优先级推荐（按性价比）

### Tier 1：高性价比，建议优先

1. **[`tool-adapter.md`](./tool-adapter.md)** —— 3 个散落的 `to_*_tools` free function 抽成统一 trait，补 schema 修复 + tool_use id 归一化 + Vertex Gemini。现有代码有 3 套对应实现，改造成本 ~2.5 天；**接第 4 个 provider 时受益最大**。

2. **[`plugin-exec.md`](./plugin-exec.md)** —— 几十行代码，让 plugin 调试从 "aliased println debugging" 升级到 "本地可重现"。Alva `SubprocessLoaderExtension` 直接受益。

3. **[`context-diagnostics.md`](./context-diagnostics.md)** —— 一个 `alva context` 子命令，让用户看清"context 被什么占了"。生产期 debug 的救命工具。

4. **[`workflow-skill.md`](./workflow-skill.md)** —— 把 `/commit` `/merge` `/deploy` 做成 canonical workflow。直接提升安全性和可预测性。

### Tier 2：中等成本，显著收益

5. **[`handoff-tool.md`](./handoff-tool.md)** —— 让 LLM 自主开新 thread 替代自动 compaction。需要改 `BaseAgent` + `ThreadService` 接口。

6. **[`resource-lock-scheduler.md`](./resource-lock-scheduler.md)** —— 工具并发成为**调度器的硬保证**，不是 prompt 的"温柔建议"。需要改 `Tool` trait。

### Tier 3：架构级，长期收益

7. **Orchestrator 人设** —— `BaseAgent::builder().agent_kind(Orchestrator)` 切换整套工具集 + prompt。适合你们做多 agent 协作 feature 时再上。

8. **DTW 类远程 runtime** —— 等有云端需求时再做。`EngineRuntime` trait 预留好扩展点即可。

---

## 不建议抄的

Amp 的以下做法**不应该照搬**：

### 1. Thread 全部存 server

Amp 是云优先，Alva 是 local-first。强制走 ampcode.com 对 Alva 不合适。可以做**可选**云端同步（CheckpointExtension 加 remote backend），但默认本地。

### 2. 深度依赖 Prompt Caching

Amp 的很多设计（SHA 分片、static tool ordering）为了 Anthropic prompt caching 优化。这在 Alva 也有用，但不应该**主导**架构设计。你们支持多 provider，不能假设所有 provider 都有 caching。

### 3. Aggman 双 persona 的实际实现

概念值得学（orchestrator vs executor 不同 prompt），但 Amp 的具体实现耦合了他们的 server-side 架构。Alva 可以用 `SubAgentExtension` + 不同 system prompt 做等价的事，不用重写。

### 4. 特定的工具 minifier 变量名

Amp 二进制里 `${Y8}` 等占位符只是 bundler 产物，不是设计选择。Alva 直接用 `"Bash"` 这种可读名字就好。

---

## 如何使用这些文档

每个 `.md` 文件的结构：

1. **背景**：这个 feature 在 Amp 里是什么
2. **对 Alva 的价值**：为什么值得抄
3. **建议的 Rust 实现骨架**：直接能抄到 `alva-*` 某个 crate
4. **集成点**：改哪些现有 crate，加哪些新 crate
5. **优先级和风险**：什么时候做，有什么坑

建议阅读顺序：
1. [`comparison.md`](./comparison.md) —— 知道已有什么、缺什么
2. Tier 1 三个文档
3. Tier 2 两个文档
4. 剩下按需
