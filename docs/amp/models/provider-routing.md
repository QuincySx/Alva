# Provider 路由与 Amp Proxy 架构

Amp 不直连上游 provider API（除 OpenRouter 外），全部走 Amp 服务器的 `/api/provider/{name}` 反向代理。好处：用户不需要在本地配 API key；Amp 统一做 rate limit / billing / caching / 路由。

## 一、Provider 切换总入口 `PBR(T, R)`

来源：`strings.txt` 第 63170 行附近。反编译后大约等价于：

```js
function PBR(provider, model) {
  if (provider === "openai" && model?.includes("gpt-oss-120b")) return new NX();        // → groq path
  if (provider === "openai" && model?.startsWith("amp-nostromo-")) return new wWT();    // → nostromo stub
  switch (provider) {
    case "vertexai":   return new pNT();   // Gemini (native Google SDK via Vertex AI)
    case "openai":     return new OUT();   // GPT-5 family via Responses API
    case "openrouter": return new HWT();   // direct OpenRouter, no Amp proxy
    case "anthropic":  return new UBT();   // Claude via messages.stream
    case "xai":        return new LUT();   // Grok via OpenAI-compatible chat
    case "cerebras":   return new jWT();   // Z.ai GLM via Cerebras SDK
    case "fireworks":  return new KUT();   // Fireworks chat completions
    case "baseten":    return new VUT();   // Baseten chat completions
    case "groq":       return new NX();    // Groq chat completions (used for gpt-oss + Kimi)
    case "moonshotai": return new EWT();   // Moonshot (Kimi) native
    default: throw Error("Unknown provider: " + provider);
  }
}
```

每个 class（`UBT / OUT / pNT / HWT / LUT / jWT / KUT / VUT / NX / EWT / wWT`）都是一个 `Provider` 对象，暴露 `async *stream(...)`。

## 二、BaseURL 映射（全部走 Amp server）

以下是从反编译确认的每个 provider 的 `baseURL`：

| Provider class | URL path on Amp server | 上游 SDK |
|---|---|---|
| `UBT` Anthropic | `new URL("/api/provider/anthropic", serverURL)` | `@anthropic-ai/sdk` |
| `OUT` OpenAI Responses | `new URL("/api/provider/openai/v1", serverURL)` | `openai` (responses API) |
| `pNT` Vertex AI Gemini | `new URL("/api/provider/vertexai", serverURL)` | `@google/genai` (GeminiNextGenAPIClient) |
| `LUT` xAI Grok | `new URL("/api/provider/xai/v1", serverURL)` | OpenAI-compatible chat |
| `KUT` Fireworks | `new URL("/api/provider/fireworks/v1", serverURL)` | OpenAI-compatible |
| `VUT` Baseten | `new URL("/api/provider/baseten/v1", serverURL)` | OpenAI-compatible |
| `jWT` Cerebras | `new URL("/api/provider/cerebras", serverURL)` | `@cerebras/cerebras_cloud_sdk` |
| `NX` Groq | `new URL("/api/provider/groq", serverURL)` | OpenAI-compatible |
| `EWT` Moonshot | `new URL("/api/provider/kimi", serverURL)` | OpenAI-compatible |
| `HWT` OpenRouter | **`https://openrouter.ai/api/v1`（直连）** | OpenAI-compatible |

**唯一例外**：OpenRouter 不经 Amp proxy。从 `OBR(T,R)`：

```js
async function OBR(settings, meta) {
  let apiKey = settings["openrouter.apiKey"] || process.env.OPENROUTER_API_KEY;
  if (!apiKey) throw Error("OpenRouter API key not found...");
  return new y9({
    apiKey, baseURL: "https://openrouter.ai/api/v1",
    defaultHeaders: { ...Vc(), ...meta?.threadID ? {[mPT]: meta.threadID} : {}, [_s]: "amp.chat" }
  });
}
```

**推测**：Amp 不想代理 OpenRouter，可能因为它本身就是一层 meta-router 了。

## 三、认证与 Headers

每个请求都会带一组通用 header：

```js
{
  ...Vc(),                     // User-Agent / Accept 等基础 header
  [_s]: "amp.chat",            // amp-feature: "amp.chat" → 告诉后端是哪个子系统
  [FA]: messageID,             // amp-message-id
  "x-session-affinity": threadID,   // Fireworks 专用，保证同 session 路由一致
  ...Is(thread),               // thread metadata
  ...(authToken ? {"X-Workers-Service-Auth": authToken} : {})   // 远程 DTW 场景
}
```

