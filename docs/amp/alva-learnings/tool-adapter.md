# ToolAdapter —— 把 Tool 翻译给各家 LLM API

> 把 Alva 现有的 3 个散落 free function 升级为统一的 `ToolAdapter` trait，
> 补齐 schema 修复 / tool_use ID 归一化 / Vertex Gemini 支持，准备接入第 4+ 家 provider。

**优先级**：Tier 1（高）。现有代码已经有三套对应实现，改造成本低、收益大。

---

## 一、背景：Amp 是怎么做的

Amp 用**一份内部 ToolSpec**（name/description/inputSchema），发给不同 LLM 时走 4 套翻译函数：

| 函数 | 目标 API | 特殊处理 |
|---|---|---|
| `Bx(T)` | Anthropic | 去重、加 `eager_input_streaming: true` flag |
| `N3T(T)` | OpenAI **Responses** API | 扁平结构，`strict: false, additionalProperties: true` |
| `qUT(T)` | OpenAI **Chat** Completions | 嵌套结构、`YLR()` 修复缺失 `type`、去重 |
| `C3T(T)` | Vertex AI Gemini | `functionDeclarations`、type enum、`examples→example`、递归 |

完整细节见 [`../models/adapter-layer.md`](../models/adapter-layer.md)。

---

## 二、Alva 现状（基于 commit 时代码）

### 文件分布

```
crates/alva-llm-provider/src/provider/
├── anthropic.rs        L660  fn to_anthropic_tools()         → Vec<AnthropicToolDef>
├── openai_chat.rs      L562  fn to_oai_tools()                → Vec<OaiToolDef>
└── openai_responses.rs L492  fn to_responses_tools()          → Vec<Value>
```

每个 provider 文件内自己实现序列化和反序列化。Tool trait 统一暴露：

```rust
// crates/alva-kernel-abi/src/tool/types.rs:125
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    // ...
}
```

### 现有 3 套实现速览

```rust
// anthropic.rs:660
fn to_anthropic_tools(tools: &[&dyn Tool]) -> Vec<AnthropicToolDef> {
    tools.iter().map(|t| AnthropicToolDef {
        name:         t.name().to_string(),
        description:  t.description().to_string(),
        input_schema: t.parameters_schema(),         // ← 原样透传
    }).collect()
}

// openai_chat.rs:562
fn to_oai_tools(tools: &[&dyn Tool]) -> Vec<OaiToolDef> {
    tools.iter().map(|t| OaiToolDef {
        tool_type: "function".into(),
        function:  OaiFunctionDef {
            name:        t.name().into(),
            description: t.description().into(),
            parameters:  t.parameters_schema(),      // ← 原样透传
        },
    }).collect()
}

// openai_responses.rs:492
fn to_responses_tools(tools: &[&dyn Tool]) -> Vec<Value> {
    tools.iter().map(|t| serde_json::json!({
        "type": "function",
        "name": t.name(),
        "description": t.description(),
        "parameters": t.parameters_schema(),         // ← 原样透传
    })).collect()
}
```

### Gap 分析

| Gap | Alva 现状 | Amp 做法 | 严重度 |
|---|---|---|---|
| 没抽象 | 3 个 free fn 散在 3 个文件 | 4 套都在 `adapter` 模块 | 🟡 中 |
| 没去重 | 调用方保证 | `seen: Set` | 🟡 中 |
| 没 schema 修复 | `parameters_schema()` 原样出 | `YLR()` 补缺失 `type`（array/object 推断） | 🔴 高（Groq/Fireworks 严格模式会 reject） |
| 不区分 strict | 无 | 日常 `strict:false`；structured output `strict:true` | 🟡 中 |
| 没 `additionalProperties` 兜底 | 无显式设 | `true`（日常）/ `false`（structured） | 🟡 中 |
| 无 Vertex/Gemini | 不支持 | `C3T()` 完整 | 🟢 低（按需） |
| 无 tool_use ID 归一化 | 各自处理 | `toolu_${sanitized}` 统一前缀 | 🔴 高（Gemini 不返 id） |
| 反向解析也散 | 3 个 `from_*_response` 各自实现 | 每个 adapter 都有 parse + streaming merge | 🟡 中 |

