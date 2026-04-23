# Amp Rate Limit / Error Classification + Retry 策略

> Amp 有三层独立的 retry（参数各不相同），外加 11 个命名错误分类函数 + 5 种 UI action。所有 inference 错误最终都过 `KpT(err) → JU(err)` 这条管道。

## 错误归一化 `KpT(error)`

从 strings line 63104 提取。用途：把任意 JS Error 变成 `{message, status?, error?: {type, message}}` 三字段归一结构。

```js
function KpT(T) {
  const R = String(T);                        // 整个 error 的字符串表示
  const t = SB(T);                            // 尝试提取 status code (429/5xx)
  const jsonMatch = R.match(/\{[\s\S]*"type"\s*:\s*"error"[\s\S]*\}/);
  if (jsonMatch) {
    try {
      const parsed = JSON.parse(jsonMatch[0]);
      const statusFromJson = SB(parsed);
      if (typeof parsed === "object" && "error" in parsed && typeof parsed.error === "object") {
        return {
          message: R,
          status: statusFromJson ?? t,
          error: {
            type:    typeof parsed.error.type    === "string" ? parsed.error.type    : undefined,
            message: typeof parsed.error.message === "string" ? parsed.error.message : undefined,
          },
        };
      }
      return { message: R, status: statusFromJson ?? t };
    } catch {}
  }
  return { message: R, status: t };
}

// SB 尝试从 number / string / {status} / {code} / {error} / {message} 中提取 status
function SB(T) {
  if (typeof T === "number") return T;
  if (typeof T === "string") {
    const m = T.match(/\b(429|[45]\d\d)\b/);
    return m?.[1] ? Number(m[1]) : undefined;
  }
  if (typeof T !== "object" || T === null) return;
  if ("status" in T && typeof T.status === "number") return T.status;
  if ("code"   in T && typeof T.code   === "number") return T.code;
  if ("error"  in T) { const r = SB(T.error); if (r !== undefined) return r; }
  if ("message" in T && typeof T.message === "string") return SB(T.message);
}
```

关键点：Amp 不假设 error 是哪个 SDK 的形状，**从任何 JS error 里尽量抠出 status + type**。这让下游分类函数简单。

## 11 个错误分类谓词

全部定义在 strings line 62451 附近。输入都是归一后的 `{message, status?, error?}`。每个谓词都 case-insensitive 扫 `message` + `error.message` 两处：

| 函数 | 层 | 匹配关键词 / 条件 |
|---|---|---|
| `LU` | 传输 | `"stream stalled"`, `"no data received for"` |
| `UUT` | 传输 | `fetch failed / failed to fetch / ENOTFOUND / ECONNREFUSED / ECONNRESET / ETIMEDOUT / network request failed / network error / dns lookup failed / getaddrinfo / socket hang up / connection refused / unable to connect / terminated / other side closed` |
| `KLR` | 传输 | `"response incomplete"`, `"stream ended unexpectedly"`, `"stream closed before"` |
| `G3T` | HTTP | `"unauthorized"` 或 `"401"` |
| `FLR` | HTTP | `status >= 500`（纯 status 判定，不看 message） |
| `MU` | HTTP | `"overloaded"` / `"overload"` 或 `error.type === "overloaded_error"` |
| `F3T` | Billing | `message.includes(NUT)` 其中 `NUT = "Out of credits"` |
| `WLR` | Billing | `"You've reached your free usage limit"` |
| `qLR` | Billing | `"You've exceeded your usage quota of"` 或 `"You've exceeded your usage limit of"` |
| `B$` | 输入 | `prompt is too long / exceed context limit / context limit reached / token limit exceeded / context window / maximum context length` |
| `zLR` | 输入 | `"image exceeds"` AND `"MB maximum"`（AND！）或 `error.type === "invalid_request_error"` 且含前两者 |
| `VLR` | 策略 | `message` 以 `"mode has been disabled by your workspace admin."` 结尾 |
| `GLR` | 策略 | `message` 以 `"InvalidModelOutputError"` 起头 |

组合判定 `HUT(error)`（"是否可自动重试"）：
```js
function HUT(T) {
  return MU(T) || LU(T) || UUT(T) || FLR(T)
      || T.error?.type === "rate_limit_error" || T.status === 429
      || GLR(T) || KLR(T);
}
```
注意 `HUT` **不包括** Billing / 输入 / 策略层 —— 这些错误不自动重试。

