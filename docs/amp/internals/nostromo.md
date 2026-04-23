# Nostromo —— Amp 内部的 Scenario 回放 Fake LLM

> Amp 有一个**伪装成 LLM provider 的本地脚本引擎**，用来跑确定性 agent 行为测试。名字取自《异形》的飞船 Nostromo（Sourcegraph 内部梗）。

---

## 强证据 vs 推测

**强证据**（从代码行为直接读出）：
- model 名前缀路由、DSL parser、streaming 时序模拟都完整存在。
- 错误类型（`context_limit`、`midstream`、`unauthorized` 等）和真 LLM 对齐，说明是为了**替换真 LLM 做测试**。

**推测**：
- 具体谁在用 —— 没证据说是 CI / QA / 开发者本地，但从设计看最可能是 **Sourcegraph 内部 eval / scenario 测试框架**喂给 agent 的 replay 数据源。
- 有无 Alva-eval 类似的 "scenario runner"：binary 里没看到独立 runner，可能是直接通过 model setting 切换。

## 入口：Model 前缀路由

位置：function `PBR(T, R)` —— provider 工厂。

```js
if (T === "openai" && R?.startsWith("amp-nostromo-")) return new wWT;
```

**意思**：只要 model ID 是 `amp-nostromo-<suffix>` 且 provider 被设置成 `openai`，Amp 就 **bypass 真 OpenAI**，创建一个本地 `wWT` 实例。`wWT` 实现了和其他 provider 相同的 `stream()` async generator 接口，Amp 的 inference pipeline 感知不到差异。

**推测**：suffix 可能是 scenario 名（例如 `amp-nostromo-tool-error`、`amp-nostromo-context-limit`），但 binary 里没列出具体 suffix 列表。

## Scenario DSL

Scenario 写在 **最近一条 user message 的文本里**，有两种容器：
1. Fenced code block：`` ```nostromo ... ``` ``
2. 行首前缀 `nostromo:` 后面跟 DSL。

**提取逻辑**（`eBR` 函数）：
```js
function eBR(T) {
  let R = T.match(/```nostromo\s*([\s\S]*?)```/i);
  if (R?.[1]) return R[1].trim();
  let t = T.toLowerCase().indexOf("nostromo:");
  if (t !== -1) return T.slice(t + "nostromo:".length).trim();
  return null;
}
```

### 支持的语句（6 种）

从 parser `p4` 的分支读出：

| 语法 | 含义 |
|---|---|
| `tool <name> <json-input>` | 让 fake LLM 触发一次 tool call |
| `reply <text>` / `reply <<LABEL ... LABEL` | 输出一段 assistant text（heredoc 形式支持多行） |
| `error <kind>` / `error <kind> after <N> chunks` | 流式中途抛错（模拟真 LLM 失败） |
| `delay <duration>` | 插入延迟（模拟思考时间 / 网络延迟） |
| `repeat <N>:` + 缩进块 | 循环 N 次 |
| `if <expr>:` + 缩进块 / `else:` | 条件分支 |

### if 表达式 —— 唯一支持的判断

```
if result[0].contains("hello world"):
  reply ok
else:
  reply not found
```

- `result[N]` 引用第 N 次 tool 调用的结果。
- 支持 `.text`、`.json.path.to.field` 访问器。
- 只有 `.contains("literal")` 这一个谓词（没有 equals、regex）。

### 错误 kind 白名单

从 `IBR` 函数读出 8 种：

```
context_limit          → "prompt is too long, exceed context limit"
entitlement_limit      → "…4 hours and 30 minutes"
internal_error         → "HTTP 500 internal server error"
midstream              → "HTTP 500 Response incomplete: stream ended unexpectedly"
midstream_overloaded   → "HTTP 429 server overloaded"
out_of_credits         → (跳转到别处定义的消息)
overloaded             → "HTTP 429 server overloaded"
unauthorized           → "HTTP 401 unauthorized"
```