---

## 三、目标设计

### trait 签名

```rust
// crates/alva-llm-provider/src/tool_adapter.rs（新建）

use alva_kernel_abi::{Tool, AgentError};
use alva_kernel_abi::base::message::ContentBlock;
use serde_json::Value;

/// 把一个 provider 的 tool API 翻译收敛成单一接口。
///
/// 为什么是 trait 而不是 enum：
/// - 各家 API 的响应结构差异极大（见 adapter-layer.md）
/// - Vertex/Gemini 需要递归 schema rewrite，逻辑多到放 enum arm 里会爆
/// - 未来 provider（Cohere、Mistral、Bedrock）继续按 impl 加
pub trait ToolAdapter: Send + Sync {
    /// Provider 标识（debug / telemetry 用）
    fn provider_name(&self) -> &'static str;

    /// 把 Alva 的 Tool 集合翻译成该 provider 的 tool 表。
    /// 实现方应做：(1) 去重 (2) schema 修复 (3) 字段重命名。
    fn serialize_tools(&self, tools: &[&dyn Tool]) -> Value;

    /// 结构化输出时使用的严格版序列化。
    /// 默认调 `serialize_tools`；需要 strict schema 的 provider 重写。
    fn serialize_tools_strict(&self, tools: &[&dyn Tool]) -> Value {
        self.serialize_tools(tools)
    }

    /// 从 provider 的 tool_call 响应块里抽 ContentBlock::ToolUse。
    /// - `raw_block` 是 provider-specific 的 JSON（Anthropic 的 content block
    ///   / OpenAI Chat 的 tool_call / Responses 的 output item / Gemini 的 functionCall）
    fn parse_tool_use(&self, raw_block: &Value) -> Result<ContentBlock, AgentError>;

    /// 把工具名加上 provider 需要的前缀（Alva 内部始终 `toolu_` 统一）。
    fn normalize_tool_use_id(&self, raw_id: Option<&str>) -> String {
        match raw_id {
            Some(id) if id.starts_with("toolu_") => id.to_string(),
            Some(id) => format!("toolu_{}", sanitize_id(id)),
            None => format!("toolu_{}", uuid::Uuid::new_v4().simple()),
        }
    }

    /// 回发 tool_result 时把内部前缀去掉。默认去 `toolu_`。
    fn denormalize_tool_use_id(&self, internal_id: &str) -> String {
        internal_id.strip_prefix("toolu_").unwrap_or(internal_id).to_string()
    }
}

/// Gemini 等 provider 要求 tool_call_id 只含 [a-zA-Z0-9_-]。
fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}
```

### 辅助：schema 修复

```rust
// crates/alva-llm-provider/src/schema_fix.rs（新建）

use serde_json::{Map, Value};

/// 对应 Amp 的 YLR()：如果 property 缺 type，从结构推断补上。
///
/// - 有 `items` → "array"
/// - 其他 → "object"
///
/// OpenAI Chat 严格模式要求每个 property 都有 type，否则 400 error。
pub fn fix_property_types(properties: &Map<String, Value>) -> Map<String, Value> {
    properties
        .iter()
        .map(|(k, v)| (k.clone(), infer_type(v)))
        .collect()
}

fn infer_type(prop: &Value) -> Value {
    let Value::Object(obj) = prop else { return prop.clone() };
    if obj.contains_key("type") { return prop.clone(); }

    let mut fixed = obj.clone();
    let inferred = if obj.contains_key("items") { "array" } else { "object" };
    fixed.insert("type".into(), Value::String(inferred.into()));
    Value::Object(fixed)
}
```

### 辅助：Tool 去重

```rust
pub fn dedupe_by_name<'a>(tools: &[&'a dyn Tool]) -> Vec<&'a dyn Tool> {
    let mut seen = std::collections::HashSet::new();
    tools.iter()
        .copied()
        .filter(|t| seen.insert(t.name().to_string()))
        .collect()
}
```

---

## 四、四个 impl

### 1. AnthropicToolAdapter