## `JU(error, options)` — 分派到 UI

strings line 63086。按顺序 if/else，第一个匹配的谓词决定 `{title, description, actions}`：

| 谓词 | Title | Description 来源 | Actions |
|---|---|---|---|
| `LU` | Model Stream Timed Out | 固定文案 ("Try again in a few seconds.") | `[retry]` |
| `UUT` | Network Error | 固定文案 ("Check your internet connection.") | `[retry]` |
| `G3T` | Unauthorized | 固定 ("Check your access token.") | `[retry]` |
| `F3T \|\| WLR` + **`freeTierEnabled`** | Out of Credits | "Add credits to keep using Amp right now, or wait until the next hour starts for more free usage. Signed in as `<email>`." | `[add-credits, retry]` |
| `F3T \|\| WLR` + ~freeTierEnabled | Out of Credits | "Add credits to keep using Amp. Signed in as `<email>`." | `[add-credits, retry]` |
| `MU` | Model Provider Overloaded | 固定文案 | `[retry]` |
| `zLR` | Image Too Large | 固定文案 | `[]`（空！不重试） |
| `error.type === "rate_limit_error"` | Rate Limit Hit | server 原文（`error.message`） | `[retry]` |
| `B$` | Context Limit Reached | 固定文案（引导 handoff / new thread） | `[handoff, new-thread]` |
| `qLR` | Usage Quota Exceeded | server 原文（`HLR(...)` 模板） | `[retry]` |
| `VLR` | Agent Mode Disabled | server 原文 | `[dismiss]` |
| **fallback** | Error | `US(error)` 截断到 200 字 | `[retry]` |

注意：
- `F3T` / `WLR` 分支因 `freeTierEnabled` 展开两种描述 —— free tier 用户多一句"等下个小时窗口"
- `zLR`（图像过大）**不给 retry** —— 用户必须改输入
- `VLR`（workspace admin 禁用）只给 `dismiss` —— 无法自助解决

### 5 种 action

```js
function uB(action, options) {
  switch (action) {
    case "retry":
      if (options?.retryCountdown != null)
        return `Retry (auto-retry in ${options.retryCountdown}s)`;
      return "Retry";
    case "add-credits":
      let label = "Add Credits";
      if (options?.ampURL) {
        const stripProtocol = new URL("/pay", options.ampURL).toString().replace(/^https?:\/\//, "");
        label += ` (${stripProtocol})`;
      }
      return label;
    case "new-thread": return "New Thread";
    case "handoff":    return "Handoff";
    case "dismiss":    return "Dismiss";
    default: throw Error(`Unhandled error action: ${String(action)}`);
  }
}
```

## 三层 Retry 策略

Amp 不是一个统一的 retry 函数，而是**三个独立循环**，用途完全不同：

### 层 1：UI 自动重试（主 inference）

strings line 63013。`ThreadWorker` 类的静态常量：

```js
static BASE_RETRY_SECONDS = 5;
static MAX_RETRY_SECONDS  = 60;
static MAX_AUTO_RETRIES   = 5;

getRetryDelaySeconds() {
  if (this.ephemeralErrorRetryAttempt >= pl.MAX_AUTO_RETRIES) return;
  const T = pl.BASE_RETRY_SECONDS * 2 ** this.ephemeralErrorRetryAttempt;
  return Math.min(T, pl.MAX_RETRY_SECONDS);
}

startRetryCountdown(seconds) {
  this.clearRetryCountdown();
  const session  = this.retrySession;
  const deadline = Date.now() + seconds * 1000;
  this.retryCountdownSeconds.next(seconds);
  this.retryTimer = setInterval(() => {
    if (session !== this.retrySession) return;      // session guard
    const remaining = Math.max(0, Math.ceil((deadline - Date.now()) / 1000));
    if (remaining <= 0) {
      this.clearRetryCountdown();
      this.retry().catch(err => TT.error("Auto-retry failed", { error: err }));
    } else {
      this.retryCountdownSeconds.next(remaining);
    }
  }, 1000);
}
```

**退避序列**：`5, 10, 20, 40, 60` 秒（5 次后放弃）。
**触发条件**：`HUT(err)` 为 true，即：`MU || LU || UUT || FLR || rate_limit_error || 429 || GLR || KLR`。
**特别**：用户可以随时手动点 Retry 按钮打断倒计时。

