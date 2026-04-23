# 支持的 Model 清单

反编译中所有 model 的中央注册表在 `Z9` 对象，provider 枚举是 `i9`。变量在 `Yt=dT(()=>{i9={...},Z9={...}})` 一次性定义。

> **来源**：反编译 `strings.txt` 第 64843 行（单行 77K 字符），关键 key 匹配 `provider: i9.XXX, name: "...", displayName: "...", contextWindow: N, maxOutputTokens: M, pricing: {...}, capabilities: {...}`

## Provider 枚举（`i9`）

```js
i9 = {
  ANTHROPIC:  "anthropic",
  BASENTEN:   "baseten",      // 原文拼写，带 N
  OPENAI:     "openai",
  XAI:        "xai",
  CEREBRAS:   "cerebras",
  FIREWORKS:  "fireworks",
  GROQ:       "groq",
  MOONSHOT:   "moonshotai",
  OPENROUTER: "openrouter",
  VERTEXAI:   "vertexai",
}
```

共 **10 个 provider**。groq 虽然在枚举里但 Z9 里没有对应 model（可能走 `gpt-oss-120b` 或 Kimi 时用作 fallback baseURL）。

## Anthropic 模型（9 个 —— 主力）

| Name | displayName | Context | Max Out | Price In/Out | 特性 |
|---|---|---:|---:|---|---|
| `claude-sonnet-4-20250514` | Claude Sonnet 4 | **1M** | 32K | $3/$15 | reasoning+vision+tools |
| `claude-sonnet-4-5-20250929` | Claude Sonnet 4.5 | **1M** | 32K | $3/$15 | 同上 |
| `claude-sonnet-4-6` | Claude Sonnet 4.6 | **1M** | **64K** | $3/$15 | 更长 max_out |
| `claude-opus-4-20250514` | Claude Opus 4 | 200K | 32K | $15/$75 | - |
| `claude-opus-4-1-20250805` | Claude Opus 4.1 | 200K | 32K | $15/$75 | - |
| `claude-opus-4-5-20251101` | Claude Opus 4.5 | 200K | 32K | **$5/$25** | 降价一半 |
| `claude-opus-4-6` | Claude Opus 4.6 | **332K** | 32K | $5/$25 | 非标 context |
| `claude-opus-4-7` | **Claude Opus 4.7** | 332K | 32K | $5/$25 | 最新旗舰 |
| `claude-haiku-4-5-20251001` | Claude Haiku 4.5 | 200K | **64K** | $1/$5 | RUSH 模式用 |

**特殊 1M flag**：`claude-opus-4-6-1m` 不在 Z9 里，但代码里有 `qBT="claude-opus-4-6-1m", zBT=1000000`（`l$=qBT`）—— 通过给模型名加 `-1m` 后缀 + `anthropic-beta: fast-mode-2026-02-01` 触发。上下文 1M，对应 `LARGE` agent mode。

**pricing 字段**：`input/output/cached/cacheWrite` per-million tokens，`cacheTTL:300` 秒（Anthropic ephemeral cache）。

## OpenAI 模型（14 个）

| Name | displayName | Context | Max Out | Price In/Out/Cached |
|---|---|---:|---:|---|
| `gpt-5` | GPT-5 | 400K | 128K | $1.25/$10/$0.125 |
| `gpt-5.1` | GPT-5.1 | 400K | 128K | $1.25/$10/$0.125 |
| `gpt-5.2` | GPT-5.2 | 400K | 128K | $1.75/$14/$0.175 |
| `gpt-5.4` | GPT-5.4 | 400K | 128K | $2.5/$15/$0.25 |
| `gpt-5.4-pro` | GPT-5.4-Pro | **1.05M** | 128K | $30/$180/$30 |
| `gpt-5-codex` | GPT-5 Codex | 400K | 128K | $1.25/$10/$0.125 |
| `gpt-5.1-codex` | GPT-5.1 Codex | 400K | 128K | $1.25/$10/$0.125 |
| `gpt-5.2-codex` | GPT-5.2 Codex | 400K | 128K | $1.75/$14/$0.175 |
| `gpt-5.3-codex` | GPT-5.3 Codex | 400K | 128K | $1.75/$14/$0.175 |
| `gpt-5-mini` | GPT-5 Mini | 400K | 128K | $0.25/$2/$0.025 |
| `gpt-5-nano` | GPT-5 Nano | 400K | 128K | $0.05/$0.4/$0.005 |
| `o3` | o3 | 200K | 100K | $2/$8/$0.5 |
| `o3-mini` | o3-mini | 200K | 100K | $1.1/$4.4/$0.55 |
| `openai/gpt-oss-120b` | GPT OSS 120B | 128K | 32K | free? (no pricing) |