```rust
// crates/alva-llm-provider/src/provider/anthropic/tool_adapter.rs

use super::*;

pub struct AnthropicToolAdapter;

impl ToolAdapter for AnthropicToolAdapter {
    fn provider_name(&self) -> &'static str { "anthropic" }

    fn serialize_tools(&self, tools: &[&dyn Tool]) -> Value {
        let deduped = dedupe_by_name(tools);
        Value::Array(deduped.iter().map(|t| serde_json::json!({
            "name": t.name(),
            "description": t.description(),
            "input_schema": t.parameters_schema(),
            // 暂不加 eager_input_streaming —— 那是 Amp 自己 proxy 的 flag，
            // 直连 Anthropic 不认
        })).collect())
    }

    fn parse_tool_use(&self, raw: &Value) -> Result<ContentBlock, AgentError> {
        // Anthropic 原生格式就是 {type:"tool_use", id, name, input}，基本原样接收
        let id = raw.get("id").and_then(Value::as_str).unwrap_or_default();
        let name = raw.get("name").and_then(Value::as_str).unwrap_or_default();
        let input = raw.get("input").cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        Ok(ContentBlock::ToolUse {
            id: self.normalize_tool_use_id(Some(id)),
            name: name.into(),
            input,
        })
    }
}
```

### 2. OpenAIChatToolAdapter

```rust
pub struct OpenAIChatToolAdapter;

impl ToolAdapter for OpenAIChatToolAdapter {
    fn provider_name(&self) -> &'static str { "openai-chat" }

    fn serialize_tools(&self, tools: &[&dyn Tool]) -> Value {
        let deduped = dedupe_by_name(tools);
        Value::Array(deduped.iter().map(|t| {
            let schema = t.parameters_schema();
            let props = schema.get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let fixed_props = fix_property_types(&props);
            let required = schema.get("required").cloned()
                .unwrap_or_else(|| Value::Array(vec![]));

            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": {
                        "type": schema.get("type").cloned()
                            .unwrap_or_else(|| Value::String("object".into())),
                        "properties": fixed_props,
                        "required": required,
                        "additionalProperties": true,   // ← 关键：关掉严格模式
                    }
                }
            })
        }).collect())
    }

    fn serialize_tools_strict(&self, tools: &[&dyn Tool]) -> Value {
        // Structured output 用的严格版
        let deduped = dedupe_by_name(tools);
        Value::Array(deduped.iter().map(|t| {
            let schema = t.parameters_schema();
            let props = schema.get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let fixed_props = fix_property_types(&props);
            let required = schema.get("required").cloned()
                .unwrap_or_else(|| Value::Array(vec![]));

            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "strict": true,             // ← 启用严格 schema 校验
                    "parameters": {
                        "type": "object",
                        "properties": fixed_props,
                        "required": required,
                        "additionalProperties": false,
                    }
                }
            })
        }).collect())
    }

    fn parse_tool_use(&self, raw: &Value) -> Result<ContentBlock, AgentError> {
        // 典型结构: {id, type:"function", function:{name, arguments: "<json str>"}}
        let id = raw.get("id").and_then(Value::as_str);
        let func = raw.get("function");
        let name = func.and_then(|f| f.get("name")).and_then(Value::as_str)
            .unwrap_or_default();
        let args_str = func.and_then(|f| f.get("arguments")).and_then(Value::as_str);
        let input = args_str
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| Value::Object(Map::new()));

        Ok(ContentBlock::ToolUse {
            id: self.normalize_tool_use_id(id),
            name: name.into(),
            input,
        })
    }
}
```

### 3. OpenAIResponsesToolAdapter