### 层 2：Handoff retry（context 溢出）

strings line 62446。用于 `handoff` tool 调模型时的 retry：

```js
const J5 = 3;                   // max retries
const yLR = 1000;               // base 1s
const mLR = 10000;              // max 10s
const pLR = 2;                  // multiplier
const uLR = 0.2;                // ±20% jitter

function bLR(attempt) {
  const R = yLR * pLR ** attempt;           // 1s, 2s, 4s, 8s, 10s(capped)
  const t = Math.min(R, mLR);
  const jitter = t * uLR * (Math.random() * 2 - 1);
  return Math.round(t + jitter);
}

// 在 handoff 循环里：
for (let attempt = 0; attempt < J5; attempt++) {
  if (deadline && attempt > 0) {
    const remainingMs = deadline - Date.now();
    if (remainingMs < 1e4) {                 // < 10s 剩余就放弃
      TT.warn("Handoff retry skipped, insufficient time remaining", { ... });
      break;
    }
  }
  try {
    return await iLR({ ... });
  } catch (err) {
    if (isAbort(err)) throw err;
    const last = attempt === J5 - 1;
    if (!ALR(err) || last) throw err;        // ALR = is-retryable-message
    const delay = bLR(attempt);
    const actualDelay = deadline
      ? Math.min(delay, deadline - Date.now() - 5000)
      : delay;
    if (actualDelay <= 0) throw err;
    TT.warn("Handoff model call failed, retrying with backoff", {
      attempt: attempt + 1, maxRetries: J5, delayMs: actualDelay, error: String(err),
    });
    await NS(actualDelay, signal);
  }
}
```

**`ALR(err)` 是简化的"消息匹配"判定**（跟 `HUT` 不同）：
```js
function ALR(T) {
  if (!(T instanceof Error)) return false;
  const R = T.message.toLowerCase();
  return R.includes("429") || R.includes("resource_exhausted")
      || R.includes("rate limit") || R.includes("too many requests")
      || R.includes("overloaded");
}
```

**退避序列**（期望值，加 jitter）：约 `1s, 2s, 4s`（3 次）。

### 层 3：Subagent retry（内部 inference）

strings line 63104。`runInferenceWithRateLimitRetries(...)`：

```js
const GpT = 3;                  // max retries
const jzR = 4000;               // base 4s
const EzR = 60000;              // max 60s

async function runInferenceWithRateLimitRetries(T, R, t, subagentKey) {
  if (!t) return T(...R);                    // t = enable flag
  const [, , , , , , signal] = R;
  let attempt = 0;
  while (true) {
    try { return await T(...R); }
    catch (err) {
      signal.throwIfAborted();
      const normalized = KpT(err);
      if (!(PzR(normalized) && attempt < GpT)) throw err;
      const delay = Math.min(jzR * 2 ** attempt, EzR);   // 4s, 8s, 16s... 上限 60s
      TT.warn("Subagent inference rate-limited, retrying", {
        subagentKey, attempt: attempt + 1, maxRetries: GpT,
        delayMs: delay, status: normalized.status, errorType: normalized.error?.type,
      });
      await NS(delay, signal);
      attempt++;
    }
  }
}

// PzR: subagent 专用 retry 判定（比 HUT 更窄）
function PzR(T) {
  const R = T.message.toLowerCase();
  const t = T.error?.message?.toLowerCase() ?? "";
  return T.status === 429
      || T.error?.type === "rate_limit_error"
      || R.includes("429")         || t.includes("429")
      || R.includes("resource_exhausted") || t.includes("resource_exhausted")
      || R.includes("resource exhausted") || t.includes("resource exhausted")
      || R.includes("rate limit")  || t.includes("rate limit")
      || R.includes("too many requests") || t.includes("too many requests");
}
```

**退避序列**：`4, 8, 16`（3 次后放弃）。

## 三层 retry 的对照表

| 层级 | 函数 | 最大重试次数 | Base | Max | 触发判定 |
|---|---|---|---|---|---|
| UI 自动 | `startRetryCountdown` + `retry` | 5 | 5s | 60s | `HUT` (最宽，含 5xx/stream/network) |
| Handoff | `bLR` + 循环 | 3 | 1s | 10s | `ALR` (字符串匹配) |
| Subagent | `runInferenceWithRateLimitRetries` | 3 | 4s | 60s | `PzR` (最窄，只 429/rate limit) |

