# Amp Billing / Usage / Free Tier

> `amp usage` 命令 + UI "Usage Entitlement" 区块 + 三种 `billingMode` + free-tier hourly reset + `NUT="Out of credits"` 的链路。

## `amp usage` CLI 命令

从 strings line 64042 提取（去混淆）：

```js
function DF0(program, getContext) {
  program.command("usage")
    .description("Show your current Amp usage and credit balance")
    .action(async (flags, cmd) => {
      const globals  = cmd.optsWithGlobals();
      const ctx      = await getContext(globals);
      const proxy    = await ctx.settings.get("proxy");
      const client   = createInternalAPIClient({
        settings: { url: ctx.ampURL, proxy },
        secrets:  { getToken: (k, url) => ctx.secrets.get(k, url) },
      });

      const response = await client.userDisplayBalanceInfo({}, { config: ctx });
      if (!response.ok) {
        if (response.error.code === "auth-required") {
          process.stderr.write(red("Error: ") + "You must be logged in to view usage. Run `amp login` first.\n");
          process.exit(1);
        }
        process.stderr.write(red("Error: ") + response.error.message + "\n");
        process.exit(1);
      }

      process.stdout.write(await LF0(response.result.displayText) + "\n");
      process.exit(0);
    });
}
```

要点：
- 命令直接 passthrough server 返回的 `displayText` 给用户（Amp 服务端决定格式）
- 客户端只负责鉴权 + 渲染 markdown (`LF0` 把 markdown 变成终端 ANSI)
- 未登录直接引导 `amp login`

## UI 里的 Usage Entitlement 区块

strings line 66308 附近是 context-window 窗口里的一个子区块。调用 `userDisplayBalanceInfo` 之外还有结构化字段：

```js
const entitlement = widget.costInfo?.entitlement;
if (entitlement) {
  const remainingFmt = bS(entitlement.remainingUSD, { intent: "balance" });
  const limitFmt     = bS(entitlement.limitUSD,     { intent: "balance" });
  const percent      = showExactNumbers
    ? entitlement.percentUsed.toFixed(2)
    : String(Math.round(entitlement.percentUsed));
  const resetsIn = q3T(entitlement.windowResetsInSeconds * -1000, { future: true, verbose: true });

  lines.push(new Text(`  ${remainingFmt} remaining of ${limitFmt} ${entitlement.windowPeriod} limit\n`));
  lines.push(new Text(`  ${percent}% used`));
  lines.push(new Text(" · "));
  lines.push(new Text(`resets in ${resetsIn}\n`));
}
```

`entitlement` 对象的 shape（从 UI 字段反推）：

```ts
interface Entitlement {
  remainingUSD: number;
  limitUSD: number;
  percentUsed: number;                  // 0-100
  windowPeriod: "hour" | "day" | "week" | "month";
  windowResetsInSeconds: number;        // 未来时间（正数）或过去时间（？）
}
```

**渲染示例**：
```
Usage Entitlement

  $3.42 remaining of $5.00 hour limit
  31% used · resets in 24 minutes
```

## USD 格式化 `bS`

strings line 63784。精度规则：

```js
const a2 = 2;  // 默认 2 位小数

function hD0(T, R) {
  // "more-if-tiny": 余额 <0.01 时显示 3 位小数
  const decimalPlaces = R.decimalPlaces === "more-if-tiny"
    ? (Math.abs(T) < 0.01 ? a2 + 1 : a2)
    : (R.decimalPlaces ?? a2);
  // intent:"balance" 用保守舍入：负数 "expand"（更负），正数 "trunc"（不上舍）
  // → 显示余额时宁可少给用户感知 1 美分也不虚报
  const roundingMode = R.intent === "balance"
    ? (T < 0 ? "expand" : "trunc")
    : "halfExpand";
  return { decimalPlaces, roundingMode };
}

function bS(T, R) {
  const { decimalPlaces, roundingMode } = hD0(T, R);
  const formatted = T % 1 === 0 && !R.alwaysShowCents
    ? T.toLocaleString("en-US", { style: "currency", currency: "USD",
        minimumFractionDigits: 0, maximumFractionDigits: 0, roundingMode })
    : T.toLocaleString("en-US", { style: "currency", currency: "USD",
        minimumFractionDigits: decimalPlaces, maximumFractionDigits: decimalPlaces,
        roundingMode });
  return R.includeCurrencyCode ? `${formatted} USD` : formatted;
}
```

