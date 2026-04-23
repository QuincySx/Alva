# Tool Format 适配层（4 套）

Amp 内部定义一种通用 tool spec（`ToolSpec { name, description, inputSchema, ... }`），发给 LLM 时要翻译成各家 API 要求的格式。反编译里能看到 **4 种不同的翻译函数**，各自处理一种 API 形态。

## 适配函数总览

| 函数 | 目标 API | 使用的 Provider class |
|---|---|---|
| `Bx(T)` | Anthropic `messages.create` | `UBT` Anthropic |
| `N3T(T)` | OpenAI Responses API | `OUT` OpenAI |
| `qUT(T)` | OpenAI Chat Completions | `LUT` xAI, `KUT` Fireworks, `VUT` Baseten, `NX` Groq, `EWT` Moonshot, `HWT` OpenRouter, `jWT` Cerebras |
| `C3T(T)` | Google Vertex AI `generateContent` | `pNT` Vertex |

## 一、Anthropic tool format `Bx(T)`

来源：`strings.txt` 第 62407 行。

```js
function Bx(tools) {
  let seen = new Set();
  return tools
    .filter(t => {
      if (seen.has(t.name)) return false;
      return seen.add(t.name), true;
    })
    .map(t => ({
      name: t.name,
      description: t.description ?? "",
      eager_input_streaming: true,    // ← Amp 特有，告诉 server 要流式入参
      input_schema: t.inputSchema,    // 原样透传 Amp 的 JSON schema
    }));
}
```

**特点**：
- 保留原始 JSON schema（Anthropic 对 schema 容忍度高）
- 去重（同名 tool 只保留第一个）
- `eager_input_streaming: true` 是 Amp 自己的 flag，服务端在 tool_use 还在流式生成时就开始展示

## 二、OpenAI Responses API `N3T(T)`

来源：`strings.txt` 第 62445 行。

```js
function N3T(t) {
  let properties = t.inputSchema?.properties ?? {};
  let required   = t.inputSchema?.required   ?? [];
  let parameters = {
    type: t.inputSchema?.type ?? "object",
    properties,
    required,
    additionalProperties: true,   // ← 允许额外字段
  };
  return {
    type: "function",
    name: t.name,
    description: t.description || "",
    parameters,
    strict: false,                // ← 不用 strict schema validation
  };
}
```

**特点**：
- OpenAI Responses API 的 tool 结构是**扁平**的 `{type:"function", name, parameters}`（不嵌套在 `function: {...}` 里）
- `strict: false` + `additionalProperties: true` —— Amp 不用 structured output strict 模式（因为 Amp 的 JSON schema 可能用 `examples`/`default` 等非标字段）
- 无去重（调用方保证）

**handoff tool 里 `_.additionalProperties=false + strict:true`** 是 structured output 专用（`oLR(...)`），跟日常 tool_use 不同。

## 三、OpenAI Chat Completions `qUT(T)`

来源：`strings.txt` 第 62452 行。适用于 xAI/Fireworks/Baseten/Groq/Moonshot/OpenRouter/Cerebras。

```js
function qUT(tools) {
  let seen = new Set();
  return tools
    .filter(t => {
      if (seen.has(t.name)) return false;
      return seen.add(t.name), true;
    })
    .map(t => {
      let schema = t.inputSchema;
      let rawProps = schema?.properties ?? {};
      let fixedProps = {};
      // YLR 修复缺失 type 的 property
      for (let [key, val] of Object.entries(rawProps)) {
        fixedProps[key] = YLR(val);   // 如果 val.items 存在但无 type，填 "array"；否则 "object"
      }
      return {
        type: "function",
        function: {
          name: t.name,
          description: t.description ?? "",
          parameters: {
            type: schema?.type ?? "object",
            properties: fixedProps,
            required: schema?.required ?? [],
            additionalProperties: true,
          },
        },
      };
    });
}

function YLR(prop) {
  if (prop.type) return prop;
  if (prop.items !== undefined) return {...prop, type: "array"};
  return {...prop, type: "object"};
}
```