设计哲学：**越靠近用户的层越宽容**（UI retry 把 5xx 也算），**越深的层越谨慎**（subagent 只重试明确的 rate limit）。

## 关键常量 & 字符串

```js
NUT = "Out of credits";         // 主要触发 F3T() 分类
HLR = (tokens, days, resetIn) => `You've exceeded your usage quota of ${tokens} per ${daysLabel(days)}. Your quota will reset in ${resetIn}. Contact your administrator to adjust your usage quota.`;

DLR = 3600000;                  // 1 hour
NLR = 30000;                    // 30 秒（某网络超时）
ULR = 10000;                    // 10 秒
r4R = 120000;                   // 2 分钟（大超时，用于 getUserFreeTierStatus）
e4R = 30000;                    // 30 秒（小超时）
```

## ephemeralError 流

`ThreadWorker` 有一个 `ephemeralError: BehaviorSubject<Error | undefined>`。任何上面 `HUT` 分类成立的错误都 `ephemeralError.next(err)`，UI 订阅这个流渲染错误 widget。用户点 Retry 或自动倒计时到期时 `ephemeralError.next(void 0)` 清零。

关键字段（用于调试渲染）：
```js
`- Message: ${ephemeralError.message}`
`- Error type: ${ephemeralError.error?.type ?? "n/a"}`
`- Retry countdown: ${ephemeralError.retryCountdownSeconds ?? "n/a"}s`
```

## 对 Alva 的启发

### 值得直接抄的 4 点

1. **错误归一化 `KpT`**：Alva 的 LLM adapter 各家响应格式不一（anthropic / openai / gemini），应该有一个 `normalize_error(err) -> NormalizedError { message, status, error_type? }` 层，让上层分类函数不用关心来源 SDK。

2. **11 个命名谓词**：不要写 `if err.contains("rate limit") || err.contains("429")` 散落各处，抽出 `fn is_rate_limit(e: &NormalizedError) -> bool`。Rust 更友好 —— 直接 `enum ErrorCategory { Network, RateLimit, Overloaded, OutOfCredits, ContextLimit, … }` + `From<AnthropicError>` impl。

3. **三层 retry 分工**：
   - Alva 的 LLM middleware 层：抄 subagent 级，4s×2^n 上限 60s，只重试 429
   - Alva 的 agent loop：抄 UI 级，5s×2^n 上限 60s，宽容（包括 5xx / stream / network）
   - context 溢出走单独 path（handoff 或 Alva 的 summarize），不要跟 rate limit 混

4. **UI action 拆分**：返回 `{ title, description, actions: Vec<Action> }` 让 frontend 渲染按钮。五种 action：`Retry / AddCredits / NewThread / Handoff / Dismiss` —— Alva 可以按需增减（比如加 `Reauth`）。

### Alva 当前的问题

扫 `AnalyticsExtension`，没看到任何错误分类 / retry 组件。错误处理散落在：
- `alva-llm-*`（各模型 adapter）：retry 逻辑可能各写各的
- `agent-core`：可能只有 `Result<T, E>` 冒泡，没有 category
- GUI 层：可能看到 error 只能渲染 `err.to_string()`

建议新建 `crates/alva-error/`：统一 `AlvaError` enum + category + retry policy。然后 AnalyticsExtension 订阅 error events 做 log + telemetry。

---

## 交叉引用

- `NUT="Out of credits"` 触发的 billing 展示见 `./billing-usage.md`
- retry 里的 `TT.warn(...)` log 格式见 `./logging.md`

## 原始产物位置

- strings line 62446：`bLR` / `ALR` (handoff retry)
- strings line 62447-62448：`_LR` handoff retry 循环体
- strings line 62451：11 个错误分类谓词（`LU/UUT/G3T/F3T/WLR/qLR/MU/zLR/B$/VLR/GLR/KLR/FLR/HUT`）
- strings line 63013：`ThreadWorker` UI 自动 retry (`BASE_RETRY_SECONDS / MAX_RETRY_SECONDS / MAX_AUTO_RETRIES / startRetryCountdown`)
- strings line 63086：`JU` + `uB` action 定义
- strings line 63104：`runInferenceWithRateLimitRetries` + `PzR` + `KpT / SB`
- strings line 64184：`NUT / HLR / J5 / yLR / mLR / pLR / uLR / DLR / NLR / ULR / jzR / EzR / GpT` 常量