**这说明**：Nostromo 的主要用途是**用确定性回放覆盖各种失败路径**（context 爆、流中断、429、401）—— 一看就是给 agent 的失败处理逻辑做 smoke test。

## 流式时序模拟

`kBR` 是主 streaming 函数，模拟两种输出风格：

1. **`chunked`**（匀速定长 chunk）—— `Gf=5` 字符一个 chunk。
2. **`paced`** —— 按词切分，每词延迟 `1000/speedTokensPerSecond * (1 ± jitter)`，段落间加 `paragraphPauseMs`。

**随机但确定**：jitter 是用 FNV-1a hash (`gBR(text, index)`) 算的，所以同一个 scenario **多次运行输出时序完全一样** —— 测试可复现。

## Scenario 状态机

```
入口：xBR(thread) → 找到最近带 scenario 的 user message
     → dBR 检查已执行的 tool_result 序列
     → pBR 推进 "每轮 tool 结果对应一段 reply+tools" 的游标
     → DmT 吐出本轮的 {text, tools, delay, error}
     → kBR 流式 yield ModelStreamEvent
```

**关键**：scenario 是**有记忆的** —— 每当真 tool 执行完返回结果，游标前进一格。所以可以写:

```
nostromo:
tool list_files {"path": "/tmp"}
if result[0].contains("foo"):
  reply found foo
  tool read_file {"path": "/tmp/foo"}
else:
  reply no foo
  error context_limit
```

—— Nostromo 会调 `list_files`，等真实 tool 结果回来，判断包含 foo 后决定继续 `read_file` 还是抛 context_limit。

## 用途推测

强推测（基于设计动机）：
1. **Replay bug**：线上 LLM 的某次"奇怪"输出，用 scenario 精确还原，本地 debug。
2. **Agent 行为测试**：给 subagent / orchestrator 的失败恢复路径写 scenario，验证 handoff / error recovery。
3. **快速 e2e 测试**：不花 token、不走网络、完全确定性，CI 里用得上。

弱推测：
- 可能被 Sourcegraph 内部的 eval 框架 wrap 成 "scenario file"（`.nostromo.md` ？）—— 但 binary 里**没看到文件后缀约定**，只看到"从 user message 里提取"的逻辑。

## 错误消息

```
"nostromo is waiting for a user message."
"nostromo scenario parse error: <detail>"
"nostromo scenario complete."
"nostromo inference aborted"   // AbortError.message
"nostromo scenario is empty"
```

这些消息会作为 assistant output 返回，**调用方（agent 逻辑）感知不到是 fake** —— 完全走正常 LLM 流程。

## 对 Alva 的启发

`alva-app-eval` 可以抄这套设计：

1. **Fake `LlmProvider` 实现**：Alva 的 `LlmProvider` trait 已经有，直接加一个 `ScenarioProvider { script: Scenario }`，model 名 `alva-scenario-*` 触发。
2. **Scenario DSL**：抄 Nostromo 6 种语句 + `if .contains()` 判断就够了。可以考虑用 TOML / KDL 而不是自定义缩进 parser（Rust 里 serde_kdl 省事）。
3. **确定性 jitter**：抄 FNV hash 思路，保证 CI 可复现。
4. **错误注入**：Alva 的 `LlmError` 枚举每个 variant 对应一个 scenario `error` kind —— 直接覆盖失败路径测试。
5. **有记忆的游标**：scenario 必须跟着 tool 结果推进，**不是**无状态 replay —— 这是 Nostromo 设计的精髓。

具体落地位置：`alva-app-eval` crate 加 `scenario/` 模块，和 recorder/inspector 并列（见用户 memory `project_alva_eval.md`）。

## 引用

- `/tmp/amp-decompile/strings.txt` 62457-62464 行：`wWT`、`LWT`、`eBR`、`p4`、`kBR`、`IBR`、`xBR`、`dBR`、`pBR` 全链路。
- `PBR` (provider factory) 在 62464 行，`amp-nostromo-` 字符串路由分支。
- `BmT="nostromo:"` 常量定义在 62459 行附近。
