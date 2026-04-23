---
name: amp-observability
description: Amp 的可观测性 + rate limit 退避 + billing/usage 三合一分析。想了解 Amp CLI 怎么做日志 / OTEL tracing / 错误分类 / 自动重试 / credit 余额展示时加载。
trigger_words:
  - amp logging
  - amp tracing
  - AMP_LOG_LEVEL
  - AMP_LOG_FILE
  - AMP_CLI_STDOUT_DEBUG
  - AMP_MAX_LOG_FILE_SIZE
  - TT.debug
  - TT.info
  - startActiveSpan
  - amp usage
  - billingMode
  - freeTierEnabled
  - enterprise.selfserve
  - rate limit retry
  - ephemeralError
  - runInferenceWithRateLimitRetries
  - debug package
  - Cloudflare Logs
  - DTW Commands
---

# Amp Observability / Rate Limit / Billing

本 skill 专门拆解 Amp 可观测性与错误/计费三条管道。它们三者在反编译里密切相关：

- 日志 (`TT.debug/info/warn/error`) → Winston 写 `~/.amp/logs/cli.log` + 同时通过 `@opentelemetry/api` 当作 span event 发出
- OTEL span (`startActiveSpan`) 包裹 agent loop / tool / inference / plugin，汇入 trace store（线程数据一部分）
- 错误一旦抛出，经 `KpT / JU` 归类 → 展示给用户，UI 里 `startRetryCountdown` 做指数退避自动重试
- 计费 UI 从 server `userDisplayBalanceInfo` 拉余额，render 成 "Remaining USD / Limit USD · n% used · resets in …"

## 文件索引

| 文件 | 内容 | 何时读 |
|---|---|---|
| `./logging.md` | Winston logger `TT` 实现 + 所有 log env vars + 默认文件路径 + audit 级别 | 想给 Alva 加日志系统 |
| `./tracing.md` | `@opentelemetry/api` + NodeSDK + 自定义 traceStore + fetch/plugin/tool/inference span 自动包装 | 想做分布式 trace |
| `./debug-package.md` | `bL0` 生成的 `Debug Instructions` markdown 完整结构 + Cloudflare Logs / DTW Commands | 想抄 debug dump 功能 |
| `./rate-limit-errors.md` | 完整 15 个 error 分类函数 + 三层 retry (UI 自动 / handoff / subagent) + backoff 参数 | 想做健壮的 LLM 重试 |
| `./billing-usage.md` | `amp usage` 命令 + free tier / enterprise / enterprise.selfserve 三种 billingMode + USD 格式化 | 想做 credit / 订阅计费 |

## 常见快速问答

**Q：Amp 用什么日志库？在哪里写？**
A：Winston。默认写到 `~/.amp/logs/cli.log`，JSON 行格式。单文件上限 `AMP_MAX_LOG_FILE_SIZE`（默认 10MB）。级别通过 `AMP_LOG_LEVEL` 或 `--log-level` 设，允许 `debug / info / warn / error` 再加自定义 `audit`。用 `AMP_CLI_STDOUT_DEBUG=true` 同时输出到 stdout。详见 `./logging.md`。

**Q：Amp 有做 OpenTelemetry 吗？**
A：有。`@opentelemetry/api` + `NodeSDK`，`serviceName: "amp.cli"`，`AlwaysOnSampler`，`AsyncLocalStorageContextManager`。内建 fetch instrumentation 把每个 HTTP 请求都变成 span。Agent turn / tool / plugin hook / inference / skills / subagent 主路径都用 `tracer.startActiveSpan("tools"|"inference"|"plugin")` 包裹。**但**没有 traceExporter（`metricReader:void 0` 同样为空），trace 只在本进程内流转 → 写入 thread 的 `traceStore`，然后通过 WebSocket 发给 DTW，最终反映到 `amp debug` 的 Cloudflare dash 链接里。详见 `./tracing.md`。

**Q：rate limit 退避怎么算？**
A：Amp 有**三层独立的 retry**，参数各不相同：

| 层级 | 最大次数 | 退避公式 | 用途 |
|---|---|---|---|
| UI 自动重试（`pl.*`） | 5 | `min(5 * 2^attempt, 60)` 秒 | 用户看到的 "Retry (auto-retry in Xs)" 倒计时 |
| Handoff (`bLR`) | 3 | `min(1000 * 2^n, 10000) ms ± 20% jitter` | context 溢出后 LLM 调 handoff 的重试 |
| Subagent (`jzR/EzR`) | 3 | `min(4000 * 2^n, 60000) ms` | Subagent 内部 inference 429 重试 |

详见 `./rate-limit-errors.md`。

**Q：错误分类有几种？**
A：11 个命名判定函数：
`LU` = stream stalled / timeout，`UUT` = network error (ECON…)，`G3T` = unauthorized，`F3T` = Out of credits，`WLR` = free tier limit，`qLR` = enterprise quota exceeded，`MU` = overloaded，`zLR` = image too large，`B$` = context window，`VLR` = agent mode disabled by admin，`KLR` = stream incomplete。它们组合决定 UI 展示的 `title / description / actions`（4 种可选 action：`retry / add-credits / new-thread / handoff / dismiss`）。详见 `./rate-limit-errors.md`。

**Q：`amp usage` 打印什么？**
A：调用 server `userDisplayBalanceInfo` API，直接打印 `displayText`。内部数据结构（在 CLI UI 的 entitlement 区也用）：
```
remainingUSD, limitUSD, percentUsed, windowPeriod, windowResetsInSeconds
```
USD 用 `Intl.NumberFormat("en-US", {style:"currency", currency:"USD"})`，默认 2 位小数，余额 <0.01 时展示 3 位。free tier `canUseAmpFree` 布尔标志，出错时 message 里 `NUT="Out of credits"` 字符串触发 `F3T` 分类。详见 `./billing-usage.md`。

**Q：Amp 有集成 Sentry / PostHog 之类的 analytics 吗？**
A：**没有**。`@sentry/*` 和 `datadog` 只是 Bun 打包时的排除列表里出现，运行时未使用。Amp 选择把所有遥测都压成**OTEL trace + 自家 server 日志**，不走第三方。这点值得注意。

## 对 Alva 的启发（跨三文档）

当前 `crates/alva-app-core/src/extension/analytics.rs` 只是空壳（`log_path` + 写 debug）。参考 Amp：

1. **`AnalyticsExtension` 不等于 analytics**，应重命名或拆分：
   - `logging::LoggerExtension` — 负责 `tracing_subscriber` + 文件写入 + 环境变量解析
   - `tracing::TracerExtension` — OTEL / `tracing-opentelemetry` 搭建
   - `debug::DebugDumpExtension` — 生成 debug markdown（类似 `bL0`），方便用户贴给维护者
2. **三层 retry 各司其职**，Alva 在 middleware 层可以抄：LLM adapter 层做 subagent 式内部重试（backoff=4s×2^n），UI 层暴露给用户 "retry countdown"（5s×2^n），context 溢出后单独再试（handoff 场景）。
3. **错误分类集中在一处**（Amp 的 `JU` 函数），UI 只拿 `{title, description, actions}` 渲染。Alva 现在错误分散在各 crate，建议统一 `AlvaError` → `(ErrorCategory, Actions)` 映射。

---

## 顶层产物路径

- 反编译 strings 在 `/tmp/amp-decompile/strings.txt`
- 本目录里每个 md 都引了具体的 strings line 号（62446 / 63013 / 63086 / 63662 等），便于交叉验证
