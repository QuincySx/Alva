---
name: amp-knowledge
description: Amp (Sourcegraph CLI) 反编译分析的完整知识库。需要了解 Amp 怎么做 prompt / tools / context / skills / plugins / orchestration / storage / remote runtime 时加载此 skill。子 skill 按模块拆分，只加载需要的部分。
trigger_words:
  - amp
  - ampcode
  - sourcegraph agent
  - amp 架构
  - bun-binary
  - amp 反编译
---

# Amp Knowledge Base

这是 Amp 静态分析的**顶级索引**。本 skill 本身不含深度内容 —— 按模块有子 skill，按需 load。

## 子 Skill 清单

| Skill | 何时 load | 覆盖 |
|---|---|---|
| `amp-prompts` | 想看 Amp 的 system prompt 原文、prompt 装配逻辑、变量解码 | 7 executor modes + Aggman + 5 subagents + 装配 pipeline |
| `amp-tools` | 想看工具定义结构、调度、清单 | tool spec + executionProfile + 40+ builtin 清单 |
| `amp-context` | 想看上下文管理、压缩、handoff | 四层策略 + file tracker + /compact + handoff + 诊断 |
| `amp-storage` | 想看 thread 怎么存、message 数据结构 | Thread 模型 + version vector + server sync |
| `amp-skills-system` | 想看 Amp 自己的 skill 系统怎么做的 | 懒加载 + frontmatter + 渲染模式 + builtin skills |
| `amp-plugins` | 想看 `.amp/plugins/` 插件系统 | hooks + RPC + debugging |
| `amp-remote-runtime` | 想看远程执行 / stream-JSON | DTW (Cloudflare Workers) + NDJSON subprocess |
| `amp-orchestration` | 想看多 agent 编排 | Aggman 双 persona + execution threads + canonical workflow |
| `amp-observability` | 想看日志 / OTEL / rate limit retry / billing | Winston `TT` + `@opentelemetry/api` + 三层 retry + Out of Credits + free tier |
| `amp-alva-learnings` | 想看对 Alva 的具体建议 | 对比表 + 5 个实施方案骨架 |
| `amp-checks` | 想看 checks 框架（可插拔 diff-scoped review）| check skill 格式 + code_review 集成 + builtin 清单 |
| `amp-mcp` | 想看 MCP client 完整 lifecycle（发现、连接、OAuth、trust、过滤、resource、错误） | 3 处配置源 + 8 态状态机 + shared OAuth callback + `includeTools` glob + `read_mcp_resource` + `vCT` 错误分类 |
| `amp-security` | 想看 permission/sandbox/secret 系统 | rules DSL（allow/reject/ask/delegate）+ `amp permissions` CLI + 无 sandbox + `[REDACTED:xxx]` 管道 |
| `amp-models` | 想看多 model / provider 适配层 | 41 model × 10 provider + Amp `/api/provider/{name}` proxy + rate limit 指数退避 + 4 套 tool format（Anthropic/OpenAI Responses/Chat/Vertex）|
| `amp-ide-integration` | 想看 IDE 检测 / editor launch / 工作区状态读取 / WS 插件协议 | 6 IDE detection + `Dv` 注册表 + Zed/VSCode SQLite query 模式 + JSON-RPC notification schema |
| `amp-cli` | 想看 Amp CLI 完整命令树 / flag / exit code 约定（做 alva-app-cli 时参考）| 15 顶级 + 36 子命令 + 全局 flag + exit code 0/1/2/130 + 对 alva 的设计启发 |

子 skill 路径：`./prompts/SKILL.md`、`./tools/SKILL.md` ... 以此类推。

## 顶层文件（不在子 skill 里的）

| 文件 | 内容 | 何时看 |
|---|---|---|
| `./README.md` | 全目录树导览 + 快速入口表 | 首次了解 |
| `./00-methodology.md` | 反编译方法论（`strings(1)` + awk + grep）| 想复制这套方法到其他 Bun-compiled binary |
| `./01-architecture.md` | Amp 整体架构全景（四种形态 / 双 persona / 8 子系统）| 需要整体视角 |

## 常见快速问答（不用加载任何子 skill 就能答）

**Q：Amp 是什么？**
A：Sourcegraph 的闭源 CLI coding agent。CLI 模式下是 executor，在 ampcode.com web UI 下是 orchestrator ("Agg Man")，指挥一组跑在 Cloudflare Workers (DTW) 上的 execution threads。

**Q：Amp 怎么把上下文维持干净？**
A：4 层叠加 —— 工具输入端截断（Read 500 行）+ Skills 懒加载（名字在 prompt、内容按需 load）+ 子 agent 用 Gemini Flash 压缩 + handoff 开新 thread（不是 in-place compact）。详见 `amp-context` skill。

**Q：Amp 的工具怎么并发的？**
A：每个 tool 声明 `resourceKeys(args)` 资源锁，调度器用多读一写语义。Bash 是 `serial:true` 全局独占。详见 `amp-tools` skill。

**Q：Amp 怎么处理"上下文快满了"？**
A：**不自动压缩**。LLM 自己调 `handoff` 工具开新 thread，带 first-person recap + top 10 files。详见 `amp-context` skill 的 `handoff.md`。

**Q：做了多少工作量？**
A：从反编译到文档成型 ~62500-66000 行 strings 被解析；45 个 markdown 文件 10K 行结构化内容。

## 原始产物

- 反编译 strings 在 `/tmp/amp-decompile/strings.txt`（工作区文件，不 commit）
- 本 docs/amp/ 目录是它的结构化提炼
