# alva-llm-wire + 协议网关 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 LLM 线格式转换层抽成独立 crate `alva-llm-wire`（只依赖 serde+uuid+chrono、复用 `Message`、与可执行 `Tool` 解耦），并在其上建协议网关 `alva-app-gateway`，对外暴露 Responses/Chat/Anthropic 三种入站协议，按 model 别名路由翻译到任意上游。

**Architecture:** 现有 `ToolAdapter`（出站编解码）改名 `ProtocolAdapter` 并补对称的入站三方法；类型下沉到新 crate，kernel-abi re-export 保持兼容；网关复用 `LanguageModel`/`AliasRouter` 做上游+路由，只新写入站编解码 + HTTP 壳。

**Tech Stack:** Rust workspace（30 crates）、serde/serde_json、axum/tokio、async-trait、uuid/chrono。

**Spec:** `docs/superpowers/specs/2026-05-22-alva-llm-wire-gateway-design.md`（v2.1）。
**Branch:** `feat/llm-wire-gateway`（已存在）。

---

## 全局约定

- 每个 Task 结束都 `cargo build` + 相关 `cargo test` 绿，再 commit。
- 验证命令统一用：`cargo test -p <crate> <test_name> -- --nocolor`。
- 整体回归：`cargo check --workspace` 必须绿；wasm 门禁 `cargo check -p alva-llm-wire --target wasm32-unknown-unknown` 必须绿。
- Commit 前缀遵守 AGENTS.md：`feat:` / `refactor:` / `test:` / `chore:`。

## 文件结构地图（先看清谁动谁）

| 文件 | 动作 | 职责 |
|------|------|------|
| `crates/alva-kernel-abi/src/tool/execution.rs` | 拆 | 把 `ToolContent`/`ToolOutput`（serde-clean）移出，留 `ToolExecutionContext`（bus 耦合） |
| `crates/alva-kernel-abi/src/tool/content_payload.rs` | 新建 | 接收 `ToolContent`/`ToolOutput` |
| `crates/alva-kernel-abi/src/model/mod.rs` | 拆 | 把 `ModelConfig`/`ReasoningEffort` 移出，留 `LanguageModel`/`TokenCounter` |
| `crates/alva-kernel-abi/src/model/config.rs` | 新建 | 接收 `ModelConfig`/`ReasoningEffort` |
| `crates/alva-kernel-abi/src/base/stream.rs` | 改 | 加 `StreamEvent::Stop{reason}` + `StopReason` |
| `crates/alva-kernel-core/src/run.rs` | 改 | match `StreamEvent` 加 `Stop` 臂（369-454） |
| `crates/alva-kernel-abi/src/adapter/*.rs` | 移出+改 | 整个 adapter 模块迁入 wire crate；4 个 `decode_stream_event` emit `Stop` |
| `crates/alva-llm-wire/` | 新建 crate | 下沉类型 + `ProtocolAdapter` + 4 adapter + 入站方法 |
| `crates/alva-kernel-abi/src/lib.rs` + `base/*` + `model/*` + `tool/*` | 改 | re-export wire 类型（扁平 + 模块两路径） |
| `crates/alva-llm-provider/src/provider/*.rs` | 改 | `encode_tools` 调用点（8 处）传 `ToolDefinition` |
| `crates/alva-llm-provider/src/registry.rs` | 改 | 加 `AliasRouter` |
| `crates/alva-app-gateway/` | 新建 crate | HTTP 壳 + `GatewayConfig` + `RawTool` + 串线 |

---

## Phase 0 — 类型隔离（前置必做，纯重排，零行为变化）

### Task 0.1: 拆出 `ToolContent` / `ToolOutput` 到 serde-clean 模块

**Files:**
- Create: `crates/alva-kernel-abi/src/tool/content_payload.rs`
- Modify: `crates/alva-kernel-abi/src/tool/execution.rs`（移走 `ToolContent`/`ToolOutput`/`ProgressEvent` 及其 impl，保留 `ToolExecutionContext`/`MinimalExecutionContext`）
- Modify: `crates/alva-kernel-abi/src/tool/mod.rs`（加 `pub mod content_payload;` + re-export）

- [ ] **Step 1: 建新模块，移入纯 serde 类型**

把 `execution.rs` 中 `ProgressEvent`(23-)、`ToolContent`(48-92)、`ToolOutput`(104-) 整段剪切到 `content_payload.rs`，文件头加：
```rust
// INPUT:  serde, serde_json
// OUTPUT: ToolContent, ToolOutput, ProgressEvent
// POS:    Pure-serde tool payload types, split out of execution.rs so they carry
//         zero bus/runtime coupling and can be re-exported by alva-llm-wire.
use serde::{Deserialize, Serialize};
use serde_json::Value;
```
（`ToolContent`/`ToolOutput` 不依赖 bus，剪切后补齐它们用到的 import。）

- [ ] **Step 2: execution.rs 重新引用**

`execution.rs` 顶部加 `use super::content_payload::{ToolContent, ToolOutput, ProgressEvent};`，删掉已移走类型的定义。`ToolExecutionContext`（依赖 `alva_kernel_bus::BusHandle`）留在原地。

- [ ] **Step 3: tool/mod.rs re-export（保持旧路径）**

`tool/mod.rs` 加：
```rust
pub mod content_payload;
pub use content_payload::{ProgressEvent, ToolContent, ToolOutput};
```
确认 `content.rs:35` 的 `crate::tool::execution::ToolContent` 仍能解析——若 `execution.rs` 已 `pub use super::content_payload::*` 则旧路径不断；否则把 `content.rs` 改用 `crate::tool::content_payload::ToolContent`（同 crate 内，单文件改动）。

- [ ] **Step 4: 编译 + 全测**

Run: `cargo test -p alva-kernel-abi -- --nocolor`
Expected: PASS（纯搬运，所有既有测试不变）。

- [ ] **Step 5: 全工作区 check**

Run: `cargo check --workspace`
Expected: 绿。`ToolContent`/`ToolOutput` 的 ~25/~40 个导入者多走扁平 `alva_kernel_abi::ToolContent`（`lib.rs:45`），不受影响。

