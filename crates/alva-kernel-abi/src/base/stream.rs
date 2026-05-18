// INPUT:  serde, crate::base::message::UsageMetadata
// OUTPUT: pub enum StreamEvent
// POS:    Streaming event enum representing incremental deltas from a language model response.
use serde::{Deserialize, Serialize};

use crate::base::message::UsageMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    Start,
    TextDelta { text: String },
    ReasoningDelta { text: String },
    /// Completed reasoning / thinking block — emitted by adapter when a
    /// full reasoning content block ends in the stream. Consumers
    /// (run.rs) should append a `ContentBlock::Reasoning { text, signature }`
    /// to the built assistant message. This is the authoritative record
    /// of the block, in contrast with `ReasoningDelta` which is a UI
    /// progress signal. `signature` is Anthropic's extended-thinking
    /// attestation that MUST be echoed back verbatim on the next turn.
    ReasoningBlock {
        text: String,
        signature: Option<String>,
    },
    /// A new tool call is about to stream. Fires once per tool call, before
    /// any `ToolCallDelta`. UI layers can use this to render a "tool X
    /// starting" indicator and to allocate per-tool state keyed by `id`.
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        name: Option<String>,
        arguments_delta: String,
    },
    /// The tool call with this `id` has emitted its last argument delta —
    /// callers holding per-tool buffers can finalize / parse them now.
    /// Fires once per tool call, after all `ToolCallDelta`s for that id.
    ToolCallEnd {
        id: String,
    },
    Usage(UsageMetadata),
    Done,
    Error(String),
}

#[cfg(test)]
mod tests {
    //! Tests for StreamEvent serde wire format.
    //!
    //! The enum uses default `externally-tagged` representation
    //! (no `#[serde(tag = ...)]`):
    //!   * Unit variants serialize as the bare variant name string
    //!   * Struct variants serialize as `{"VariantName": {fields}}`
    //!   * Newtype variants serialize as `{"VariantName": value}`
    //!
    //! Provider adapters emit these events; SSE-layer consumers parse
    //! them back. Wire-format drift breaks streaming silently.
    use super::*;
    use serde_json::{json, Value};

    fn roundtrip(ev: &StreamEvent) -> Value {
        let v = serde_json::to_value(ev).expect("serialize StreamEvent");
        // Roundtrip back to assert the shape is decodable.
        let back: StreamEvent = serde_json::from_value(v.clone()).expect("deserialize StreamEvent");
        // We don't assert event-equality (StreamEvent isn't PartialEq);
        // we re-serialize and compare values to confirm idempotent shape.
        let back_v = serde_json::to_value(&back).expect("serialize back");
        assert_eq!(v, back_v, "roundtrip changed shape: first={v}, second={back_v}");
        v
    }

    // -- Unit variants ----------------------------------------------------

    #[test]
    fn start_serializes_as_bare_variant_string() {
        // External tag for unit variant: just the name as a JSON string.
        assert_eq!(roundtrip(&StreamEvent::Start), json!("Start"));
    }

    #[test]
    fn done_serializes_as_bare_variant_string() {
        assert_eq!(roundtrip(&StreamEvent::Done), json!("Done"));
    }

    // -- Struct variants --------------------------------------------------

    #[test]
    fn text_delta_serializes_with_named_fields_under_variant_key() {
        let v = roundtrip(&StreamEvent::TextDelta { text: "hello".into() });
        assert_eq!(v, json!({ "TextDelta": { "text": "hello" } }));
    }

    #[test]
    fn reasoning_delta_serializes_under_variant_key() {
        let v = roundtrip(&StreamEvent::ReasoningDelta { text: "thinking".into() });
        assert_eq!(v, json!({ "ReasoningDelta": { "text": "thinking" } }));
    }

    #[test]
    fn reasoning_block_with_signature_serializes_both_fields() {
        // Anthropic-critical pin: when present, `signature` MUST be
        // serialized so the next turn can echo it back. A future
        // serde attr that skipped it would silently 400 on the next
        // request.
        let v = roundtrip(&StreamEvent::ReasoningBlock {
            text: "deep".into(),
            signature: Some("sig-abc".into()),
        });
        assert_eq!(
            v,
            json!({ "ReasoningBlock": { "text": "deep", "signature": "sig-abc" } })
        );
    }

    #[test]
    fn reasoning_block_without_signature_includes_null_field() {
        // Pin current behavior: NO `skip_serializing_if`, so
        // `signature: None` shows up as `"signature": null` in the
        // wire format. If someone adds a skip-on-None later, the
        // wire payload changes — this test fires.
        let v = roundtrip(&StreamEvent::ReasoningBlock {
            text: "deep".into(),
            signature: None,
        });
        assert_eq!(
            v,
            json!({ "ReasoningBlock": { "text": "deep", "signature": null } })
        );
    }

    #[test]
    fn tool_call_start_serializes_with_id_and_name() {
        let v = roundtrip(&StreamEvent::ToolCallStart {
            id: "id1".into(),
            name: "read_file".into(),
        });
        assert_eq!(v, json!({ "ToolCallStart": { "id": "id1", "name": "read_file" } }));
    }

    #[test]
    fn tool_call_delta_with_name_some() {
        let v = roundtrip(&StreamEvent::ToolCallDelta {
            id: "id1".into(),
            name: Some("tool".into()),
            arguments_delta: "{\"k\":".into(),
        });
        assert_eq!(
            v,
            json!({ "ToolCallDelta": { "id": "id1", "name": "tool", "arguments_delta": "{\"k\":" } })
        );
    }

    #[test]
    fn tool_call_end_serializes_id_only() {
        let v = roundtrip(&StreamEvent::ToolCallEnd { id: "id1".into() });
        assert_eq!(v, json!({ "ToolCallEnd": { "id": "id1" } }));
    }

    // -- Newtype variants -------------------------------------------------

    #[test]
    fn usage_newtype_serializes_under_variant_key() {
        let usage = UsageMetadata {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        };
        let v = roundtrip(&StreamEvent::Usage(usage));
        // Cache fields skipped (UsageMetadata pinned in L116).
        assert_eq!(
            v,
            json!({ "Usage": { "input_tokens": 10, "output_tokens": 5, "total_tokens": 15 } })
        );
    }

    #[test]
    fn error_newtype_serializes_under_variant_key() {
        let v = roundtrip(&StreamEvent::Error("boom".into()));
        assert_eq!(v, json!({ "Error": "boom" }));
    }
}
