# Inference 重试 / Rate Limit / 错误分类

Amp 把 LLM inference 错误分成三个维度：**HTTP 层重试**（rate limit）、**流错误类型**（8 种 kind）、**context window 越界**。每个维度有独立处理逻辑。

## 一、Rate Limit 指数退避（subagent 专用）

**核心函数**：`runInferenceWithRateLimitRetries`，`strings.txt` 第 63104 行。

```js
async runInferenceWithRateLimitRetries(fn, args, retryOnRateLimit, subagentKey) {
  if (!retryOnRateLimit) return fn(...args);
  let [,,,,,,abortSignal] = args;
  let attempt = 0;
  while (true) {
    try { return await fn(...args); }
    catch (err) {
      abortSignal.throwIfAborted();
      let info = KpT(err);  // extract {status, error.type, message}
      if (!(PzR(info) && attempt < GpT)) throw err;  // GpT = 3
      let delayMs = Math.min(jzR * 2**attempt, EzR);  // jzR=4000, EzR=60000
      TT.warn("Subagent inference rate-limited, retrying", {
        subagentKey, attempt: attempt+1, maxRetries: GpT, delayMs, ...
      });
      await NS(delayMs, abortSignal);
      attempt++;
    }
  }
}
```

### 常量（line 64398）

| 常量 | 值 | 含义 |
|---|---:|---|
| `jzR` | **4000 ms** | 初始 retry delay（基数） |
| `EzR` | **60000 ms** | max delay cap |
| `GpT` | **3** | max retries |
| `$zR` | 3 | subagent 同一 tool error 重复容忍次数（超了 abort） |

**退避序列**：`4s → 8s → 16s`（各次 `Math.min(4000 * 2^attempt, 60000)`，第 3 次前就达到 max 16s << 60s）。

### 只对 subagent 启用

`Ls.runOne` 方法里：
```js
let {result, toolUses, ...} = await this.runInferenceWithRateLimitRetries(
  (...v) => T.runInference(...v),
  [...args, abortSignal, onProgress],
  retryOnRateLimit,   // ← spec.retryOnRateLimit，仅在 Code Reviewer / Task 等 subagent spec 开启
  spec?.key
);
```

**主对话不自动 retry 429**。只有：
- Code Review check agent：`retryOnRateLimit: true`（见 `rGR(...)`）
- 其他 subagent 按 spec 配置

## 二、Rate Limit 判定 `PzR(T)`

```js
function PzR(info) {
  let msgL = info.message.toLowerCase();
  let errMsgL = info.error?.message?.toLowerCase() ?? "";
  return info.status === 429
      || info.error?.type === "rate_limit_error"
      || msgL.includes("429") || errMsgL.includes("429")
      || msgL.includes("resource_exhausted") || errMsgL.includes("resource_exhausted")
      || msgL.includes("resource exhausted") || errMsgL.includes("resource exhausted")
      || msgL.includes("rate limit") || errMsgL.includes("rate limit")
      || msgL.includes("too many requests") || errMsgL.includes("too many requests");
}
```

**综合多种信号**：HTTP 状态码 + Anthropic error type + 字符串 fallback。"rate_limit_error" 是 Anthropic 专用，"resource_exhausted" 是 Vertex AI 专用。

## 三、Stream 错误分类（8 种 kind）

**`UWT` set**（line 65252）：

```js
UWT = new Set([
  "context_limit",
  "entitlement_limit",
  "internal_error",
  "midstream",
  "midstream_overloaded",
  "out_of_credits",
  "overloaded",
  "unauthorized",
])
```

每种 kind 对应人类可读 message（`IBR(T)`，line 62463）：

```js
function IBR(kind) {
  switch (kind) {
    case "context_limit":        return "prompt is too long, exceed context limit";
    case "entitlement_limit":    return HLR("$0", 1, "4 hours and 30 minutes");  // "try again in 4h30m"
    case "internal_error":       return "HTTP 500 internal server error";
    case "midstream":            return "HTTP 500 Response incomplete: stream ended unexpectedly";
    case "midstream_overloaded": return "HTTP 429 server overloaded";
    case "out_of_credits":       return NUT;  // "You've exceeded your usage quota of..."
    case "overloaded":           return "HTTP 429 server overloaded";
    case "unauthorized":         return "HTTP 401 unauthorized";
  }
}
```