- [ ] **Step 6: Commit**
```bash
git add crates/alva-kernel-abi/src/tool/
git commit -m "refactor(kernel-abi): split ToolContent/ToolOutput into content_payload module

为后续把 ContentBlock 下沉到 alva-llm-wire 解耦：把纯 serde 的 payload 类型
从 bus 耦合的 execution.rs 拆出。re-export 保持旧路径不变。"
```

### Task 0.2: 拆出 `ModelConfig` / `ReasoningEffort` 到独立模块

**Files:**
- Create: `crates/alva-kernel-abi/src/model/config.rs`
- Modify: `crates/alva-kernel-abi/src/model/mod.rs`

- [ ] **Step 1: 建 config.rs，移入值类型**

把 `model/mod.rs` 的 `ModelConfig`(13-52)、`ReasoningEffort`(64-126) 及其 `impl` + 相关测试剪切进 `config.rs`：
```rust
// INPUT:  serde, serde_json
// OUTPUT: ModelConfig, ReasoningEffort
// POS:    Pure-serde request-config value types, split out of model/mod.rs so
//         alva-llm-wire can own them without pulling LanguageModel/bus_cap.
use serde::{Deserialize, Serialize};
```

- [ ] **Step 2: model/mod.rs re-export（双路径关键）**

`model/mod.rs` 加：
```rust
mod config;
pub use config::{ModelConfig, ReasoningEffort};
```
**验收点**：`alva_kernel_abi::model::ModelConfig` 必须仍可解析——5 个模块路径导入者依赖它（`app-tauri/src/provider_api.rs:13`、`host-wasm/src/{agent.rs:27,smoke.rs:28,stub.rs:21}`、`app-core/tests/scope_integration.rs:14`）。

- [ ] **Step 3: 编译 + 测**

Run: `cargo test -p alva-kernel-abi model::config -- --nocolor`
Expected: PASS（原 model 测试随类型迁移）。

- [ ] **Step 4: 双路径回归**

Run: `cargo check -p alva-app-tauri && cargo check -p alva-host-wasm --target wasm32-unknown-unknown`
Expected: 绿（验证模块路径 re-export 生效）。

- [ ] **Step 5: Commit**
```bash
git add crates/alva-kernel-abi/src/model/
git commit -m "refactor(kernel-abi): split ModelConfig/ReasoningEffort into model::config

为下沉做准备；model/mod.rs re-export 保旧路径（扁平 + alva_kernel_abi::model::）。"
```

---

## Phase 1 — `StreamEvent::Stop` 终止原因（出站增强，无回归）

### Task 1.1: 加 `StopReason` + `StreamEvent::Stop` 变体

**Files:**
- Modify: `crates/alva-kernel-abi/src/base/stream.rs`

- [ ] **Step 1: 写失败测试**

在 `stream.rs` 的 `#[cfg(test)] mod tests` 加：
```rust
#[test]
fn stop_serializes_with_reason() {
    let v = roundtrip(&StreamEvent::Stop { reason: StopReason::MaxTokens });
    assert_eq!(v, json!({ "Stop": { "reason": "max_tokens" } }));
}
#[test]
fn stop_reason_other_carries_string() {
    let v = roundtrip(&StreamEvent::Stop { reason: StopReason::Other("refusal".into()) });
    assert_eq!(v, json!({ "Stop": { "reason": { "other": "refusal" } } }));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p alva-kernel-abi stop_serializes_with_reason -- --nocolor`
Expected: FAIL（`StopReason` / `Stop` 未定义）。

- [ ] **Step 3: 实现**

`stream.rs` 加：
```rust
/// Cross-protocol terminal reason. Maps to Chat finish_reason /
/// Anthropic stop_reason / Responses status (see spec §7 table).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Other(String),
}
```
在 `enum StreamEvent` 的 `Done` 之前加变体：
```rust
    /// Terminal reason, emitted right before `Done`. Carries why generation
    /// stopped so a gateway can reconstruct the inbound protocol's terminal frame.
    Stop { reason: StopReason },
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test -p alva-kernel-abi stop_ -- --nocolor`
Expected: PASS。

- [ ] **Step 5: Commit**
```bash
git add crates/alva-kernel-abi/src/base/stream.rs
git commit -m "feat(kernel-abi): add StreamEvent::Stop{reason} + StopReason

现 StreamEvent 无终止原因，decode_stream_event 读到 finish_reason 即丢弃。
网关重建入站终止帧需要它。Stop 在 Done 之前发。"
```

### Task 1.2: `run.rs` 穷尽 match 补 `Stop` 臂

**Files:**
- Modify: `crates/alva-kernel-core/src/run.rs`（唯一无 wildcard 的 `StreamEvent` match，约 369-454）

- [ ] **Step 1: 编译暴露缺臂**

Run: `cargo build -p alva-kernel-core`
Expected: FAIL `non-exhaustive patterns: StreamEvent::Stop not covered`。

- [ ] **Step 2: 把 `Stop` 并入现有边界组**

定位 `run.rs` 末臂（现 `StreamEvent::Start | StreamEvent::Done | StreamEvent::ReasoningDelta | StreamEvent::ToolCallEnd { .. } => { /* no-op */ }`），改为：
```rust
            StreamEvent::Start
            | StreamEvent::Done
            | StreamEvent::Stop { .. }   // agent loop 不消费 reason（Message 无 stop_reason 字段，YAGNI）
            | StreamEvent::ReasoningDelta { .. }
            | StreamEvent::ToolCallEnd { .. } => {}
```

- [ ] **Step 3: 编译 + 测**

Run: `cargo test -p alva-kernel-core -- --nocolor`
Expected: PASS。

- [ ] **Step 4: 全工作区扫剩余 match**

Run: `cargo check --workspace`
Expected: 绿。其余消费者（`app-cli/ui/app.rs:1219`、`e2e_http_test.rs`）均有 `_ => {}`，不报错。

- [ ] **Step 5: Commit**
```bash
git add crates/alva-kernel-core/src/run.rs
git commit -m "refactor(kernel-core): handle StreamEvent::Stop in run loop (no-op boundary)"
```

### Task 1.3: 四个 `decode_stream_event` 发 `Stop`（先做 Chat，余下随各自 adapter）

> 此处只改 openai_chat 作为范式；anthropic/openai_responses/gemini 在本 task 同样处理（各自 finish/stop 字段见 §7）。