**内部模型**：
| Name | 说明 |
|---|---|
| `amp-nostromo-v1` | `AMP_NOSTROMO`，确定性测试 stub（nostromo agent mode），免费，capabilities 只有 `tools` |

**Codex 模型无 vision**（早期版本）；Codex 5.2/5.3 回加 vision。Codex 系列专用于 `gpt-5-codex` prompt 路由（见 `amp-prompts` skill 的 `EwR` prompt）。

## xAI 模型（1 个）

| Name | Context | Max Out |
|---|---:|---:|
| `grok-code-fast-1` | 256K | 32K |

无 pricing（估计 Amp proxy 对用户 free-tier？）。capabilities: `reasoning+tools`，无 vision。

## Vertex AI / Gemini 模型（4 个）

| Name | displayName | Context | Max Out |
|---|---|---:|---:|
| `gemini-3-pro-preview` | Gemini 3 Pro Preview | **1.05M** | 65K |
| `gemini-3.1-pro-preview` | Gemini 3.1 Pro Preview | 1.05M | 65K |
| `gemini-3-flash-preview` | Gemini 3 Flash Preview | 1.05M | 65K |
| `gemini-3-pro-image-preview` | Gemini 3 Pro Image | 1.05M | 65K |

**Gemini 3 Flash 的特殊用法**：代码中硬编码 `DXR="gemini-3-flash-preview"` 作为 File Analyzer 子 agent 的模型（见 `amp-prompts/subagents.md`）。**主对话默认不用 Flash** —— 它是廉价摘要子 agent 专用。

**Gemini 3 Pro Image** 的 `capabilities={vision, imageGeneration}`，无 tools/reasoning —— `painter` 工具（image generation）专用，见 `amp-tools/catalog.md` 的 K6R 部分。

## Cerebras 模型（1 个）

| Name | Context | Max Out |
|---|---:|---:|
| `zai-glm-4.7` | 131K | 40K |

capabilities: `tools` 仅。Cerebras 走单独 SDK（`CEREBRAS_BASE_URL`、`CEREBRAS_API_KEY`），但 Amp 仍然通过 `/api/provider/cerebras` 代理。

## Fireworks 模型（6 个）

| Name | displayName | Context | Max Out | capabilities |
|---|---|---:|---:|---|
| `accounts/fireworks/models/qwen3-coder-480b-a35b-instruct` | Qwen3 Coder 480B | 230K | 32K | tools |
| `accounts/fireworks/models/qwen3-235b-a22b-instruct-2507` | Qwen3 235B | 230K | 32K | tools |
| `accounts/fireworks/models/kimi-k2-instruct-0905` | Kimi K2 Instruct | 230K | 32K | tools |
| `accounts/fireworks/models/glm-4p6` | GLM 4P6 | 162K | 40K | tools+reasoning |
| `accounts/fireworks/models/glm-5` | GLM 5 | 202K | 40K | tools，pricing $1/$3.2 |
| `accounts/fireworks/models/minimax-m2p5` | MiniMax M2.5 | 200K | 32K | tools，pricing $0.3/$1.2 |

Fireworks 支持 `x-fireworks-direct-routing: true` header（settings.internal.fireworks.directRouting）。

## Baseten 模型（1 个）

