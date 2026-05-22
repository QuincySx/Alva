// INPUT:  serde_json, crate::{base::message::Message, base::stream::StreamEvent, tool::Tool}
// OUTPUT: ToolAdapter trait, AdapterError, EncodedMessages, DecodedResponse, StreamDecodeState
// POS:    Provider-neutral tool / message serialization contract. 4 concrete impls (Anthropic /
//         OpenAI Chat / OpenAI Responses / Gemini) translate normalized alva-kernel-abi types into
//         each LLM provider's wire JSON and back. Pure data transformation — no HTTP, no reqwest,
//         wasm-friendly. See AMP models/adapter-layer.md for the reference design.

//! Tool / message serialization adapters for different LLM provider APIs.
//!
//! **Why this lives in kernel-abi, not llm-provider**: adapters are the
//! serialization contract *of* the kernel-abi types (`Tool`, `Message`,
//! `ContentBlock`, `StreamEvent`). They only consume / produce
//! `serde_json::Value` — no HTTP, no runtime. Keeping them here means
//! any SDK consumer gets 4-provider tool-calling for free, including
//! wasm consumers that can't pull in `alva-llm-provider` (which
//! depends on reqwest).
//!
//! # Adapters
//!
//! - [`anthropic::AnthropicAdapter`] — Anthropic `messages.create`
//! - [`openai_chat::OpenAIChatAdapter`] — OpenAI Chat Completions
//!   (also Groq / Fireworks / OpenRouter / xAI / Moonshot / etc.)
//! - [`openai_responses::OpenAIResponsesAdapter`] — OpenAI Responses API
//! - [`gemini::GeminiAdapter`] — Google Vertex AI Gemini
//!
//! # Usage
//!
//! ```rust,ignore
//! use alva_kernel_abi::adapter::{ProtocolAdapter, anthropic::AnthropicAdapter};
//!
//! let adapter = AnthropicAdapter::new();
//! let encoded = adapter.encode_messages(&messages);
//! let tool_json = adapter.encode_tools(&tools);
//! // ... send via HTTP ...
//! let response = adapter.decode_response(&resp_json)?;
//! ```

use std::collections::HashMap;

use serde_json::Value;

use crate::base::message::{Message, UsageMetadata};
use crate::base::stream::StreamEvent;
use crate::tool::ToolDefinition;

pub mod common;

pub mod anthropic;
pub mod gemini;
pub mod openai_chat;
pub mod openai_responses;

// ---------------------------------------------------------------------------
// ProtocolAdapter trait (formerly ToolAdapter)
// ---------------------------------------------------------------------------

/// Provider-neutral contract for translating normalized `Message` / `Tool` /
/// `ContentBlock` / `StreamEvent` values into a specific LLM provider's wire
/// JSON and back.
///
/// Each concrete adapter owns the quirks of one provider family:
/// - schema flavor (strict vs open, `additionalProperties` etc.)
/// - tool-use block layout (`tool_use` vs `tool_calls[]` vs `function_call`
///   vs `functionCall`)
/// - streaming delta shape (per-token vs per-arg-chunk vs non-streaming)
/// - message role encoding (Anthropic splits system out; OpenAI keeps it inline)
///
/// Adapters are stateless by construction (take `&self`); streaming decode
/// uses an explicit `&mut StreamDecodeState` to accumulate partial JSON.
pub trait ProtocolAdapter: Send + Sync {
    /// Provider identifier, e.g. `"anthropic"`, `"openai-chat"`, `"gemini"`.
    /// Used for tracing and debug logs.
    fn provider(&self) -> &'static str;

    /// Encode a list of tool definitions into the provider's `tools` field. Each
    /// element is a JSON value ready to be spliced into the request body.
    ///
    /// Adapters handle provider-specific quirks: Anthropic passes schema
    /// through as-is; OpenAI Chat wraps in `{type:"function", function:{}}`
    /// and fixes missing property `type`s (YLR); Gemini wraps the whole
    /// list in a single `functionDeclarations[]` tool.
    fn encode_tools(&self, tools: &[ToolDefinition]) -> Vec<Value>;

    /// Encode a normalized conversation into the provider's messages shape.
    ///
    /// Anthropic separates system prompt; OpenAI keeps it as a `system`-role
    /// message in the array. The returned [`EncodedMessages::system`] is
    /// populated only for providers that want system out-of-band.
    fn encode_messages(&self, messages: &[Message]) -> EncodedMessages;

    /// Decode a completed non-streaming response body into a normalized
    /// `Message` plus usage metadata. `raw` retains the original JSON for
    /// callers that need provider-specific fields not surfaced by the
    /// normalization.
    fn decode_response(&self, response: &Value) -> Result<DecodedResponse, AdapterError>;