**Files:**
- Modify: `crates/alva-kernel-abi/src/adapter/openai_chat.rs`（finish_reason 分支 277-287）
- Modify: `adapter/anthropic.rs`、`adapter/openai_responses.rs`（gemini 无显式 stop 时映射 EndTurn）

- [ ] **Step 1: 写失败测试（Chat）**

把现有 `decode_stream_accumulates_tool_call_partials` 的第三块断言扩展，新增：
```rust
#[test]
fn decode_stream_emits_stop_then_tool_end_on_finish() {
    let mut state = StreamDecodeState::new();
    // 先建一个 open tool call
    let c1 = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_x","function":{"name":"r","arguments":"{}"}}]}}]});
    let _ = OpenAIChatAdapter.decode_stream_event(&c1, &mut state).unwrap();
    let c3 = serde_json::json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]});
    let out = OpenAIChatAdapter.decode_stream_event(&c3, &mut state).unwrap();
    // 顺序：Stop{ToolUse} 在 ToolCallEnd 之后、整体在 Done 之前（Done 由 provider 壳发）
    assert!(out.iter().any(|e| matches!(e, StreamEvent::Stop { reason } if *reason == StopReason::ToolUse)));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p alva-kernel-abi decode_stream_emits_stop -- --nocolor` → FAIL。

- [ ] **Step 3: 实现 finish_reason → StopReason 映射**

在 `openai_chat.rs` finish_reason 分支（281）末尾，clear 之后加：
```rust
                    let reason = match finish_reason {
                        "tool_calls" => StopReason::ToolUse,
                        "length" => StopReason::MaxTokens,
                        "stop" => StopReason::EndTurn,
                        other => StopReason::Other(other.to_string()),
                    };
                    out.push(StreamEvent::Stop { reason });
```
（import 加 `use crate::base::stream::StopReason;`。）anthropic：`message_delta.delta.stop_reason` `end_turn|tool_use|max_tokens|stop_sequence` → 对应；openai_responses：`response.completed`→EndTurn、`response.incomplete`→MaxTokens、有 function_call output→ToolUse。

- [ ] **Step 4: 运行确认通过 + 全测**

Run: `cargo test -p alva-kernel-abi -- --nocolor`
Expected: PASS（注意更新 anthropic/responses 既有 stream 测试，使其容纳新增 `Stop` 帧）。

- [ ] **Step 5: Commit**
```bash
git add crates/alva-kernel-abi/src/adapter/
git commit -m "feat(kernel-abi): adapters emit StreamEvent::Stop mapped from finish/stop_reason"
```

---

## Phase 2 — 抽 `alva-llm-wire`（下沉 + 改名 + re-export）

### Task 2.1: 建 crate 骨架

**Files:**
- Create: `crates/alva-llm-wire/Cargo.toml`、`crates/alva-llm-wire/src/lib.rs`
- Modify: 根 `Cargo.toml`（workspace members 加 `crates/alva-llm-wire`）

- [ ] **Step 1: Cargo.toml**
```toml
[package]
name = "alva-llm-wire"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```
（若搬运中发现某类型 `#[derive(JsonSchema)]` 再加 `schemars`。）

- [ ] **Step 2: 空 lib + 根 workspace 注册**

`lib.rs`：`#![doc = "LLM wire-format conversion: protocol-neutral types + ProtocolAdapter."]`
根 `Cargo.toml` 的 `members` 加 `"crates/alva-llm-wire"`。

- [ ] **Step 3: 编译**

Run: `cargo build -p alva-llm-wire` → 绿（空 crate）。

- [ ] **Step 4: Commit**
```bash
git add Cargo.toml crates/alva-llm-wire/
git commit -m "chore: scaffold alva-llm-wire crate"
```

### Task 2.2: 下沉类型 + adapter 模块，kernel-abi 改 re-export

**Files:**
- Move into `crates/alva-llm-wire/src/`: `base/{content,message,stream}.rs`、`model/config.rs`（→`config.rs`）、`tool/content_payload.rs`（→`tool_payload.rs`）、`tool` 中的 `ToolDefinition`（→`tool_def.rs`）、整个 `adapter/` 目录
- Modify: `crates/alva-kernel-abi/Cargo.toml`（加 `alva-llm-wire = { path = "../alva-llm-wire" }`）
- Modify: `crates/alva-kernel-abi/src/{lib.rs, base/mod.rs, model/mod.rs, tool/mod.rs}`（改为 re-export）

- [ ] **Step 1: 物理迁移 + wire lib.rs 导出**

把上述文件移入 wire crate，`alva-llm-wire/src/lib.rs`：
```rust
pub mod content;
pub mod message;
pub mod stream;
pub mod config;        // ModelConfig / ReasoningEffort
pub mod tool_payload;  // ToolContent / ToolOutput / ProgressEvent
pub mod tool_def;      // ToolDefinition
pub mod adapter;       // ProtocolAdapter + 4 impls（见 Phase 3 改名）

pub use content::ContentBlock;
pub use message::{Message, MessageRole, UsageMetadata};
pub use stream::{StreamEvent, StopReason};
pub use config::{ModelConfig, ReasoningEffort};
pub use tool_payload::{ProgressEvent, ToolContent, ToolOutput};
pub use tool_def::ToolDefinition;
```
adapter 内 `use crate::base::...` 改成 `use crate::{...}`（wire crate 内路径）。`ToolDefinition` 从 `tool/types.rs` 抽出（只搬这一个 struct，`Tool` trait 留 kernel-abi）。

- [ ] **Step 2: kernel-abi re-export（双路径，零下游改动）**

