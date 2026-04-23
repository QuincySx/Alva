---
name: amp-models
description: Amp 的多 model / provider 适配层。完整 model 清单（41+ 模型横跨 10 个 provider）、provider 路由（全部走 Amp 的 /api/provider/{name} proxy）、rate limit 指数退避、stream 错误分类（8 种 error kind）、Anthropic / OpenAI Responses / Vertex AI / OpenAI Chat Completions 四种 tool format 转换。需要看 Amp 怎么支持那么多 model、rate limit 怎么退、tool format 怎么转、context window 怎么算时 load。
trigger_words:
  - model
  - provider
  - multi-model
  - anthropic
  - openai
  - gemini
  - kimi
  - grok
  - claude-opus-4-7
  - gpt-5
  - rate limit
  - 429
  - retry
  - backoff
  - context window
  - token count
  - tool format
  - Anthropic tool spec
  - OpenAI function tool
  - Responses API
  - thinking mode
  - reasoning effort
  - provider routing
  - Amp proxy
  - Z9
  - i9
---

# Amp Models & Providers

Amp 的多 model 适配层。覆盖完整 model 目录、provider 路由、rate limit 重试、错误分类、tool format 转换。

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./supported-models.md` | 41+ model 完整清单 + context window / max output / pricing / capabilities | 想看 Amp 支持哪些 model、pricing、thinking/vision 能力 |
| `./provider-routing.md` | 10 provider 枚举 + `/api/provider/{name}` proxy + 路由逻辑 `PBR` | 想看 provider 怎么选、baseURL、特殊 header |
| `./inference-retry.md` | rate-limit 指数退避 (4s→60s, 3 次) + 8 种 stream error kind + context-window 分类 | 想看重试 / 错误分类 / timeout / overloaded 怎么处理 |
| `./adapter-layer.md` | Anthropic / OpenAI Responses / Vertex AI / Chat-Completions 四种 tool format 转换 | 想看 tool spec 从 Amp 内部模型怎么翻译到各家 API |

## 常见问答（不用加载子文件）

### Q：Amp 支持哪些 model？
**A：41+ 个，横跨 10 个 provider**。典型模型：
- **Anthropic**（主力）：Claude Sonnet 4 / 4.5 / 4.6、Claude Opus 4 / 4.1 / 4.5 / 4.6 / 4.7、Claude Haiku 4.5（1M 上下文是 Opus 4.6/4.7 专属）
- **OpenAI**：GPT-5 / 5.1 / 5.2 / 5.4 / 5.4-Pro、GPT-5 Codex / 5.1/5.2/5.3 Codex、GPT-5 Mini / Nano、o3 / o3-mini
- **xAI**：grok-code-fast-1
- **Vertex AI**：Gemini 3 Pro Preview / 3.1 Pro Preview / 3 Flash Preview / 3 Pro Image
- **Cerebras**：Z.ai GLM 4.7
- **Fireworks**：Qwen3 Coder 480B / 235B、Kimi K2 Instruct、GLM 4P6 / 5、MiniMax M2.5
- **Baseten**：Kimi K2.5
- **Moonshot**（原生）：Kimi K2 Instruct（1M context！）
- **OpenRouter**：Sonoma Sky Alpha、GLM 4.6、Kimi K2 0905、Qwen3 Coder / 235B
- **内部**：`amp-nostromo-v1` 确定性测试 stub

详见 `supported-models.md`。

### Q：rate limit 怎么处理？
**A：指数退避 4s → 8s → 16s → 60s (cap)，最多 3 次**，仅对 subagent 启用（`retryOnRateLimit:true`）。429 / `rate_limit_error` / `resource_exhausted` / "too many requests" 关键字命中即重试。主线程不自动重试 —— 直接把错误显示给用户。详见 `inference-retry.md`。

### Q：provider 怎么选？
**A：模型名里带 `provider/model` 前缀（`anthropic:claude-sonnet-4-5`），CLI `--model` flag 解析**。所有请求都走 Amp 服务器的 `/api/provider/{name}` proxy（Anthropic 是 `/api/provider/anthropic`，OpenAI 是 `/api/provider/openai/v1`，依此类推）—— 用户 API key 不需要在本地配。唯一例外：OpenRouter 走 `https://openrouter.ai/api/v1`（需要用户自己的 `openrouter.apiKey`）。详见 `provider-routing.md`。

### Q：Gemini 3 Flash 是干嘛的？
**A：专门给 file analyzer / librarian 子 agent 用的廉价摘要模型**。Amp 主对话不会用 Flash —— 它是被 Oracle / File Analyzer / Librarian 固定引用（反编译中用 `Y3("GEMINI3_FLASH_PREVIEW")` 字面量）。Alva 如果做 agg 也可以这样：主对话用 Opus，子 agent 压缩用 Flash。