Anthropic 额外 header（function `I$`）：

```js
"anthropic-beta": [...betas].join(","),   // e.g. "task-budgets-2026-03-13,fast-mode-2026-02-01"
"x-amp-override-provider": settings["anthropic.provider"],   // optional: "anthropic" / "bedrock" / "vertex"
```

**beta flag 触发条件**：
- `anthropic.taskBudget` + 模型是 opus-4-7 → `task-budgets-2026-03-13`
- `anthropic.speed=="fast"` + `EK(model)` 为 true（1M 模型满足条件）→ `fast-mode-2026-02-01`
- `anthropic.interleavedThinking.enabled` → `interleaved-thinking-2025-05-14`

## 四、Provider 切换触发点 `iLR(...)`（handoff tool）

```js
async function iLR({thread, ..., modelOverride, ...}) {
  let {model: C, agentMode: o} = nn(A.settings, thread);
  let n = modelOverride ?? C;
  let [b] = n.split("/");   // provider name
  let _ = va(n);            // model name
  switch (b) {
    case "anthropic":  return cLR(...);
    case "xai":        return nLR(...);
    case "openai":     return oLR(...);
    case "vertexai":   return sLR(...);
    case "openrouter":
    case "groq":
    case "moonshotai":
    case "cerebras":
    default: throw Error(`Unsupported provider for handoff: ${b}`);
  }
}
```

**有意思**：handoff 工具只支持 4 个 provider（anthropic/xai/openai/vertexai）—— 其他 provider 的"专项 tool_choice=ANY"行为可能不可靠。

## 五、Agent Mode → Primary Model 映射

来源：`gS` 对象（line 65561 附近）：

```js
gS = {
  SMART:    { primaryModel: Y3("CLAUDE_OPUS_4_6") },
  RUSH:     { primaryModel: Y3("CLAUDE_HAIKU_4_5") },
  AGG:      { primaryModel: Y3("CLAUDE_OPUS_4_6") },   // server-only, web UI
  LARGE:    { primaryModel: Y3("CLAUDE_OPUS_4_6") },   // 加 -1m 后缀触发 1M context
  DEEP:     { primaryModel: Y3("GPT_5_4"), reasoningEffort: "high" },
  INTERNAL: { primaryModel: Y3("CLAUDE_OPUS_4_7"), reasoningEffort: "xhigh" },
  NOSTROMO: { primaryModel: Y3("AMP_NOSTROMO"), reasoningEffort: "low" },
}
```

`Y3("CLAUDE_OPUS_4_6")` 是 lazy accessor（避免循环 import），运行时解析为 `Z9.CLAUDE_OPUS_4_6`。

## 六、Prompt 路由 `T6R` （基于 provider + model）

来源：`strings.txt` 第 63013 行：

```js
function ZwR(spec) {     // spec = Z9[...]
  if (spec.name === "gpt-5-codex") return "gpt-5-codex";
  if (spec.name.includes("kimi-k2")) return "kimi";
  if (spec.provider === "openai")   return "gpt";
  if (spec.provider === "xai")      return "xai";
  if (spec.provider === "vertexai") return "gemini";
  return "default";
}

function JwR(modelName, provider) {  // fallback when no spec match
  if (modelName.includes("gpt-5-codex")) return "gpt-5-codex";
  if (modelName.includes("kimi-k2"))     return "kimi";
  if (modelName.includes("gpt"))         return "gpt";
  if (provider === "xai")      return "xai";
  if (provider === "vertexai") return "gemini";
  return "default";
}

function T6R({agentMode, model, provider}) {
  if (agentMode === G0T)       return "aggman";       // AGG mode → aggman prompt
  if (agentMode === "rush")    return "rush";         // RUSH mode → wwR prompt
  if (agentMode === "deep")    return "deep";         // DEEP mode → fwR prompt
  if (rAR(agentMode))          return "internal";     // INTERNAL mode → MwR prompt
  let spec = vn(`${provider}/${model}`);
  if (spec) return ZwR(spec);
  return JwR(model, provider);
}
```

→ 路由到 9 种 prompt（`aggman / rush / gpt / gpt-5-codex / deep / internal / xai / kimi / gemini / default`），prompt 本身见 `amp-prompts/executor-modes.md`。