```rust
pub struct OpenAIResponsesToolAdapter;

impl ToolAdapter for OpenAIResponsesToolAdapter {
    fn provider_name(&self) -> &'static str { "openai-responses" }

    fn serialize_tools(&self, tools: &[&dyn Tool]) -> Value {
        let deduped = dedupe_by_name(tools);
        Value::Array(deduped.iter().map(|t| {
            let schema = t.parameters_schema();
            let props = schema.get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let required = schema.get("required").cloned()
                .unwrap_or_else(|| Value::Array(vec![]));

            // Responses API 是扁平结构（不嵌套 function: {...}）
            serde_json::json!({
                "type": "function",
                "name": t.name(),
                "description": t.description(),
                "strict": false,
                "parameters": {
                    "type": schema.get("type").cloned()
                        .unwrap_or_else(|| Value::String("object".into())),
                    "properties": props,
                    "required": required,
                    "additionalProperties": true,
                }
            })
        }).collect())
    }

    fn parse_tool_use(&self, raw: &Value) -> Result<ContentBlock, AgentError> {
        // 典型结构: {type:"function_call", call_id, name, arguments: "<json str>"}
        let id = raw.get("call_id").and_then(Value::as_str);
        let name = raw.get("name").and_then(Value::as_str).unwrap_or_default();
        let args_str = raw.get("arguments").and_then(Value::as_str);
        let input = args_str
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| Value::Object(Map::new()));

        Ok(ContentBlock::ToolUse {
            id: self.normalize_tool_use_id(id),
            name: name.into(),
            input,
        })
    }
}
```

### 4. VertexAIGeminiToolAdapter（新）

```rust
pub struct VertexAIGeminiToolAdapter;

impl ToolAdapter for VertexAIGeminiToolAdapter {
    fn provider_name(&self) -> &'static str { "vertex-gemini" }

    fn serialize_tools(&self, tools: &[&dyn Tool]) -> Value {
        if tools.is_empty() { return Value::Array(vec![]); }
        let deduped = dedupe_by_name(tools);
        // Gemini 把所有 tools 塞在一个 tool 下的 functionDeclarations 数组
        let declarations: Vec<Value> = deduped.iter().map(|t| {
            serde_json::json!({
                "name": t.name(),
                "description": t.description(),
                "parameters": gemini_schema(&t.parameters_schema()),
            })
        }).collect();
        serde_json::json!([{ "functionDeclarations": declarations }])
    }

    fn parse_tool_use(&self, raw: &Value) -> Result<ContentBlock, AgentError> {
        // Gemini 格式: {functionCall: {name, args: {...}}}
        // ⚠ Gemini 不返回 tool_use id，我们必须自己生成
        let fc = raw.get("functionCall").ok_or_else(|| {
            AgentError::LlmError("not a functionCall block".into())
        })?;
        let name = fc.get("name").and_then(Value::as_str).unwrap_or_default();
        let input = fc.get("args").cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        Ok(ContentBlock::ToolUse {
            id: self.normalize_tool_use_id(None),  // ← 自动生成
            name: name.into(),
            input,
        })
    }
}

/// 把 JSON schema 递归改写成 Gemini 风格：
/// - type: "string" → "STRING"（Gemini 用大写 enum 字符串）
/// - examples: [v1, v2, ...] → example: v1（Gemini 只要一个）
/// - 递归 properties / items
fn gemini_schema(schema: &Value) -> Value {
    let Value::Object(obj) = schema else { return schema.clone() };
    let mut out = Map::new();

    if let Some(t) = obj.get("type").and_then(Value::as_str) {
        let gem_t = match t {
            "string"  => "STRING",
            "number"  => "NUMBER",
            "integer" => "INTEGER",
            "boolean" => "BOOLEAN",
            "object"  => "OBJECT",
            "array"   => "ARRAY",
            _         => "TYPE_UNSPECIFIED",
        };
        out.insert("type".into(), Value::String(gem_t.into()));
    }
    if let Some(d) = obj.get("description") { out.insert("description".into(), d.clone()); }
    if let Some(r) = obj.get("required")    { out.insert("required".into(), r.clone()); }
    if let Some(examples) = obj.get("examples").and_then(Value::as_array) {
        if let Some(first) = examples.first() {
            out.insert("example".into(), first.clone());
        }
    }
    if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        let mapped: Map<_, _> = props.iter()
            .map(|(k, v)| (k.clone(), gemini_schema(v)))
            .collect();
        out.insert("properties".into(), Value::Object(mapped));
    }
    if let Some(items) = obj.get("items") {
        out.insert("items".into(), gemini_schema(items));
    }
    Value::Object(out)
}
```

---

## 五、迁移方案（现有 3 个 provider 怎么改）