**特点**：
- OpenAI Chat Completions API 的 tool 是**嵌套**的 `{type:"function", function: {name, parameters}}`
- **`YLR` schema 修复**：某些 tool 的 schema property 可能漏写 `type`（Amp 内部用 TypeScript 推断），这里补全
- 同样 `additionalProperties: true`

## 四、Vertex AI Gemini `C3T(T)`

来源：`strings.txt` 第 62428 行。

```js
function C3T(tools) {
  if (tools.length === 0) return [];
  return [{
    functionDeclarations: tools.map(gPR)
  }];
}

function gPR(t) {
  return {
    name: t.name,
    description: t.description ?? "",
    parameters: sV(t.inputSchema),
  };
}

function sV(schema) {
  let result = {};
  let gType = SNT[schema.type ?? "any"];   // 映射 JSON schema type → Gemini enum
  if (gType) result.type = gType;
  if (schema.description) result.description = schema.description;
  if (schema.required)    result.required = schema.required;
  let examples = schema.examples;
  if (Array.isArray(examples) && examples.length > 0) {
    result.example = examples[0];         // ← JSON schema 用 examples 数组, Gemini 用单个 example
  }
  if (schema.properties) {
    result.properties = Object.fromEntries(
      Object.entries(schema.properties).map(([k, v]) => [k, sV(v)])  // 递归
    );
  }
  if (schema.items) result.items = sV(schema.items);
  return result;
}

// SNT 枚举映射
SNT = {
  string:  yh.STRING,
  number:  yh.NUMBER,
  integer: yh.INTEGER,
  boolean: yh.BOOLEAN,
  object:  yh.OBJECT,
  array:   yh.ARRAY,
}
```

**特点**：
- Gemini 要求 **`functionDeclarations` 数组**包在最外层 `tools` 里（单个元素，多 function）
- JSON schema `type: "string"` → `type: yh.STRING`（Gemini 用 enum 而非字符串字面量）
- JSON schema `examples: [...]` → Gemini `example` 单值（取第一个）
- **递归**处理 `properties` / `items`，因为 Gemini 也要求嵌套的 type field

## 五、Tool Use Response 回传的差异

各家 provider 返回 tool_use 的格式也不同，Amp 也有反向翻译。

### Anthropic → Amp

直接用 `{type:"tool_use", id, name, input}` blocks，本来就跟 Amp 内部模型一致。

### OpenAI Chat → Amp（Fireworks 等）

```js
// from GUT(...) streaming processor
if (ChoicesDelta.tool_calls) {
  for (let tc of ChoicesDelta.tool_calls) {
    if (tc.id) {
      toolUses.push({
        id: tc.id,
        name: tc.function?.name ?? "",
        input: tc.function?.arguments ? JSON.parse(tc.function.arguments) : undefined,
      });
    }
  }
}
```

### OpenAI Responses → Amp

```js
// from RLR(...)
case "function_call": {
  try {
    let parsed = JSON.parse(A.arguments);
    blocks.push({type:"tool_use", complete:true, id: A.call_id, name: A.name, input: parsed, ...});
  } catch {
    blocks.push({type:"tool_use", complete:false, id: A.call_id, name: A.name, inputPartialJSON: {...}});
  }
  break;
}
```

### Vertex AI Gemini → Amp

```js
// from SU / dPR
let functionCall = message.functionCalls?.at(0);
// → convert to {type:"tool_use", id: <generated>, name: functionCall.name, input: functionCall.args}
```

**特殊点**：Gemini 不返回 tool_use id，Amp 需要自己生成一个（用 `toolu_${randomID}` 格式）。反向翻译回 Amp 格式时，ID 用 `KDR(T)` 做 sanitize：

```js
function KDR(T) {
  return `toolu_${T.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}