### Q：`claude-opus-4-6-1m` / Fast mode / task budget 是啥？
**A：Anthropic beta feature flags**：
- `-1m` 后缀（`qBT="claude-opus-4-6-1m"`, `zBT=1000000`）= 开启 1M 上下文，靠 `anthropic-beta: fast-mode-2026-02-01` header
- `anthropic.taskBudget` 设置开启 `task-budgets-2026-03-13` header，传 `output_config.task_budget.total=<tokens>`
- `anthropic.interleavedThinking.enabled` 开启 `interleaved-thinking-2025-05-14` header
- `thinking.type=adaptive`（Opus 4.7 默认）替代旧的 `thinking.type=enabled`

### Q：每个 model 的 thinking / reasoning effort 怎么设？
**A：provider-specific**：
- **Anthropic**：`thinking.type=enabled` + `budget_tokens`（Opus 4.7 默认 `adaptive` + `output_config.effort`）。`budget_tokens` 由用户 prompt 触发词映射：`think super hard→31999`, `think hard→10000`, `think→4000`
- **OpenAI Responses**：`reasoning.effort=low/medium/high` + `reasoning.summary=auto`
- **Gemini**：`thinkingConfig.thinkingLevel=LOW/MEDIUM/HIGH`
- **Moonshot/Fireworks Kimi**：`internal.kimi.reasoning=none/low/medium/high`（`none` 时 `temperature=0.6`，其他 `temperature=1`）

### Q：Amp 做了多少工作量？
**A：41 个 model × 10 个 provider × 4 套 tool format（Anthropic / OpenAI Responses / OpenAI Chat / Vertex AI）= 大约 3500 行 adapter 代码**，全部经 Amp 服务器 proxy。

## 关键核心洞察（不用 load 就能用）

1. **Provider 全部经 Amp proxy**：`new URL("/api/provider/{name}", serverURL)` —— 用户不需要在本地配 API key（OpenRouter 例外）。Alva 当前是直连 provider API，值得借鉴这个架构。
2. **Model 按 "大小/角色" 分 agent mode**：SMART→Opus 4.6、RUSH→Haiku 4.5、DEEP→GPT-5.4、INTERNAL→Opus 4.7、LARGE→Opus 4.6 1M、AGG→Opus 4.6、NOSTROMO→internal test stub。每个 mode 有不同 `primaryModel` + 不同工具集 + 不同 prompt（见 `amp-prompts` skill）。
3. **8 种 stream error 显式命名**：`context_limit / entitlement_limit / internal_error / midstream / midstream_overloaded / out_of_credits / overloaded / unauthorized`。这个分类值得抄。
4. **Tool format 4 变种**：Anthropic 的 `{name,description,input_schema,eager_input_streaming:true}`、OpenAI Responses 的 `{type:"function",parameters,strict:false}`、OpenAI Chat 的 `{type:"function",function:{parameters}}`、Vertex 的 `{functionDeclarations:[{parameters}]}`（且需要重写 JSON schema `type`字段）。
5. **Rate limit 仅对 subagent 启用**：主对话 0 重试，subagent `retryOnRateLimit:true` 时指数退避 3 次。设计哲学：主线程快速报错让用户决策，子 agent 可以重试。

## 对 Alva 的启发（摘要）

| 当前 Alva | Amp 做法 | 建议 |
|---|---|---|
| `AnthropicProvider` / `OpenAIChatProvider` / `OpenAIResponsesProvider` 直连 | 全部经 `/api/provider/{name}` proxy | 可选加一层 `AmpProxyProvider`，SaaS 分发时不暴露 key |
| `rate_limit.rs` 只追踪 5h/7d 窗口 + `x-ratelimit-remaining` | 显式分类 8 种 stream error + 指数退避 + max-3 retry | 把 `StreamError` enum 加进去，方便调用方区分 |
| 未见 tool format 适配层 | 四套 tool format 转换（`Bx`/`N3T`/`qUT`/`C3T`）| 在 `llm-provider` 加 `ToolAdapter` trait，按 provider 输出不同 JSON |
| 未见 model catalog | Z9 catalog（41 model × 10 provider）集中注册 | 加 `models.rs` 注册表，包含 `context_window / max_output / pricing / capabilities` |
| 未见 agent mode → model 映射 | `gS.SMART.primaryModel` 等在 `Mh/n3` 导出 | Alva agent-core 可加 `AgentMode` → `ModelId` 路由，与 `ExecutionProfile` 绑定 |

详细实施建议在 `provider-routing.md` 和 `adapter-layer.md` 末尾。