### Step 1：新 crate 结构

```
crates/alva-llm-provider/src/
├── lib.rs
├── tool_adapter/              ← 新
│   ├── mod.rs                 (trait + 工厂函数)
│   ├── anthropic.rs
│   ├── openai_chat.rs
│   ├── openai_responses.rs
│   ├── vertex_gemini.rs       ← 新
│   └── helpers.rs             (dedupe_by_name, fix_property_types, sanitize_id)
├── provider/
│   ├── anthropic.rs           ← 删 to_anthropic_tools，改调 AnthropicToolAdapter
│   ├── openai_chat.rs         ← 删 to_oai_tools
│   └── openai_responses.rs    ← 删 to_responses_tools
```

### Step 2：Provider 内部改调点

**anthropic.rs** 大致改动：

```diff
+ use crate::tool_adapter::{AnthropicToolAdapter, ToolAdapter};
+
+ pub struct AnthropicProvider {
+     adapter: AnthropicToolAdapter,
+     // ... 其他字段
+ }

- fn to_anthropic_tools(tools: &[&dyn Tool]) -> Vec<AnthropicToolDef> {
-     tools.iter().map(|t| AnthropicToolDef {
-         name: t.name().to_string(),
-         description: t.description().to_string(),
-         input_schema: t.parameters_schema(),
-     }).collect()
- }

// 调用处
- let tool_defs = to_anthropic_tools(&tools);
+ let tool_defs = self.adapter.serialize_tools(&tools);

// 从 response 拆 content block 时
match block.block_type.as_str() {
    "tool_use" => {
-       let id = block.id.unwrap_or_default();
-       let name = block.name.unwrap_or_default();
-       let input = block.input.unwrap_or(Value::Object(Map::new()));
-       content_blocks.push(ContentBlock::ToolUse { id, name, input });
+       // 序列化回 Value 再让 adapter 解（小损耗，但代码统一）
+       let raw = serde_json::to_value(&block)?;
+       content_blocks.push(self.adapter.parse_tool_use(&raw)?);
    }
}
```

**openai_chat.rs** / **openai_responses.rs** 类似。

> **注意**：`parse_tool_use` 参数是 `Value` 而非 typed struct —— 是为了让
> adapter 保持 provider-agnostic。如果担心 serde_json::to_value 的性能损耗，
> 可以给 adapter 加 typed 入参的 specialized 方法（后续优化）。

### Step 3：工厂函数（便利）

```rust
// crates/alva-llm-provider/src/tool_adapter/mod.rs

pub fn adapter_for(provider: ProviderKind) -> Box<dyn ToolAdapter> {
    match provider {
        ProviderKind::Anthropic       => Box::new(AnthropicToolAdapter),
        ProviderKind::OpenAIChat      => Box::new(OpenAIChatToolAdapter),
        ProviderKind::OpenAIResponses => Box::new(OpenAIResponsesToolAdapter),
        ProviderKind::VertexGemini    => Box::new(VertexAIGeminiToolAdapter),
    }
}
```

---

## 六、验证（Test Plan）

### 单元测试

```rust
// crates/alva-llm-provider/src/tool_adapter/tests.rs

#[test]
fn anthropic_serialize_preserves_schema_shape() { /* ... */ }

#[test]
fn openai_chat_fixes_missing_type_in_properties() {
    // 输入: { "props": { "items": [...] } }  (缺 type)
    // 输出: { "props": { "items": [...], "type": "array" } }
}

#[test]
fn openai_chat_adds_additional_properties_true() { /* ... */ }

#[test]
fn gemini_recursively_rewrites_schema() {
    // 嵌套 properties/items 都递归
    // examples 数组变 example 单值
    // type: "string" → "STRING"
}

#[test]
fn gemini_parse_tool_use_generates_id() {
    // Gemini 不返回 id，adapter 必须自己造
    // 结果 id 以 "toolu_" 开头
}

#[test]
fn openai_chat_parse_tool_use_handles_empty_arguments() {
    // arguments 是 "" 或 "{}"，都不应 panic，返回空 input
}

#[test]
fn dedupe_removes_duplicate_names() {
    // 两个同名 Tool 只序列化一次
}

#[test]
fn structured_output_variant_uses_strict_true() {
    // serialize_tools_strict 的输出里 strict:true, additionalProperties:false
}
```