**关键洞察**：`midstream` vs `internal_error` 是 Amp 做出的区分 —— `midstream` 是流开始后连接断开（stream incomplete），可能因为服务端 overload，行为更接近"部分成功"；`internal_error` 是请求开始就失败。重试策略可以不同。

## 四、Error Message → 错误分类函数

### `B$(T)` - 是否 context window 越界

line 62451：

```js
function B$(err) {
  let needles = [
    "prompt is too long",
    "exceed context limit",
    "context limit reached",
    "token limit exceeded",
    "context window",
    "maximum context length",
  ];
  let matches = msg => needles.some(n => (msg ?? "").toLowerCase().includes(n));
  let a = err.error?.type === "invalid_request_error" && matches(err.error.message);
  let b = matches(err.message);
  return a || b;
}
```

### `F3T(T)` - 是否 out of credits

```js
function F3T(err) {
  return err.message.includes(NUT);  // "You've exceeded your usage quota of..."
}
```

### `WLR(T)` - 是否免费用量用完

```js
function WLR(err) {
  return err.message?.includes("You've reached your free usage limit") ?? false;
}
```

### `G3T(T)` - 是否 unauthorized

（未完全提取，推测类似）：
```js
function G3T(err) {
  return err.status === 401 || err.message?.includes("unauthorized");
}
```

### `MU(T)` / `LU(T)` - overloaded / stream timeout

```js
// 根据 UdT 使用推断：
function MU(err) { return err.status === 529 || err.message?.includes("overloaded"); }
function LU(err) { return err instanceof W3T /* stream idle timeout */; }
```

### 统一的错误 → 用户消息 `UdT(err)`

line 63334：

```js
function UdT(err) {
  if (err instanceof Error) {
    if (G3T(err))  return "Unauthorized. Check your access token.";
    if (B$(err))   return "Context window limit reached.";
    if (MU(err))   return "Model provider overloaded. Try again in a few seconds.";
    if (LU(err))   return "Model stream timed out. Try again in a few seconds.";
    if (F3T(err))  return "Insufficient credit balance.";
    return err.message;
  }
  // fallback
  if (typeof err === "object" && err && "message" in err && typeof err.message === "string") {
    return err.message;
  }
  return String(err);
}
```

## 五、Stream 相关的特殊超时

### `ELR(stream, timeoutMs=120000)` - stream idle timeout

line 62451：

```js
async function*ELR(stream, timeoutMs = 120000) {
  let it = stream[Symbol.asyncIterator]();
  while (true) {
    let timer = null;
    try {
      let timeoutP = new Promise((_, reject) => {
        timer = setTimeout(() => reject(new W3T(timeoutMs)), timeoutMs);
      });
      let result = await Promise.race([it.next(), timeoutP]);
      if (result.done) return;
      yield result.value;
    } finally {
      if (timer !== null) clearTimeout(timer);
    }
  }
}
```

**含义**：如果 120 秒没收到 stream chunk，抛 `W3T(timeoutMs)` → 被 `LU(err)` 识别为 stream timeout。

### Kimi / Fireworks 的 per-chunk timeout

Fireworks stream 里还有 `e4R` = chunk 间隔告警阈值，超了 log warn 但不中断（`r4R` 是 idle timeout）。

## 六、Non-streaming timeout 计算

Anthropic SDK 的 `calculateNonstreamingTimeout(max_tokens, modelSpec)`：

```js
// line 65165
let timeout;
if (!R.stream && this._client._options.timeout == null) {
  let spec = P8T[R.model] ?? undefined;
  timeout = this._client.calculateNonstreamingTimeout(R.max_tokens, spec);
}
```

这是 Anthropic SDK 内置的 "大 max_tokens → 更长 timeout" 算法，默认 600 秒，根据 max_tokens 线性外推。

## 七、Prompt Too Long 特殊处理