`alva-kernel-abi/src/base/content.rs` → 整文件替换为 `pub use alva_llm_wire::content::*;`（message/stream 同理）。
`base/mod.rs` 保留 `pub mod content; pub mod message; pub mod stream;`（现在它们只是 re-export shim）。
`model/mod.rs` 保留 `pub use alva_llm_wire::config::{ModelConfig, ReasoningEffort};`（模块路径 `alva_kernel_abi::model::ModelConfig` 仍解析）。
`tool/mod.rs`：`pub use alva_llm_wire::{ToolDefinition, ProgressEvent, ToolContent, ToolOutput};`。
`lib.rs` 顶层扁平 re-export 同步：`pub use alva_llm_wire::{Message, ContentBlock, StreamEvent, ModelConfig, ToolDefinition, ...};`（对照现有 `lib.rs` 扁平导出清单逐一保留）。
`adapter`：`alva-kernel-abi/src/adapter` 整目录删除，`lib.rs` 加 `pub use alva_llm_wire::adapter;` + 兼容别名 `pub use alva_llm_wire::adapter::ProtocolAdapter as ToolAdapter;`（Phase 3 改名后）。

- [ ] **Step 3: wire 自测 + kernel-abi 编译**

Run: `cargo test -p alva-llm-wire -- --nocolor`（迁移过来的所有单测随之绿）
Run: `cargo build -p alva-kernel-abi`
Expected: 绿。

- [ ] **Step 4: 全工作区回归**

Run: `cargo check --workspace`
Expected: 绿（所有 `use alva_kernel_abi::{base::message::Message, adapter::ToolAdapter, ...}` 经 re-export 不变）。

- [ ] **Step 5: wasm 门禁**

Run: `cargo check -p alva-llm-wire --target wasm32-unknown-unknown`
Expected: 绿（serde+uuid+chrono 均 wasm-safe）。把 `alva-llm-wire` 加入 `scripts/ci-check-deps.sh` 的 wasm-clean 名单与"无 app/host 依赖"规则。

- [ ] **Step 6: Commit**
```bash
git add -A
git commit -m "refactor: extract alva-llm-wire crate (down-sink wire types + adapter)

content/message/stream/config/tool_payload/tool_def + adapter 迁入新 crate；
kernel-abi 改 re-export 保旧路径（扁平 + 模块）。纯搬运，全工作区零行为变化。"
```

---

## Phase 3 — `encode_tools` 解耦 `Tool` trait

### Task 3.1: trait 改名 + `encode_tools` 签名换 `ToolDefinition`

**Files:**
- Modify: `crates/alva-llm-wire/src/adapter/mod.rs`（trait 定义，现 `ToolAdapter`→`ProtocolAdapter`，`encode_tools(&[&dyn Tool])`→`&[ToolDefinition]`）
- Modify: 4 个 adapter impl 的 `encode_tools`（用 `td.name`/`td.description`/`td.parameters` 取代 `t.name()`/...）
- Modify: 各 adapter in-file 测试（构造 `ToolDefinition` 取代 `&dyn Tool`/MockTool）

- [ ] **Step 1: 改 trait**

`adapter/mod.rs`：`pub trait ToolAdapter` → `pub trait ProtocolAdapter`；方法 `fn provider`→保留或改 `fn protocol`（保留 `provider` 名以缩小改动，仅 rename trait）；`fn encode_tools(&self, tools: &[ToolDefinition]) -> Vec<Value>;`。删除 `use crate::tool::Tool`（wire crate 无此类型），加 `use crate::tool_def::ToolDefinition;`。

- [ ] **Step 2: 改 4 个 impl 的 encode_tools**

以 `openai_chat.rs` 为例（其余同构，字段名一致）：
```rust
    fn encode_tools(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        let mut seen = std::collections::HashSet::new();
        tools.iter()
            .filter(|t| seen.insert(t.name.clone()))
            .map(|t| {
                let mut params = t.parameters.clone();
                schema_fix::fill_missing_types(&mut params);
                schema_fix::force_additional_properties(&mut params, true);
                serde_json::json!({"type":"function","function":{
                    "name": t.name, "description": t.description, "parameters": params }})
            }).collect()
    }
```
（anthropic / gemini / openai_responses 各按其原结构把 `t.name()`→`t.name` 等。）

- [ ] **Step 3: 改 in-file 测试**

各 adapter 测试里 `let tools: Vec<&dyn Tool> = vec![&t];` → `let tools = vec![ToolDefinition{name:"read".into(), description:String::new(), parameters: json!({...})}];`，调用 `encode_tools(&tools)`。删 MockTool（若仅测试用）。

- [ ] **Step 4: kernel-abi 兼容别名**

`alva-kernel-abi/src/lib.rs`：`#[deprecated(note="renamed to ProtocolAdapter")] pub use alva_llm_wire::adapter::ProtocolAdapter as ToolAdapter;`。

- [ ] **Step 5: 测**

Run: `cargo test -p alva-llm-wire -- --nocolor` → PASS。

- [ ] **Step 6: Commit**
```bash
git add crates/alva-llm-wire/
git commit -m "refactor(llm-wire): rename ToolAdapter→ProtocolAdapter, encode_tools takes ToolDefinition

与可执行 Tool trait 解耦：encode_tools 只需 name/desc/parameters。"
```

### Task 3.2: 8 个 provider 调用点适配

**Files:**
- Modify: `crates/alva-llm-provider/src/provider/{anthropic.rs:201,302, openai_responses.rs:117,191, openai_chat.rs:181,262, gemini.rs:181,255}`

- [ ] **Step 1: 改调用点**

每处 `adapter.encode_tools(tools)`（`tools: &[&dyn Tool]`）→：
```rust
let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| t.definition()).collect();
let api_tools = adapter.encode_tools(&tool_defs);
```
import 加 `use alva_kernel_abi::ToolDefinition;`（`Tool::definition()` 已存在，`tool/types.rs`）。

- [ ] **Step 2: 编译 + 测**

Run: `cargo test -p alva-llm-provider -- --nocolor` → PASS。

- [ ] **Step 3: 全工作区回归**

Run: `cargo check --workspace` → 绿。

- [ ] **Step 4: Commit**
```bash
git add crates/alva-llm-provider/
git commit -m "refactor(llm-provider): pass ToolDefinition to encode_tools (8 call sites)"
```

---

## Phase 4 — 入站 trait 方法 + Responses 入站（worked reference，全量编码）

### Task 4.1: 给 `ProtocolAdapter` 加入站三方法（默认 Unsupported）+ 辅助类型

**Files:**
- Modify: `crates/alva-llm-wire/src/adapter/mod.rs`

- [ ] **Step 1: 写失败测试**

