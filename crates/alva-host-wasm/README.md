# alva-host-wasm

> wasm32 host装配层 for the alva agent kernel — wasm-side counterpart of `alva-host-native`.

This crate lets you run the `alva-kernel-core` agent loop in a browser
(or any wasm runtime). It provides the wasm-friendly glue: a `Sleeper`
impl backed by `gloo-timers`, a minimal `WasmAgent` facade, a
`#[wasm_bindgen]` entry module, and a public `StubLanguageModel` for
smoke tests.

It does **not** ship a real LLM provider, real tools, persistent
storage, or a CORS proxy. Those are the consumer's responsibility, by
design — see the responsibility table below.

## Library vs consumer responsibilities

| Responsibility | Owner | Notes |
|---|---|---|
| `LanguageModel` trait wasm-impl-able | **library** | Verified by `StubLanguageModel` |
| `Tool` trait wasm-impl-able | **library** | Caller passes `Vec<Arc<dyn Tool>>` to `WasmAgent::new` |
| `AgentSession` trait wasm-impl-able | **library** | `WasmAgent::with_session` accepts any impl |
| `Sleeper` runtime primitive | **library** | `WasmSleeper` via `spawn_local + oneshot` |
| `WasmAgent` consumer facade | **library** | `new` / `with_session` / `run` / `run_simple` / `clear_session` |
| Real LLM provider impl | **consumer** | Use `gloo-net::http`, `web_sys::fetch`, or `reqwest` with the wasm32 feature |
| Real tools impl | **consumer** | The tools in `alva-agent-extension-builtin` target native on purpose; wasm consumers write their own |
| Persistent session impl | **consumer** | Implement `AgentSession` over IndexedDB / localStorage / your backend |
| `wasm-pack build` | **consumer** | Run it on your wasm app, not on this library |
| CORS / API key proxying | **consumer** | Browser → your backend → upstream LLM API |

## Quick start

### 1. Add the dependency

```toml
[dependencies]
alva-host-wasm = { path = "../alva-host-wasm" }  # or git/version
alva-kernel-abi = { path = "../alva-kernel-abi" }
async-trait = "0.1"
gloo-net = "0.6"
serde_json = "1"
```

### 2. Implement a `LanguageModel` that talks to your proxy

```rust
use alva_kernel_abi::base::content::ContentBlock;
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::{Message, MessageRole};
use alva_kernel_abi::model::{CompletionResponse, LanguageModel, ModelConfig};
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct ProxiedLlm;

#[async_trait]
impl LanguageModel for ProxiedLlm {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        // POST to your own backend, which holds the API key and forwards
        // to OpenAI / Anthropic / etc. The browser only ever talks to
        // your own origin, so CORS + key handling stays server-side.
        let body = serde_json::json!({ "messages": messages });
        let resp = gloo_net::http::Request::post("/api/chat")
            .json(&body)
            .map_err(|e| AgentError::LlmError(e.to_string()))?
            .send()
            .await
            .map_err(|e| AgentError::LlmError(e.to_string()))?;
        let text: String = resp
            .text()
            .await
            .map_err(|e| AgentError::LlmError(e.to_string()))?;
        Ok(CompletionResponse::from_message(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }))
    }

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = alva_kernel_abi::base::stream::StreamEvent> + Send>> {
        // Either implement SSE / chunked streaming via fetch, or
        // synthesize a single-event stream from complete() for the
        // simplest possible non-streaming agent.
        unimplemented!("see gloo-net::http::Request::send() with response.body()")
    }

    fn model_id(&self) -> &str { "proxied" }
}
```

### 3. Wire it into `WasmAgent` and expose to JS

```rust
use std::sync::Arc;
use alva_host_wasm::WasmAgent;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn ask(prompt: String) -> Result<String, JsValue> {
    let mut agent = WasmAgent::new(
        Arc::new(ProxiedLlm),
        Vec::new(),                              // no tools for a chat-only demo
        "You are a helpful assistant.",
    );
    agent
        .run_simple(prompt)
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
```

### 4. Build for the browser

```bash
# Install wasm-pack once per machine.
cargo install wasm-pack

# Build your wasm app crate (the one with #[wasm_bindgen] entries).
wasm-pack build --target web --out-dir pkg

# Serve the result. Chrome refuses to load wasm via file://.
python3 -m http.server 8000
```

### 5. Call it from JS

```html
<!doctype html>
<script type="module">
  import init, { ask } from './pkg/your_wasm_app.js';
  await init();
  const reply = await ask('hi from chrome');
  document.body.textContent = reply;
</script>
```

That's it. Every piece of consumer-side code lives in your app crate;
the library only owns the trait surface and `WasmAgent`.

## Architecture notes

- **Why a separate `WasmSleeper`?** `alva_kernel_abi::Sleeper` requires
  `Send + Sync`, but `gloo_timers::future::sleep` returns a future
  that holds a non-Send `Closure`. `WasmSleeper` bridges this with
  `wasm_bindgen_futures::spawn_local + tokio::sync::oneshot`: the
  non-Send timer runs inside `spawn_local`'s single-threaded task,
  while the outer future captures only the `Receiver<()>` (which is
  Send). The kernel sees a Send future and stays runtime-agnostic.

- **Why an empty default tool set?** `alva-agent-extension-builtin` is the native
  batteries-included tool collection — file edit, shell, grep,
  browser automation, sqlite memory. Most of those have no meaning
  in a browser. wasm consumers should write only the tools that make
  sense for their domain (or run a chat-only agent with zero tools).

- **Why `cdylib` + `rlib`?** `cdylib` is the crate type wasm-pack /
  wasm-bindgen-cli need to emit a real `.wasm` binary. `rlib` is
  kept so the crate is still importable from native Rust workspaces
  (e.g., the smoke probe in `src/smoke.rs` runs on `cargo test`
  through the rlib path).

- **What about `tokio::time` / `Instant` / `SystemTime`?** The kernel
  layers (`alva-kernel-{bus,abi,core}`) have been audited for
  wasm32-blocking primitives. `Instant::now()` is replaced with
  `web-time`, `SystemTime::now()` with `chrono`, and
  `tokio::time::timeout` is wrapped behind `Sleeper` + the runtime-
  agnostic `alva_kernel_abi::timeout` helper. See `ci-check-deps.sh`
  for the regression-protected wasm32 invariant.