## 七、Reasoning Effort 路由 `EBR`

来源：`strings.txt` 第 63170 行：

```js
function EBR(modelName, settings, context) {
  let [provider, baseName] = modelName.includes("/") ? modelName.split("/", 2) : ["", modelName];
  let slashCommandEffort = context ? Si(context)?.reasoningEffort : void 0;
  let hasDeepReasoning = settings["agent.deepReasoningEffort"] !== undefined;
  switch (provider) {
    case "anthropic":
      return settings["anthropic.effort"]
          ?? slashCommandEffort
          ?? (baseName === Z9.CLAUDE_OPUS_4_7.name ? "medium" : "high");
    case "openai":
      return (baseName?.includes("codex") && hasDeepReasoning ? z2(settings) : void 0)
          ?? slashCommandEffort
          ?? "medium";
    case "vertexai":
      return settings["gemini.thinkingLevel"] ?? slashCommandEffort ?? "medium";
    default:
      return slashCommandEffort ?? "medium";
  }
}
```

每个 provider 有独立的 effort 配置 key + 不同默认值。Opus 4.7 默认 medium，其他 Claude 默认 high（因为 4.7 adaptive thinking 效果更好）。

## 八、Model Name → Provider 解析

来源 `e$(T)` 附近：

```js
function e$(modelName) {
  // modelName 格式: "provider/model" 或 "model" (default anthropic)
  let slash = modelName.indexOf("/");
  if (slash === -1) return { provider: "anthropic", model: modelName };
  return {
    provider: modelName.slice(0, slash),
    model: modelName.slice(slash + 1)
  };
}
```

所以 `claude-sonnet-4-5-20250929` == `anthropic/claude-sonnet-4-5-20250929`。`openai/gpt-5` 明确指定 provider。

## 对 Alva 的启发

### 1. 加一层 Amp 风格的 proxy provider（可选）

**现状**：Alva 的 `AnthropicProvider` / `OpenAIChatProvider` 直连 `api.anthropic.com` / `api.openai.com`。好处是简单，坏处是如果 Alva 以后要做 SaaS 分发，每个用户都要配自己的 API key。

**建议**：加 `AmpStyleProxyProvider`，baseURL = `https://alva.example/api/provider/{name}`，共享一个用户 access token（而不是每家 provider 的 key）。具体实现：

```rust
// crates/alva-llm-provider/src/provider/proxy.rs
pub struct ProxyProvider {
    upstream_name: String,       // "anthropic" / "openai" / etc.
    alva_server: Url,
    auth_token: SecretString,    // 单个 user token，代替 provider key
    upstream: Box<dyn Provider>, // 内部还是复用 AnthropicProvider 等
}

impl Provider for ProxyProvider {
    fn base_url(&self) -> Url {
        self.alva_server.join(&format!("api/provider/{}", self.upstream_name)).unwrap()
    }
    fn auth_headers(&self) -> Vec<(String, String)> {
        vec![("Authorization".into(), format!("Bearer {}", self.auth_token.expose_secret()))]
    }
    // 其他方法 delegate 到 self.upstream
}
```

### 2. 把 Provider 选择从 AppState 拉出来成 Service

Amp 的 `PBR(T,R)` 把 provider 实例构造集中在一个函数里，从 `provider` string + `model` name 路由。Alva 当前可能各处 `Box<dyn Provider>` 传来传去，值得改成 `ProviderRegistry::get(provider_id) -> Arc<dyn Provider>`。

### 3. Agent Mode → (Model, Prompt, Tools) 三元组

Amp 的 `gS` 对象把 agent mode 打包成一个整体配置（primary model + include tools + prompt type + reasoning effort）。Alva 的 `agent-core` 有 `ExecutionProfile` 概念，可以把：
- `primary_model: ModelId`
- `prompt_variant: PromptVariant`（default / rush / deep / gpt-codex / kimi / gemini...）
- `reasoning_effort: Effort`
- `include_tools: Vec<ToolId>`

合并成 `AgentMode` struct，一处声明即可，避免分散在各处 `match` 里。

### 4. 不要全部支持

Amp 的 `iLR(...)` handoff tool 只支持 4 个 provider —— 其他 provider 明确抛错。Alva 学这个"**小功能早期不用全 provider 都支持**"的态度，避免 `provider = "openrouter"` 时工具调用不可靠。