```

## 六、Kimi / Cerebras 的 tool_use ID 规范化

Fireworks / Moonshot 的 OpenAI-compat tool_call ID 不保证以 `toolu_` 开头，Amp 做了统一前缀（`KDR`）。然后在回传 tool_result 时把 `toolu_` 去掉（`tc.id.replace(/^toolu_/, "")`）。

## 七、Schema "fix" 汇总

| 函数 | 做了什么 |
|---|---|
| `Bx` (Anthropic) | 仅去重，schema 原样 |
| `N3T` (Responses) | 固定 `additionalProperties: true`, `strict: false` |
| `qUT` (Chat) | `YLR` 补全缺失 `type`（array/object 推断）+ `additionalProperties: true` |
| `C3T` (Gemini) | 全部递归、`examples → example`、type 枚举映射、去掉多余字段 |

**大致趋势**：Anthropic > OpenAI Chat > OpenAI Responses > Gemini，后者对 schema 约束越来越严，Amp 的翻译工作越来越多。

## 八、Tool Use 流式处理差异

### Anthropic - 原生支持 tool_use stream

`content_block_start` / `content_block_delta` 事件里 `input` 字段逐步增长（JSON partial），Amp 用 `__json_buf` 累积字符串，end 时 parse。

### OpenAI Chat - tool_call 分片 function.arguments

`choices[0].delta.tool_calls[i].function.arguments` 是部分字符串，需要按 index 累积。Amp 在 `L$(prev, chunk)` 里做 merge。

### Vertex AI Gemini - 不流式 functionCall

Gemini 只在 stream 结束时一次性返回 `functionCalls`，无 partial。Amp 里用 `j$(...)` 非-stream 调用做 tool_choice=ANY 的 handoff。

### OpenAI Responses - 独立 function_call item

stream 里 `output` 数组出现 `{type:"function_call", name, arguments, call_id}` item，Amp 在 `RLR(...)` 里 case `function_call` 分支处理。

## 九、对 Alva 的启发

### 1. 加 `ToolAdapter` trait

当前 Alva 的 `AnthropicProvider` / `OpenAIChatProvider` / `OpenAIResponsesProvider` 可能各自实现 tool 序列化。建议提取：

```rust
// crates/alva-llm-provider/src/tool_adapter.rs
pub trait ToolAdapter {
    fn serialize_tool_spec(&self, spec: &ToolSpec) -> serde_json::Value;
    fn parse_tool_use(&self, api_response: &serde_json::Value) -> Result<Vec<ToolUse>>;
}

pub struct AnthropicToolAdapter;
pub struct OpenAIResponsesToolAdapter;
pub struct OpenAIChatToolAdapter;
pub struct VertexAIGeminiToolAdapter;
```

让每个 Provider impl 持有一个 adapter instance，序列化/反序列化集中处理。

### 2. Schema 修复（针对 Chat Completions）

如果 Alva 支持 OpenAI-compatible provider（Groq/Fireworks/OpenRouter），需要学 Amp 的 `YLR` 修复缺失 `type` 的 schema property。**Anthropic 容忍，但 OpenAI Chat 严格**。

### 3. Tool use ID 前缀统一

Amp 的 `toolu_${sanitized}` 方案 —— 无论上游用什么 ID 格式，对内统一 `toolu_` 前缀，对外再去掉。这保证了：
- Alva 内部处理 tool_use / tool_result 对的时候不用关心 ID 来源
- 给 OpenAI 回 tool_result 时记得 strip `toolu_` 前缀

### 4. Gemini 的特殊性

如果 Alva 要支持 Vertex AI，要做最多的适配工作：
- `type: "string"` → gemini enum（要依赖 Google SDK 的常量）
- `examples: [...]` → `example: <first>`
- 递归处理 properties/items
- tool_use id 需要自己生成（Gemini 不返回 id）
- stream 不支持 tool_use partial，只能 sync 返回

### 5. `additionalProperties` 该 true 还是 false?

Amp 日常 tool_use **都用 `additionalProperties: true`**（宽松），只有在 structured output（`handoff` 工具的 `oLR`）用 `false + strict: true`。Alva 跟进时也应该区分这两种场景。

### 6. `eager_input_streaming` 值得抄

Amp 给 Anthropic 加了 `eager_input_streaming: true` flag（不是官方 API，是 Amp 自己 proxy 加的？）—— 让 server 在 tool_use input 还在流式时就把中间 JSON 发给客户端。Alva 如果做 SaaS proxy，可以加类似 flag 优化 TUI 的即时反馈。