`adapter/mod.rs` 测试加：
```rust
#[test]
fn default_inbound_is_unsupported() {
    let a = crate::adapter::gemini::GeminiAdapter::new();
    assert!(matches!(a.decode_request(&serde_json::json!({})), Err(AdapterError::InboundUnsupported(_))));
}
```

- [ ] **Step 2: 运行确认失败** → `cargo test -p alva-llm-wire default_inbound_is_unsupported` → FAIL。

- [ ] **Step 3: 实现辅助类型 + trait 默认方法**
```rust
use crate::config::ModelConfig;
use crate::tool_def::ToolDefinition;
use crate::message::Message;

pub struct DecodedRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub config: ModelConfig,
    pub stream: bool,
}
pub struct SseFrame { pub event: Option<String>, pub data: Value }
#[derive(Default)]
pub struct StreamEncodeState {
    pub response_id: String,
    pub seq: i64,
    pub output_index: usize,
    pub started: bool,
}
```
`AdapterError` 加 `InboundUnsupported(&'static str)`（+ Display 臂）。trait 加三默认方法：
```rust
    fn decode_request(&self, _body: &Value) -> Result<DecodedRequest, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.provider()))
    }
    fn encode_response(&self, _resp: &DecodedResponse) -> Result<Value, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.provider()))
    }
    fn encode_stream_event(&self, _ev: &StreamEvent, _st: &mut StreamEncodeState)
        -> Result<Vec<SseFrame>, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.provider()))
    }
```

- [ ] **Step 4: 通过** → `cargo test -p alva-llm-wire default_inbound_is_unsupported` → PASS。

- [ ] **Step 5: Commit**
```bash
git add crates/alva-llm-wire/src/adapter/mod.rs
git commit -m "feat(llm-wire): add inbound ProtocolAdapter methods (default Unsupported) + DecodedRequest/SseFrame"
```

### Task 4.2: Responses `decode_request`

**Files:** Modify `crates/alva-llm-wire/src/adapter/openai_responses.rs`

- [ ] **Step 1: 写失败测试**
```rust
#[test]
fn responses_decode_request_basic() {
    let body = serde_json::json!({
        "model": "gpt-x",
        "instructions": "be brief",
        "input": [{"role":"user","content":[{"type":"input_text","text":"hi"}]}],
        "tools": [{"type":"function","name":"read","description":"d","parameters":{"type":"object"}}],
        "stream": true,
        "reasoning": {"effort":"high"}
    });
    let r = OpenAIResponsesAdapter::new().decode_request(&body).unwrap();
    assert_eq!(r.model, "gpt-x");
    assert!(r.stream);
    assert_eq!(r.tools[0].name, "read");
    assert_eq!(r.config.reasoning_effort, Some(ReasoningEffort::High));
    // system 进 messages 头部（System 角色）
    assert!(matches!(r.messages[0].role, MessageRole::System));
    assert!(r.messages.iter().any(|m| matches!(m.role, MessageRole::User)));
}
#[test]
fn responses_decode_request_rejects_image() {
    let body = serde_json::json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","image_url":"x"}]}]});
    assert!(matches!(OpenAIResponsesAdapter::new().decode_request(&body), Err(AdapterError::UnexpectedFormat(_))));
}
```

- [ ] **Step 2: 运行确认失败** → FAIL。

- [ ] **Step 3: 实现**（镜像本文件 `encode_messages`/`encode_tools` 的反向；`instructions` → 头部 `Message::system`；`input[].content[].type` 中 `input_text`→Text、`input_image`→报 `UnexpectedFormat("image not supported")`；`tools[]` 的 `{name,description,parameters}` → `ToolDefinition`；`reasoning.effort` → `ReasoningEffort::parse`；`max_output_tokens`→`config.max_tokens`，`temperature`/`top_p` 同名）。

- [ ] **Step 4: 通过 + 测** → `cargo test -p alva-llm-wire responses_decode_request -- --nocolor` → PASS。

- [ ] **Step 5: Commit** `git commit -m "feat(llm-wire): Responses decode_request (+image reject)"`

### Task 4.3: Responses `encode_response`（非流式）

**Files:** Modify `openai_responses.rs`

- [ ] **Step 1: 写失败测试**
```rust
#[test]
fn responses_encode_response_text_and_tool() {
    let dr = DecodedResponse {
        message: Message { id:"r1".into(), role: MessageRole::Assistant,
            content: vec![ContentBlock::Text{text:"hello".into()},
                          ContentBlock::ToolUse{id:"toolu_a".into(), name:"read".into(), input: serde_json::json!({"p":"/x"})}],
            tool_call_id: None, usage: Some(UsageMetadata{input_tokens:3,output_tokens:4,total_tokens:7,cache_creation_input_tokens:None,cache_read_input_tokens:None}), timestamp:0 },
        usage: None,
    };
    let v = OpenAIResponsesAdapter::new().encode_response(&dr).unwrap();
    assert_eq!(v["object"], "response");
    assert_eq!(v["status"], "completed");
    // output 数组含 message + function_call item
    let outs = v["output"].as_array().unwrap();
    assert!(outs.iter().any(|o| o["type"]=="function_call" && o["name"]=="read"));
    assert_eq!(v["usage"]["input_tokens"], 3);
}
```

- [ ] **Step 2: 失败** → FAIL。

- [ ] **Step 3: 实现**（构造 Responses 响应体：`{id, object:"response", status:"completed", model, output:[...], usage:{input_tokens,output_tokens,total_tokens}}`；`ContentBlock::Text`→`{type:"message", role:"assistant", content:[{type:"output_text", text}]}`；`ContentBlock::ToolUse`→`{type:"function_call", call_id: to_provider(id), name, arguments: input.to_string()}`；`usage` 取 `message.usage`）。

- [ ] **Step 4: 通过** → PASS。
- [ ] **Step 5: Commit** `git commit -m "feat(llm-wire): Responses encode_response"`

### Task 4.4: Responses `encode_stream_event`（流式 SSE）

**Files:** Modify `openai_responses.rs`