### 集成测试

`crates/alva-llm-provider/tests/adapter_integration.rs`：

1. **跨 provider 往返**：构造一个 Tool，通过每个 adapter 序列化后，再用一个
   mock provider 响应（typed JSON）走 `parse_tool_use`，期待得到同样的
   `ContentBlock::ToolUse`。
2. **schema 兼容性**：准备一个 zod-generated 风格的 schema（含 `examples`、
   缺 `type` 的 property、嵌套 array），每个 adapter 都不 panic、输出都能通过
   各家 API 的 JSON schema 校验（离线 validator）。

---

## 七、非目标（不在这次做）

- ❌ **Streaming tool_use partial JSON 合并**：各家差异大（Anthropic input 逐 delta 增、OpenAI Chat arguments 分片、Gemini 不 stream function call）。先做完整响应级别，stream 级别后续再抽。
- ❌ **Eager input streaming**：Amp 给 Anthropic 加的 `eager_input_streaming: true` 是他们 proxy 专用 flag，官方 API 不认，直连不能加。
- ❌ **Cache control**：`{"cache_control": {"type": "ephemeral"}}` 是 Anthropic prompt cache 的机制，归 prompt 装配管，不在 tool adapter 范围。

---

## 八、里程碑

1. **Milestone 1**（0.5 天）：新建 `tool_adapter/` 模块 + trait + helpers + Anthropic impl + 单测
2. **Milestone 2**（0.5 天）：OpenAI Chat + OpenAI Responses impl + 单测
3. **Milestone 3**（1 天）：迁移 3 个现有 provider 到 adapter 调用，删除老 `to_*_tools` free functions，集成测试通过
4. **Milestone 4**（0.5 天）：Vertex Gemini adapter 骨架（不接真 API，只跑单测）
5. **Milestone 5**（按需）：后续新增 Cohere / Mistral / Bedrock adapter 走同一 trait

总计约 2.5 天工作量。

---

## 九、潜在坑

1. **ContentBlock::ToolUse 字段稳定性**：目前是 `{id, name, input}`。如果未来要加 `is_partial` 等字段（支持 streaming），trait 的 `parse_tool_use` 签名要改成返回 `Vec<ContentBlock>` 或自定义 struct。先定好。

2. **Tool use ID 前缀污染下游 logs**：内部一律 `toolu_xxx`。如果某个 extension 直接读 id 做人类可读展示（如审计日志），要确保 strip 前缀后才对人显示。

3. **Gemini 空 tool 列表**：不能返回 `[]`，必须返回 `[]`（即无 `tools` 字段）或完全省略 `tools`。目前设计返回 `Value::Array(vec![])` 调用方要判空跳过。

4. **Responses API 的 structured output**：如果未来 Alva 用 `handoff` 这类结构化输出，要让调用方区分 `serialize_tools` vs `serialize_tools_strict`。可以由 `LanguageModel` 层根据 request 里是否带 `response_format: json_schema` 自动选。

5. **YLR 在嵌套 property 里**：Amp 的 `YLR` 只对 top-level properties 修复。Alva 实现时要决定要不要递归（嵌套 object 的 property 也可能缺 type）。建议**只 top-level**，和 Amp 一致，避免 overreach。

---

## 十、和其他 Alva learnings 的关系

- 配合 [`../models/inference-retry.md`](../models/inference-retry.md) 的 `StreamError` enum —— adapter 返回的错误要归到 `StreamError::InvalidToolCall` 之类。
- 配合 [`../mcp/tool-filtering.md`](../mcp/tool-filtering.md) 的 `includeTools` glob —— MCP adapter 本身也生成 tool spec，经过 `ToolAdapter` 再发给 LLM。两层互不冲突。
- 不影响 [`./resource-lock-scheduler.md`](./resource-lock-scheduler.md) —— 调度器在 tool adapter 之上，关心的是 ToolUse 的 resource keys，不关心序列化格式。