**关键特性**：
- 整数不显示小数（`$5` 而非 `$5.00`），除非 `alwaysShowCents: true`
- 精度 2 位小数，但余额 < $0.01 时自动用 3 位（防止显示 `$0.00`）
- 余额场景（`intent: "balance"`）用"保守舍入"：正数 trunc，永不把 $3.497 显示成 $3.50

额外的 "big number" 格式化（用在 token 数等场景）：

```js
function o7(T) {
  if (T >= 1e6)  return `${Math.round(T / 1e6)}M`;
  if (T >= 1000) return `${Math.round(T / 1000)}k`;
  return T.toString();
}
```

## 三种 Billing Mode

从 strings line 63315 / 64101 提取：

```js
// 能配置 "默认 thread 可见性" 的场景
if (!(team?.billingMode === "enterprise" || team?.billingMode === "enterprise.selfserve"))
  v8(`Default visibility is only configurable in enterprise workspaces.`);
```

即 `team.billingMode` 有至少 3 种值：

| billingMode | 含义 | 对应 UI |
|---|---|---|
| 未设置 / `individual`（推断） | 个人用户，按用量或 free tier | 展示 `remainingUSD / limitUSD` 窗口 |
| `enterprise` | 企业传统合同 | 额外解锁 "Default visibility" 设置 |
| `enterprise.selfserve` | 企业自助版（Stripe 付费） | 同上 |

Enterprise 能额外设默认可见性 (`private / public / workspace`)，对应 strings line 63315 附近的 `["private", "public", "workspace"]` 枚举。

个人 / 自助付费用户看到的错误消息会带 `/pay` 链接（strings line 63086 `uB("add-credits")`）：

```
Add Credits (ampcode.com/pay)
```

## Free Tier

strings line 64072 / 66356 提取：

```js
// 每个 thread worker 持有
this.freeTierStatus = { canUseAmpFree: false };

// 定期通过 API 更新：
const rt = AbortSignal.timeout(r4R);    // r4R = 120000 (2 分钟)
const cT = await client.getUserFreeTierStatus({}, { config: ctx.configService, signal: rt });
if (cT.ok) {
  TT.info("User free tier status:", cT);
  this.freeTierStatus = cT.result;
}
```

### Free Tier 的 UX 差异

错误分类（`./rate-limit-errors.md` 里的 `JU`）里：

```js
if (F3T(T) || WLR(T)) {
  const emailSuffix = R.userEmail ? ` Signed in as ${R.userEmail}.` : "";
  if (R.freeTierEnabled) return {
    title: "Out of Credits",
    description: `Add credits to keep using Amp right now, or wait until the next hour starts for more free usage.${emailSuffix}`,
    actions: ["add-credits", "retry"],
  };
  return {
    title: "Out of Credits",
    description: `Add credits to keep using Amp.${emailSuffix}`,
    actions: ["add-credits", "retry"],
  };
}
```

意思：free tier **按小时刷**。用户花光本小时额度，要么掏钱，要么等下一个小时窗口开始。付费用户则没有"等"这个退路。

## 配额超出的消息模板

strings line 64184 的 `HLR`（"human-limit-reached"）：

```js
const HLR = (tokens, days, resetIn) => {
  const dayLabel = days === 1 ? "day"
                 : days === 30 ? "month"
                 : `${days} days`;
  return `You've exceeded your usage quota of ${tokens} per ${dayLabel}. Your quota will reset in ${resetIn}. Contact your administrator to adjust your usage quota.`;
};
```

例：`You've exceeded your usage quota of 2,000,000 tokens per day. Your quota will reset in 4 hours. Contact your administrator to adjust your usage quota.`

这条消息被 `qLR(err)` 识别（matches `"You've exceeded your usage quota of"`）→ 分类为 `Usage Quota Exceeded`，action=`retry`。

## "Out of credits" 错误链

整条 Server → Client 的错误链路：

1. Server 检测到账户余额不足，返回 JSON error：`{ "type": "error", "error": { "type": "out_of_credits", "message": "Out of credits" } }`（或 message 含此字符串）
2. CLI `KpT(err)` 归一化 → `{ message: "...Out of credits...", error: { type: "out_of_credits" } }`
3. `F3T(normalized)` 匹配 `message.includes("Out of credits")` → true（`NUT = "Out of credits"`）
4. `JU(normalized, { userEmail, freeTierEnabled })` 分派 → `{ title: "Out of Credits", actions: ["add-credits", "retry"] }`
5. UI 渲染 "Add Credits (ampcode.com/pay)" + "Retry" 两个按钮
6. 用户点 Retry，`ThreadWorker.retry()` 重新 inference