- [ ] **Step 1: 写失败测试（含顺序 + 终止）**
```rust
#[test]
fn responses_encode_stream_text_then_stop() {
    let a = OpenAIResponsesAdapter::new();
    let mut st = StreamEncodeState::default();
    let mut frames = vec![];
    for ev in [StreamEvent::Start,
               StreamEvent::TextDelta{text:"hi".into()},
               StreamEvent::Usage(UsageMetadata{input_tokens:1,output_tokens:1,total_tokens:2,cache_creation_input_tokens:None,cache_read_input_tokens:None}),
               StreamEvent::Stop{reason:StopReason::EndTurn},
               StreamEvent::Done] {
        frames.extend(a.encode_stream_event(&ev, &mut st).unwrap());
    }
    let names: Vec<_> = frames.iter().filter_map(|f| f.event.as_deref()).collect();
    assert!(names.contains(&"response.created"));
    assert!(names.contains(&"response.output_text.delta"));
    assert!(names.contains(&"response.completed"));
    // sequence_number 单调
    let seqs: Vec<i64> = frames.iter().filter_map(|f| f.data.get("sequence_number").and_then(|v| v.as_i64())).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]));
}
```

- [ ] **Step 2: 失败** → FAIL。

- [ ] **Step 3: 实现**（`Start`→发 `response.created`（生成 `response_id`，`st.started=true`）；`TextDelta`→`response.output_text.delta`（带 `output_index`/`content_index`/递增 `sequence_number`）；`ReasoningDelta`→`response.reasoning_summary_text.delta`；`ToolCall*`→`response.function_call_arguments.delta`/`.done`；`Usage`→缓存进 state 待 completed 带出；`Stop`→`response.completed`（status 由 reason 映射：EndTurn/ToolUse→`completed`、MaxTokens→`incomplete`）；`Done`→无帧或 `[DONE]` 行）。每帧 `st.seq += 1`。

- [ ] **Step 4: 通过** → `cargo test -p alva-llm-wire responses_encode_stream -- --nocolor` → PASS。
- [ ] **Step 5: Commit** `git commit -m "feat(llm-wire): Responses encode_stream_event (SSE, monotonic seq)"`

---

## Phase 5 — Chat 入站 + Anthropic 入站

> 与 Phase 4 同构，每协议 3 方法各一组 TDD task。下面给关键 wire 差异 + 必写测试；实现镜像各自文件的 `encode_messages`/`decode_response`/`decode_stream_event`。

### Task 5.1: Chat `decode_request`
**File:** `adapter/openai_chat.rs`
- 关键映射：`messages[]`（`role:system|user|assistant|tool` 内联）→ `Vec<Message>`；assistant 的 `tool_calls[]`→`ContentBlock::ToolUse`（`arguments` 是 string，`serde_json::from_str`）；`tool` 角色 + `tool_call_id`→`ContentBlock::ToolResult`；`tools[].function`→`ToolDefinition`；`reasoning_effort`→effort；遇 `content` 数组里的 image part→`UnexpectedFormat`。
- [ ] Step1 失败测试：
```rust
#[test]
fn chat_decode_request_roundtrips_tool_call() {
    let body = serde_json::json!({"model":"m","stream":false,"messages":[
        {"role":"system","content":"s"},
        {"role":"user","content":"hi"},
        {"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read","arguments":"{\"p\":\"/x\"}"}}]},
        {"role":"tool","tool_call_id":"call_1","content":"ok"}],
        "tools":[{"type":"function","function":{"name":"read","description":"d","parameters":{"type":"object"}}}]});
    let r = OpenAIChatAdapter::new().decode_request(&body).unwrap();
    assert_eq!(r.tools[0].name, "read");
    assert!(r.messages.iter().any(|m| matches!(m.role, MessageRole::Tool)));
}
```
- [ ] Step2 失败 → Step3 实现 → Step4 PASS → Step5 commit `feat(llm-wire): Chat decode_request`

### Task 5.2: Chat `encode_response`
**File:** `adapter/openai_chat.rs`
- 构造 `{id, object:"chat.completion", model, choices:[{index:0, message:{role:"assistant", content, tool_calls:[...]}, finish_reason}], usage:{prompt_tokens,completion_tokens,total_tokens}}`；finish_reason 来自……非流式无 Stop 事件，按"有 ToolUse→tool_calls 否则 stop"判定。
- [ ] Step1 测试断言 `v["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]` 是 string、`usage.prompt_tokens` 来自 `message.usage.input_tokens`。→ 失败 → 实现 → PASS → commit。

### Task 5.3: Chat `encode_stream_event`
**File:** `adapter/openai_chat.rs`
- `Start`→首帧 `{choices:[{delta:{role:"assistant"}}]}`；`TextDelta`→`{choices:[{delta:{content}}]}`；`ToolCall*`→`{choices:[{delta:{tool_calls:[{index,id,function:{name,arguments}}]}}]}`；`Stop{reason}`→`{choices:[{delta:{}, finish_reason: map(reason)}]}`（ToolUse→`tool_calls`/EndTurn→`stop`/MaxTokens→`length`）；`Done`→`[DONE]`（用 `SseFrame{event:None, data: json!("[DONE]")}` 约定，gateway 写成 `data: [DONE]`）。
- [ ] Step1 测试断言终止帧 `finish_reason=="length"`（输入 `Stop{MaxTokens}`）+ chunk `object=="chat.completion.chunk"`。→ 失败 → 实现 → PASS → commit。

### Task 5.4–5.6: Anthropic `decode_request` / `encode_response` / `encode_stream_event`
**File:** `adapter/anthropic.rs`
- decode_request：`system`（string 或 block 数组）→ 头部 `Message::system`；`messages[]`（content 可为 string 或 block 数组）→ `Message`；`tool_use`/`tool_result` block→ 对应 `ContentBlock`；`tools[]`（`{name,description,input_schema}`）→ `ToolDefinition`；`thinking.budget_tokens`→ effort（用 `ReasoningEffort` 就近映射：见 §7）；image block→`UnexpectedFormat`。
- encode_response：`{id, type:"message", role:"assistant", model, content:[{type:"text",text}|{type:"tool_use",id,name,input}], stop_reason: map, usage:{input_tokens,output_tokens}}`。**signature 规则**：若 `ContentBlock::Reasoning.signature.is_none()` 且本响应面向 Anthropic 客户端，按 §7：thinking block 须带 signature 才能回传——encode_response 输出 thinking 时若无 signature 则**省略该 thinking block**（不发不完整 thinking）。
- encode_stream_event：`message_start`→`content_block_start/delta(text_delta|input_json_delta)/stop`→`message_delta{stop_reason}`→`message_stop`。`Stop{reason}`→`message_delta` 的 `stop_reason`（EndTurn→`end_turn`/ToolUse→`tool_use`/MaxTokens→`max_tokens`/StopSequence→`stop_sequence`/Other(s)→s）。
- 每个 task：失败测试（断言关键字段）→ 实现（镜像同文件 decode/encode）→ PASS → commit。