| Name | displayName | Context | Max Out |
|---|---|---:|---:|
| `moonshotai/Kimi-K2.5` | Kimi K2.5 | 262K | 32K |

pricing: `$0.6/$3/$0.1`，capabilities: `tools+vision`。

## Moonshot 原生（1 个）

| Name | displayName | Context | Max Out |
|---|---|---:|---:|
| `kimi-k2-instruct-0905` | Kimi K2 Instruct | **1M** | 32K |

**注意**：Moonshot 原生版 context 是 1M，而 Fireworks 转卖版是 230K。

## OpenRouter 模型（5 个）

| Name | displayName | Context | Max Out |
|---|---|---:|---:|
| `sonoma-sky-alpha` | Sonoma Sky Alpha | 256K | 32K |
| `z-ai/glm-4.6` | OpenRouter GLM 4.6 | 131K | 40K |
| `moonshotai/kimi-k2-0905` | Kimi K2 0905 (OpenRouter) | 262K | 32K |
| `qwen/qwen3-coder` | Qwen3 Coder 480B (OpenRouter) | 262K | 32K |
| `qwen/qwen3-235b-a22b-2507` | Qwen3 235B A22B (OpenRouter) | 262K | 32K |

**OpenRouter 特殊**：这是唯一**不经 Amp proxy** 的 provider —— 直连 `https://openrouter.ai/api/v1`，需要用户自己配 `openrouter.apiKey` 或 `OPENROUTER_API_KEY` 环境变量。

## 模型名解析

CLI flag `--model <provider>:<model>`：

```
amp --model anthropic:claude-sonnet-4-20250514
amp --model openai:gpt-5-codex
amp --model "smart=anthropic:claude-opus-4-7,rush=anthropic:claude-haiku-4-5-20251001"
```

反编译函数 `dkT(T)`（line 64045）做 split-by-`:`，`zF0(T)` 支持 mode-specific overrides（`smart=provider:model,rush=...`）。存到 `internal.model` settings。

## 运行时 model 解析

`vn(T)` 查 Z9，返回规格对象。`e$(T)` 返回 `{provider, model}`。`va(T)` 从 `provider/model` 返回 `model` 部分。`Kc(T)` 返回 `contextWindow - maxOutputTokens`（可用 input budget）。

## 能力矩阵总结

| Capability | 数量 | 典型代表 |
|---|---|---|
| `reasoning` | 33 | 所有 Claude, GPT-5 系列, Gemini 3 系列 |
| `vision` | 22 | Claude, GPT-5 非-codex, Gemini 3, Baseten Kimi |
| `tools` | 41 | 全部 |
| `imageGeneration` | 1 | `gemini-3-pro-image-preview` 独占 |

## 对 Alva 的启发

1. **注册表集中化**：Alva 目前在 `AnthropicProvider::models()` 等里分散写死。参考 Amp 的 Z9 —— 把 `context_window / max_output / pricing / capabilities` 集中在 `alva-llm-provider::models` 模块，`ModelSpec` struct，Provider trait 里只做运行时调用。

2. **`-1m` 后缀约定**：把 beta flag / 变种放在 model name 后缀（`claude-opus-4-6-1m`）而不是 config，客户端代码解析时显式 opt-in。Alva 可以学这个模式处理 `thinking-enabled` / `vision-enabled` 变种。

3. **Mode-specific model override**：CLI `--model smart=x,rush=y` 语法很精巧，用一个 flag 配置所有 agent mode 的模型。Alva 的 CLI (`alva-app-cli`) 可以抄。

4. **File Analyzer 用 Flash**：子 agent 压缩 / 文件分析**不该用主对话模型**，Amp 硬编码 Gemini 3 Flash Preview。Alva 做 memory compression 时应该单独配一个 `summary_model`。

5. **Moonshot 原生 vs Fireworks 转卖**：同一模型在不同 provider 上 context window 不一样（1M vs 230K）。Alva 应该在 `ModelId` 里包 provider，而不是靠模型名去推。