Cerebras / Groq / Fireworks 都有 `HDR(T)` 或 `FUT(T)` —— 把 provider 的 "maximum context length" error 统一转成 `Im`（Amp 内部的 `ContextLimitError`）：

```js
// Cerebras HDR
function HDR(err) {
  if (isAmpErrorLike(err) && err.type === "invalid_request_error"
      && err.message.toLowerCase().includes("prompt is too long")) {
    return new Im();  // ContextLimitError
  }
  return err;
}

// Fireworks FUT
function FUT(err) {
  let msg = err?.message;
  if (typeof msg === "string") {
    if (msg.includes("maximum context length") || msg.includes("prompt is too long")
        || msg.includes("too many tokens"))
      return new Im("Token limit exceeded.");
  }
  return err;
}
```

**设计思路**：上游 error 格式各异，用一个 `ContextLimitError` 统一，上层根据类型做"压缩 + 重试"。

## 八、对 Alva 的启发

### 1. 显式 8 种 stream error kind

**当前 Alva**：`rate_limit.rs` 只追踪窗口计数 + `x-ratelimit-remaining` / `retry-after` header，没有明确的 error kind enum。

**建议**：加一个 `StreamError` enum：

```rust
// crates/alva-llm-provider/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("context limit exceeded: prompt too long")]
    ContextLimit,
    #[error("entitlement limit: try again in {retry_after_secs}s")]
    EntitlementLimit { retry_after_secs: u64 },
    #[error("internal server error (pre-stream)")]
    InternalError,
    #[error("stream ended unexpectedly")]
    Midstream,
    #[error("server overloaded mid-stream")]
    MidstreamOverloaded,
    #[error("insufficient credits")]
    OutOfCredits,
    #[error("server overloaded (pre-stream)")]
    Overloaded,
    #[error("unauthorized")]
    Unauthorized,
    #[error("stream idle timeout: {0}ms without chunk")]
    StreamIdleTimeout(u64),
    #[error("rate limit: retry after {retry_after:?}")]
    RateLimit { retry_after: Option<Duration> },
    #[error("other: {0}")]
    Other(String),
}
```

上层可根据 kind 决定：压缩 context（ContextLimit）、等一下重试（RateLimit / Overloaded）、直接报错（Unauthorized / OutOfCredits）。

### 2. 指数退避参数化 + 仅对 subagent

Amp 的选择是"**主对话不自动 retry**" —— 用户需要立即反馈。只有后台 subagent 才 retry。建议 Alva 的 `RetryPolicy` 按 `caller` 区分：

```rust
pub enum RetryPolicy {
    None,                                          // 用于主对话
    Exponential { base_ms: u64, max_ms: u64, max_attempts: u32 },  // base=4000, max=60000, attempts=3
}
```

### 3. Stream idle timeout

Amp 默认 120 秒 idle timeout。Alva 的 provider 实现（`openai_chat.rs` / `anthropic.rs`）应该在 stream loop 里加 `tokio::time::timeout(Duration::from_secs(120), stream.next())` 检测 hang。

### 4. 统一 ContextLimitError 映射

每个 provider 的 error 格式不同（Fireworks / Cerebras / Anthropic 消息都不一样）。Alva 的每个 `Provider` impl 应该负责把 upstream error → `StreamError::ContextLimit`，而不是让上层各处做字符串 match。

### 5. 错误 → 用户 message 的映射集中

参考 Amp 的 `UdT(err)` —— 一个函数负责把所有 error 翻译成用户看得懂的简短说明。建议 Alva 加：

```rust
impl StreamError {
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::Unauthorized => "Unauthorized. Check your access token.",
            Self::ContextLimit => "Context window limit reached.",
            Self::Overloaded | Self::MidstreamOverloaded => "Model provider overloaded. Try again in a few seconds.",
            Self::StreamIdleTimeout(_) => "Model stream timed out. Try again in a few seconds.",
            Self::OutOfCredits => "Insufficient credit balance.",
            _ => "(internal error)",
        }
    }
}
```

TUI / CLI 展示时优先用 `user_message()`，debug 时看 `Debug` impl。