    /// Decode a single streaming event (one SSE `data:` line, already parsed
    /// as JSON) into zero or more normalized [`StreamEvent`]s. The `state`
    /// argument carries provider-internal buffers (e.g. partial tool-input
    /// JSON that must be concatenated before being parsed).
    fn decode_stream_event(
        &self,
        event: &Value,
        state: &mut StreamDecodeState,
    ) -> Result<Vec<StreamEvent>, AdapterError>;
}

/// Backwards-compatibility alias: `ToolAdapter` resolves to `ProtocolAdapter`.
/// Existing `use alva_kernel_abi::adapter::ToolAdapter` imports continue to
/// work while callers migrate to the new name.
pub use self::ProtocolAdapter as ToolAdapter;

// ---------------------------------------------------------------------------
// EncodedMessages
// ---------------------------------------------------------------------------

/// Output of [`ToolAdapter::encode_messages`]. Split into an optional
/// out-of-band `system` prompt (used by Anthropic) and the `messages` array
/// that goes into the request body unchanged.
///
/// `system_segments` is the layered system prompt: every entry except
/// the last is "stable" (cacheable for prompt-cache providers); the
/// last is "dynamic" (per-turn, no cache). Single-segment vec means
/// the whole system prompt is treated as dynamic. Empty vec means no
/// system prompt at all.
#[derive(Debug, Clone, Default)]
pub struct EncodedMessages {
    /// System prompt segments in stable→dynamic order. See struct
    /// docs above. `None` if the provider embeds system messages
    /// inline in `messages`.
    pub system_segments: Option<Vec<String>>,
    /// The messages array ready to be spliced into the request body.
    pub messages: Vec<Value>,
}

impl EncodedMessages {
    /// Backwards-compat helper: flatten segments into a single string
    /// joined with `\n\n`. Returns `None` when there are no segments.
    pub fn system_flat(&self) -> Option<String> {
        self.system_segments
            .as_ref()
            .map(|segs| segs.join("\n\n"))
            .filter(|s| !s.is_empty())
    }
}

// ---------------------------------------------------------------------------
// DecodedResponse
// ---------------------------------------------------------------------------

/// Output of [`ToolAdapter::decode_response`] — the normalized [`Message`]
/// plus usage metadata. The caller holds the original raw JSON if they
/// need it for provider-specific fields (OpenAI `finish_reason`,
/// Anthropic `stop_reason`, etc.); this struct stays minimal.
#[derive(Debug, Clone)]
pub struct DecodedResponse {
    pub message: Message,
    pub usage: Option<UsageMetadata>,
}

// ---------------------------------------------------------------------------
// StreamDecodeState
// ---------------------------------------------------------------------------

/// Cross-event state carried by [`ToolAdapter::decode_stream_event`].
///
/// Providers stream tool-use input as incremental JSON fragments (Anthropic
/// emits `input_json_delta`; OpenAI Chat streams `function.arguments` as
/// partial strings). Callers need to accumulate these fragments across
/// events before they can parse the complete input. This state holds the
/// per-tool-call buffers (keyed by normalized tool-use id).
#[derive(Debug, Default)]
pub struct StreamDecodeState {
    /// Partial tool-input JSON keyed by tool-use id, appended to as
    /// `delta` events arrive, drained when the tool-use block ends.
    pub tool_input_buf: HashMap<String, String>,
    /// Last-seen block type per streaming block index (Anthropic uses
    /// integer block indices; OpenAI uses tool_call indices). Used to
    /// dispatch delta events to the right handler.
    pub block_type: HashMap<usize, String>,
    /// Named SSE event type, set by providers that carry it in a
    /// separate `event:` line (OpenAI Responses API). Adapters read it
    /// to dispatch the following `data:` payload to the right handler.
    /// Providers clear it after each event.
    pub event_type: Option<String>,
}

impl StreamDecodeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop per-stream state once the response completes. Call this between
    /// successive streaming requests on the same adapter instance, or just
    /// construct a fresh `StreamDecodeState` per request.
    pub fn reset(&mut self) {
        self.tool_input_buf.clear();
        self.block_type.clear();
        self.event_type = None;
    }
}

// ---------------------------------------------------------------------------
// AdapterError
// ---------------------------------------------------------------------------

/// Error type returned by decode paths when a provider response doesn't
/// match the expected shape. Intentionally small — adapters should mostly
/// tolerate unknown fields (forward compat) and only error on genuinely
/// broken input.
#[derive(Debug, Clone)]
pub enum AdapterError {
    /// A required field was missing from the provider response.
    MissingField(&'static str),
    /// A field had an unexpected shape (e.g. expected string, got object).
    UnexpectedFormat(String),
    /// JSON parse failure on a fragment (e.g. partial tool-input JSON
    /// that failed to finalize at the end of a tool-use block).
    Parse(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::MissingField(name) => write!(f, "missing field: {name}"),
            AdapterError::UnexpectedFormat(msg) => write!(f, "unexpected format: {msg}"),
            AdapterError::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for AdapterError {}