---

## Phase 6 — `AliasRouter` + `alva-app-gateway`

### Task 6.1: `AliasRouter`（alva-llm-provider）

**Files:** Modify `crates/alva-llm-provider/src/registry.rs`、`lib.rs`（导出）

- [ ] **Step 1: 失败测试**
```rust
#[test]
fn alias_router_resolves_and_reports_protocol() {
    let mut r = AliasRouter::new();
    r.insert("gpt-via-ds".into(), ProviderConfig{ api_key:"k".into(), model:"deepseek-chat".into(),
        base_url:"https://api.deepseek.com/v1".into(), max_tokens:1024, custom_headers:Default::default(), kind:Some("openai-chat".into())});
    assert_eq!(r.upstream_protocol("gpt-via-ds"), Some("openai-chat"));
    assert!(r.resolve("gpt-via-ds").is_some());
    assert!(r.resolve("missing").is_none());
}
```

- [ ] **Step 2: 失败** → FAIL。

- [ ] **Step 3: 实现**
```rust
use std::collections::HashMap;
pub struct AliasRouter { routes: HashMap<String, ProviderConfig> }
impl AliasRouter {
    pub fn new() -> Self { Self { routes: HashMap::new() } }
    pub fn insert(&mut self, alias: String, cfg: ProviderConfig) { self.routes.insert(alias, cfg); }
    pub fn upstream_protocol(&self, alias: &str) -> Option<&'static str> {
        self.routes.get(alias).map(provider_id_from_config)
    }
    pub fn resolve(&self, alias: &str) -> Option<std::sync::Arc<dyn LanguageModel>> {
        let cfg = self.routes.get(alias)?.clone();
        let model = cfg.model.clone();
        ConfigProviderAdapter::new(cfg).language_model(&model).ok()
    }
}
```

- [ ] **Step 4: 通过 + 导出** `lib.rs` 加 `pub use registry::AliasRouter;` → `cargo test -p alva-llm-provider alias_router -- --nocolor` PASS。
- [ ] **Step 5: Commit** `git commit -m "feat(llm-provider): AliasRouter (multi-alias upstream routing, reuses ConfigProviderAdapter)"`

### Task 6.2: gateway crate 骨架 + `GatewayConfig`

**Files:** Create `crates/alva-app-gateway/{Cargo.toml, src/lib.rs, src/config.rs, src/main.rs}`；根 `Cargo.toml` members。

- [ ] **Step 1: Cargo.toml**
```toml
[package]
name = "alva-app-gateway"
version = "0.1.0"
edition = "2021"
[dependencies]
alva-llm-wire = { path = "../alva-llm-wire" }
alva-llm-provider = { path = "../alva-llm-provider" }
alva-kernel-abi = { path = "../alva-kernel-abi" }
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
async-trait = "0.1"
futures = "0.3"
```

- [ ] **Step 2: `config.rs` 失败测试 + 实现**
```rust
#[test]
fn route_config_resolves_env_to_provider_config() {
    std::env::set_var("TEST_KEY", "secret");
    let rc = RouteConfig{ kind:"openai-chat".into(), base_url:"u".into(), api_key_env:"TEST_KEY".into(), model:"m".into(), max_tokens:None };
    let pc = rc.to_provider_config().unwrap();
    assert_eq!(pc.api_key, "secret");
    assert_eq!(pc.kind.as_deref(), Some("openai-chat"));
}
```
实现 `GatewayConfig{listen, routes: HashMap<String,RouteConfig>}` + `RouteConfig{kind,base_url,api_key_env,model,max_tokens}` + `to_provider_config()`（读 env，缺失返回 Err）+ `from_yaml(&str)`。

- [ ] **Step 3: 测 + commit** `cargo test -p alva-app-gateway -- --nocolor` PASS → `git commit -m "feat(gateway): GatewayConfig/RouteConfig + env-resolved ProviderConfig"`

### Task 6.3: `RawTool` 透传工具

**Files:** Create `crates/alva-app-gateway/src/raw_tool.rs`

- [ ] **Step 1: 失败测试**
```rust
#[tokio::test]
async fn raw_tool_carries_def_and_never_executes() {
    let rt = RawTool::new("read".into(), "d".into(), serde_json::json!({"type":"object"}));
    assert_eq!(rt.name(), "read");
    assert_eq!(rt.parameters_schema(), serde_json::json!({"type":"object"}));
    let err = rt.execute(serde_json::json!({}), &alva_kernel_abi::tool::execution::MinimalExecutionContext::new()).await;
    assert!(err.is_err()); // gateway 永不执行
}
```

- [ ] **Step 2: 失败 → 实现**
```rust
use alva_kernel_abi::tool::{Tool, execution::{ToolExecutionContext, ToolOutput}};
use alva_kernel_abi::base::error::AgentError;
pub struct RawTool { name: String, description: String, schema: serde_json::Value }
impl RawTool { pub fn new(name:String, description:String, schema:serde_json::Value)->Self{Self{name,description,schema}} }
#[async_trait::async_trait]
impl Tool for RawTool {
    fn name(&self)->&str{&self.name}
    fn description(&self)->&str{&self.description}
    fn parameters_schema(&self)->serde_json::Value{self.schema.clone()}
    async fn execute(&self,_i:serde_json::Value,_c:&dyn ToolExecutionContext)->Result<ToolOutput,AgentError>{
        Err(AgentError::ToolError("gateway RawTool is passthrough-only".into()))
    }
}
```
（确认 `AgentError` 变体名，按实际调整。）

- [ ] **Step 3: PASS + commit** `git commit -m "feat(gateway): RawTool passthrough (ToolDefinition -> dyn Tool, never executes)"`

