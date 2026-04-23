---
name: amp-internals
description: Amp 反编译里的两个"非面向用户"子系统 —— Nostromo（scenario-driven fake LLM，用来跑内部 agent 测试）和 Sandbox（DTW headless executor 的 `executorType:"sandbox"` 以及 `.agents/preview` 预览 URL 规则）。研究 Amp 怎么测 agent 行为 / 怎么跑隔离环境时加载。
trigger_words:
  - nostromo
  - amp-nostromo
  - scenario dsl
  - fake llm
  - mock llm
  - amp sandbox
  - executorType sandbox
  - .agents/preview
  - sandbox preview
  - amp eval
  - amp internal
---

# Amp Internals

这里收两个从 strings 里挖出来的**没文档、也没面向用户文案**的子系统。它们在 Amp binary 里明确存在，但在 ampcode.com 公开文档里基本不提，很可能是内部测试 / DTW 基础设施的一部分。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./nostromo.md` | Nostromo = 伪装成 LLM 的 scenario 执行器（`amp-nostromo-*` model prefix + DSL parser） | 想给 Alva 加类似的 agent 行为回放 / 集成测试（对应 `alva-app-eval`） |
| `./sandbox-runtime.md` | `executorType:"sandbox"` + DTW headless harness + `.agents/preview` 文件约定 | 想懂 Amp 怎么在云端跑 agent、怎么让 LLM 正确地 hand-off preview URL |

## 常见问答

**Q：Nostromo 是什么？**
A：**强证据**：是 Amp 内部的 **fake LLM provider**。当 model 名匹配 `amp-nostromo-*` 时，Amp 不会调真 OpenAI，而是 new 一个本地 `wWT` 类，把上一条 user message 里的 `nostromo:` 段或 ```nostromo 代码块解析成 **scenario DSL**（支持 `tool`、`reply`、`delay`、`error`、`repeat`、`if ... contains("x")` 六种语句），按脚本逐 chunk 流出。用于**回放 agent 行为**。命名来自《异形》飞船 Nostromo（Sourcegraph 文化梗）。

**Q：Sandbox executor 是什么？**
A：**强证据**：`executorType` 有两种取值 —— `"local-client"` 和 `"sandbox"`。**推测**：`"sandbox"` 对应 DTW headless harness（`kVT` 类），即 execution thread 跑在 Cloudflare Workers 侧的 case。走到 sandbox 分支时，system prompt 里会被注入**三段额外内容**：(1) `.amp/in/artifacts` 目录使用规则 (cqT)；(2) `git fetch --unshallow` 提示（shallow checkout 警告，h6R）；(3) `.agents/preview` 文件查找逻辑 + "never give raw localhost URL" 指引。

**Q：这两个东西对 Alva 有什么用？**
A：
- **Nostromo → `alva-app-eval`**：给 eval 框架一个"伪 LLM"模式，用 DSL 写确定性 agent 行为，不花 token 就能跑 scenario 测试。
- **Sandbox → 远程 runtime**：如果未来 Alva 做远程 executor，`.agents/preview` 这种"把部署环境的 URL 映射规则放进 repo"的模式值得抄。

**Q：这两个在 strings.txt 里强证据 or 推测？**
A：
- Nostromo：**强证据**，有完整的 DSL parser（`LWT`）、mock streaming 类（`wWT`）、model 路由逻辑（`PBR` 里 `amp-nostromo-*` → `wWT`）。
- Sandbox：**部分强证据**（`executorType:"sandbox"` 字符串 + `bootstrapExecutor` 调用 + 三段 sandbox-only system prompt 注入），**部分推测**（跑在 Cloudflare Workers、Firecracker 无直接证据，DTW 上下文才有 `zy()`、`RIVET_PUBLIC_ENDPOINT` 链路）。

## 交叉引用

- Nostromo 具体怎么流式输出：见 `../remote-runtime/stream-json.md` 对比（stream-json 是真 CLI 输出、Nostromo 是 fake LLM 输入）。
- Sandbox 和 DTW 关系：见 `../remote-runtime/dtw.md`（两者是同一套 headless harness 的不同角度）。
- 如果想做 Alva 版 scenario 测试，参考本地 `alva-app-eval` crate + `/Users/smallraw/.claude/projects/.../project_alva_eval.md` 记忆。

## 原始证据

- 所有证据在 `/tmp/amp-decompile/strings.txt` 的 62457-62498, 63005-63009, 63350-63354, 65278 行附近。
- 关键符号：`wWT`（fake LLM 类）、`LWT`（scenario parser 入口）、`BmT="nostromo:"`、`a6R=".agents/preview"`、`cqT`/`h6R`/`zwR`（sandbox-only prompt 片段）。