## `/pay` 链接

strings line 63086 `uB("add-credits")`：

```js
case "add-credits": {
  let label = "Add Credits";
  if (options?.ampURL) {
    const stripped = new URL("/pay", options.ampURL).toString().replace(/^https?:\/\//, "");
    label += ` (${stripped})`;  // 例: ampcode.com/pay
  }
  return label;
}
```

注意点击后并未直接打开浏览器，只是**展示 URL**。用户得手动去（可能是为了安全 / 避免恶意 ampURL）。

## API 表

从客户端调用看到的 user / billing 相关 API：

| 方法 | 返回 | 用途 |
|---|---|---|
| `getUserInfo` | `{ user, features, team, mysteriousMessage }` | 用户 + team + 企业特性 |
| `userDisplayBalanceInfo` | `{ displayText: markdown }` | `amp usage` 命令的原文 |
| `getUserFreeTierStatus` | `{ canUseAmpFree: bool, ... }` | 是否能用 free tier |

`team.billingMode` 在 `getUserInfo` 响应里。`features` 是个 enum array（`Ia.V2`、`Ia.NEO_TUI` 等是 feature flags）。

## 对 Alva 的启发

Alva 目前没 billing（开源 local CLI）。但若未来出云版本：

### 抄的 4 点

1. **`intent: "balance"` 保守舍入**：给用户展示余额时永不上舍。这是**信任设计**（"我看到 $3.42 就是至少有 $3.42"）。`rust_decimal::RoundingStrategy::ToZero` 即可。

2. **余额 <$0.01 自动加精度**：免得展示 `$0.00` 误导用户。`$0.003 remaining` 比 `$0.00 remaining` 对用户有意义。

3. **"Out of Credits" 不等于 "Auth Failed"**：分成独立 category，UI 给 `Add Credits` 按钮而不是 `Re-login`。Alva 当前的错误分类没考虑这层。

4. **`displayText` 模式**：`amp usage` 命令把**渲染责任交给 server**（返回 markdown 让 CLI 只负责着色）。好处：
   - 未来改计费规则（"本月前 $10 免费"）不用发 CLI 版本
   - 促销文案能在 server 侧 A/B
   - Alva 如果未来接 Stripe，可以设计成 `client.settings.get_billing_display()` 返回 markdown，前端只 render

### Rust 实现草稿

```rust
// crates/alva-billing/src/format.rs
use rust_decimal::{Decimal, RoundingStrategy};

pub enum FormatIntent { Balance, Cost, Other }

pub fn format_usd(amount: Decimal, intent: FormatIntent) -> String {
    let decimals = if amount.abs() < Decimal::new(1, 2) { 3 } else { 2 };   // $0.003 vs $3.42
    let strategy = match intent {
        FormatIntent::Balance if amount.is_sign_positive() => RoundingStrategy::ToZero,
        FormatIntent::Balance => RoundingStrategy::AwayFromZero,
        _ => RoundingStrategy::MidpointAwayFromZero,
    };
    let rounded = amount.round_dp_with_strategy(decimals, strategy);
    format!("${:.*}", decimals as usize, rounded)
}
```

### 错误分类 → Action 映射

Alva 现在没有 "out of credits" 概念。当 Alva 上云：

```rust
pub enum ErrorAction {
    Retry { auto_retry_countdown_secs: Option<u32> },
    AddCredits { pay_url: Url },
    NewThread,
    Handoff,
    Dismiss,
    Reauth,  // Alva 独有（Amp 用 Unauthorized + Retry 代替，稍生硬）
}
```

---

## 交叉引用

- `NUT = "Out of credits"` 错误归一化见 `./rate-limit-errors.md` 的 `F3T`
- "retry countdown" 行为见 `./rate-limit-errors.md` 的层 1
- `amp login` + 鉴权流见 `../prompts/`（如果未来补）

## 原始产物位置

- strings line 63086：`JU` 里 Out of Credits / free tier 分支
- strings line 63315：Enterprise 专属可见性检查
- strings line 63784：`bS` (formatUSD) + `hD0` (精度 & 舍入策略)
- strings line 64042：`amp usage` 命令 (`DF0`)
- strings line 64072：`getUserFreeTierStatus` 调用
- strings line 64101：`billingMode === "enterprise" / "enterprise.selfserve"`
- strings line 64184：`NUT / HLR / r4R` 常量
- strings line 66308：UI Usage Entitlement 区块
- strings line 66356：`freeTierStatus.canUseAmpFree` 默认值
- strings line 64780：`a2 = 2`（默认小数位数）