### Task 6.4: HTTP 串线（非流式先通）

**Files:** Create `crates/alva-app-gateway/src/server.rs`；Modify `lib.rs`（`pub async fn serve`）

- [ ] **Step 1: 实现 handler（按入站协议选 adapter）**

每条路由：
```rust
async fn handle(inbound: &dyn ProtocolAdapter, router: &AliasRouter, body: Value) -> Result<Response, GatewayError> {
    let req = inbound.decode_request(&body).map_err(bad_request)?;       // 含 image→400
    let lm = router.resolve(&req.model).ok_or(not_found(&req.model))?;
    let raw_tools: Vec<RawTool> = req.tools.iter().map(|t| RawTool::new(t.name.clone(), t.description.clone(), t.parameters.clone())).collect();
    let tool_refs: Vec<&dyn Tool> = raw_tools.iter().map(|t| t as &dyn Tool).collect();
    if req.stream {
        // SSE：把 lm.stream(...) 的每个 StreamEvent 经 inbound.encode_stream_event 写出
    } else {
        let cr = lm.complete(&req.messages, &tool_refs, &req.config).await.map_err(upstream_err)?;
        let dr = DecodedResponse { message: cr.message.clone(), usage: cr.message.usage.clone() };
        let body = inbound.encode_response(&dr).map_err(internal)?;
        Ok(json_200(body))
    }
}
```
路由表：`/v1/responses`→`OpenAIResponsesAdapter`、`/v1/chat/completions`→`OpenAIChatAdapter`、`/v1/messages`→`AnthropicAdapter`。

- [ ] **Step 2: 集成测试（mock 上游，非流式）**

Create `crates/alva-app-gateway/tests/gateway_e2e.rs`，复用 `alva-app-core/tests/e2e_http_test.rs` 的 mock server 写法：起 mock Chat 上游 + 网关（route `gpt-x`→该 mock，kind=openai-chat）。POST `/v1/responses` 一个最小请求，断言：(a) mock 收到 `/chat/completions` 形状（messages 内联）；(b) 客户端拿到 `object=="response"`。
```rust
// 关键断言
assert_eq!(resp_json["object"], "response");
assert_eq!(mock_seen_path, "/chat/completions");
```

- [ ] **Step 3: 测** `cargo test -p alva-app-gateway --test gateway_e2e -- --nocolor` PASS。
- [ ] **Step 4: Commit** `git commit -m "feat(gateway): HTTP dispatch + non-streaming Responses->Chat e2e"`

### Task 6.5: 流式 SSE 串线 + 交错用例

**Files:** Modify `server.rs`

- [ ] **Step 1: 实现 SSE 分支**（用 axum `Sse` + `async_stream`，对 `lm.stream` 的每个 `StreamEvent` 调 `inbound.encode_stream_event(&ev, &mut st)`，把每个 `SseFrame` 写成 `event: <name>\ndata: <json>\n\n`；`SseFrame{event:None,data:"[DONE]"}` 写成 `data: [DONE]`）。

- [ ] **Step 2: 集成测试（流式 + 交错）**

mock 上游返回 文本→tool_call→文本 交错的 SSE。断言客户端收到的 Responses SSE：`response.created` 在最前、`response.completed` 在最后、`sequence_number` 全程单调。

- [ ] **Step 3: 测 + Commit** `git commit -m "feat(gateway): streaming SSE passthrough with interleaved-order test"`

### Task 6.6: `main.rs` 瘦二进制 + image 拒绝测试

- [ ] **Step 1:** `main.rs`：读 `--config gateway.yml`（或 env），`GatewayConfig::from_yaml` → 建 `AliasRouter` → `serve(router, &cfg.listen).await`。
- [ ] **Step 2:** 集成测试：带 image block 的 `/v1/messages` 请求 → 断言 400。
- [ ] **Step 3:** `cargo run -p alva-app-gateway -- --config <tmp.yml>` 手动冒烟（可选）。
- [ ] **Step 4: Commit** `git commit -m "feat(gateway): binary entrypoint + image-input 400 test"`

---

## Phase 7（可选）— App 内嵌

### Task 7.1: Tauri/CLI embed `serve`
**Files:** Modify `crates/alva-app-tauri/src/...`（加一个可选命令/设置启动内嵌网关，用 app 现有 `ProviderConfig` 填 `AliasRouter`）。
- [ ] Step1 在 app 里调 `alva_app_gateway::serve(router, addr)`（后台 task）。
- [ ] Step2 手动验证 app 启动后 `curl localhost:<port>/v1/models`（若实现）或 `/v1/responses`。
- [ ] Step3 Commit `feat(app-tauri): optional embedded gateway`

---

## Self-Review 记录（spec 覆盖核对）

- §5.1 类型下沉 + 双路径 re-export → Phase 0 + Task 2.2 ✓
- §5.1 ProtocolAdapter 双向 + StopReason → Phase 1 + Task 4.1 ✓
- §5.2/5.3 kernel-abi re-export + encode_tools 改签名 → Task 2.2 / 3.1 / 3.2 ✓
- §5.4 gateway + RawTool + 配置 → Task 6.2–6.6 ✓
- §6 数据流（流式/非流式）→ Task 6.4 / 6.5 ✓
- §7 入站三方法 + StopReason 映射 + 多模态拒绝 + signature → Task 4.2–4.4 / 5.x ✓
- §8 配置（api_key_env/listen 包装）→ Task 6.2 ✓
- §9 错误信封 → Task 6.4 的 `bad_request/not_found/upstream_err` helper（实现时按各协议 error 体细化；见 §9）
- §10 测试（round-trip / StopReason / 交错 / image 拒绝 / wasm）→ 散落各 task ✓
- §11 分阶段 + 影响面 → Phase 0→7 ✓
- §12 usage 从 `message.usage` 取（Task 6.4 `dr.usage = cr.message.usage`）✓；/v1/models 留 Phase 7 可选

**待实现者注意**：§9 错误信封各协议形状（Responses/Chat `{error:{message,type,code}}`、Anthropic `{type:"error",error:{type,message}}`）在 Task 6.4 落地时按协议补；Anthropic `thinking` budget↔effort 就近映射用 `ReasoningEffort::suggested_token_budget` 反查。
