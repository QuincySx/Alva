// INPUT:  serde_json, crate::{message::Message, stream::StreamEvent, tool_def::ToolDefinition}
// OUTPUT: ToolAdapter trait, AdapterError, EncodedMessages, DecodedResponse, StreamDecodeState
// POS:    Provider-neutral tool / message serialization contract. 4 concrete impls (Anthropic /
//         OpenAI Chat / OpenAI Responses / Gemini) translate normalized alva-llm-wire types into
//         each LLM provider's wire JSON and back. Pure data transformation — no HTTP, no reqwest,
//         wasm-friendly. See AMP models/adapter-layer.md for the reference design.

//! Tool / message serialization adapters for different LLM provider APIs.
//!
//! **Why this lives in alva-llm-wire**: adapters are the serialization contract
//! *of* the wire types (`Message`, `ContentBlock`, `StreamEvent`). They only
//! consume / produce `serde_json::Value` — no HTTP, no runtime. Keeping them
//! here means any SDK consumer gets 4-provider tool-calling for free, including
//! wasm consumers that can't pull in `alva-llm-provider` (which depends on reqwest).
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
//! use alva_llm_wire::adapter::{ProtocolAdapter, anthropic::AnthropicAdapter};
//!
//! let adapter = AnthropicAdapter::new();
//! let encoded = adapter.encode_messages(&messages);
//! let tool_json = adapter.encode_tools(&tools);
//! // ... send via HTTP ...
//! let response = adapter.decode_response(&resp_json)?;
//! ```

use std::collections::HashMap;

use serde_json::Value;

use crate::message::{Message, UsageMetadata};
use crate::stream::StreamEvent;
use crate::tool_def::ToolDefinition;

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

    // -----------------------------------------------------------------------
    // Inbound (gateway) methods — default impl returns InboundUnsupported.
    // Override these in protocol adapters that act as a gateway endpoint.
    // -----------------------------------------------------------------------

    /// Decode an inbound client request (this protocol's wire format) into a
    /// neutral [`DecodedRequest`] that can be forwarded to any outbound adapter.
    ///
    /// The default implementation returns
    /// [`AdapterError::InboundUnsupported`] — override in adapters that
    /// need to serve as a gateway endpoint.
    fn decode_request(&self, _body: &serde_json::Value) -> Result<DecodedRequest, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.provider()))
    }

    /// Encode a neutral [`DecodedResponse`] back into this protocol's
    /// non-streaming response JSON body.
    ///
    /// The default implementation returns
    /// [`AdapterError::InboundUnsupported`] — override in adapters that
    /// need to serve as a gateway endpoint.
    fn encode_response(
        &self,
        _resp: &DecodedResponse,
    ) -> Result<serde_json::Value, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.provider()))
    }

    /// Encode a single neutral [`StreamEvent`] into this protocol's SSE
    /// frame(s). The `state` argument carries response-level counters that
    /// must stay consistent across all frames for a single streaming response.
    ///
    /// The default implementation returns
    /// [`AdapterError::InboundUnsupported`] — override in adapters that
    /// need to serve as a gateway endpoint.
    fn encode_stream_event(
        &self,
        _ev: &crate::stream::StreamEvent,
        _st: &mut StreamEncodeState,
    ) -> Result<Vec<SseFrame>, AdapterError> {
        Err(AdapterError::InboundUnsupported(self.provider()))
    }
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
    /// The inbound (gateway) direction is not implemented for this protocol.
    /// Argument is the provider name from [`ProtocolAdapter::provider`].
    InboundUnsupported(&'static str),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::MissingField(name) => write!(f, "missing field: {name}"),
            AdapterError::UnexpectedFormat(msg) => write!(f, "unexpected format: {msg}"),
            AdapterError::Parse(msg) => write!(f, "parse error: {msg}"),
            AdapterError::InboundUnsupported(p) => {
                write!(f, "inbound not supported for protocol: {p}")
            }
        }
    }
}

impl std::error::Error for AdapterError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_inbound_is_unsupported() {
        let a = crate::adapter::gemini::GeminiAdapter::new();
        assert!(matches!(
            a.decode_request(&serde_json::json!({})),
            Err(AdapterError::InboundUnsupported(_))
        ));
    }
}

// ---------------------------------------------------------------------------
// Inbound (gateway) types
// ---------------------------------------------------------------------------

/// Output of [`ProtocolAdapter::decode_request`]: an inbound wire request
/// parsed from a client's protocol-specific body into a neutral form that
/// can be forwarded to any outbound adapter.
pub struct DecodedRequest {
    /// The model identifier string from the inbound request body.
    pub model: String,
    /// Conversation turns decoded into normalized [`Message`] values.
    pub messages: Vec<crate::message::Message>,
    /// Tool definitions decoded from the inbound request body.
    pub tools: Vec<crate::tool_def::ToolDefinition>,
    /// Sampling / generation configuration from the inbound request body.
    pub config: crate::config::ModelConfig,
    /// Whether the client requested a streaming response.
    pub stream: bool,
}

/// One SSE frame to write to a client: an optional event name (the `event:`
/// line) plus the JSON payload that goes in the `data:` line.
pub struct SseFrame {
    /// Optional SSE `event:` name. `None` → omit the `event:` line.
    pub event: Option<String>,
    /// JSON payload for the `data:` line.
    pub data: serde_json::Value,
}

/// Cross-event state for [`ProtocolAdapter::encode_stream_event`].
///
/// Tracks the identifiers and sequence counters that must stay consistent
/// across all SSE frames for a single streaming response. Each concrete
/// protocol uses the fields it needs and ignores the rest.
#[derive(Default)]
pub struct StreamEncodeState {
    /// The response / completion id to embed in every frame (e.g.
    /// `chatcmpl-…` for OpenAI-style protocols).
    pub response_id: String,
    /// Monotonically increasing sequence number; some protocols (OpenAI
    /// Responses API) require a per-frame integer index.
    pub seq: i64,
    /// Current output/content block index within the response (used by
    /// protocols that index content parts separately from the message).
    pub output_index: usize,
    /// Whether the opening / prologue frame has been emitted. Protocols
    /// that emit a distinct first frame (e.g. a `[DONE]` sentinel) check
    /// this to know whether the stream has been opened.
    pub started: bool,
}
