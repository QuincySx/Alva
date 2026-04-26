//! LLM provider implementations.

pub mod anthropic;
pub mod gemini;
pub mod openai_chat;
pub mod openai_responses;

use serde_json::Value;

/// Merge user-supplied `extra_body` overrides into a freshly-built
/// request body. Last-write-wins: every key in `extra_body` overwrites
/// whatever the provider's `build_body` set for that key (or adds it
/// fresh). Nested objects are NOT deep-merged — passing
/// `{ "thinking": { "type": "disabled" } }` *replaces* the whole
/// `thinking` field if one was already there.
///
/// No-op if `extra_body` is `None` or empty.
///
/// Used by every provider at the tail of its body assembly so that
/// model-specific override knobs (Doubao `thinking`, Ollama `options`,
/// LiteLLM `extra_body` style escape hatches) flow through without
/// each provider growing a one-off case branch.
pub(crate) fn apply_extra_body(
    body: &mut Value,
    extra_body: Option<&serde_json::Map<String, Value>>,
) {
    let Some(extra) = extra_body else { return };
    if extra.is_empty() {
        return;
    }
    let Some(obj) = body.as_object_mut() else {
        // Body should always be an object — defensive only. If it
        // isn't, the user's extras can't be merged structurally so
        // skip (we'd rather no override than corrupt body).
        return;
    };
    for (k, v) in extra {
        obj.insert(k.clone(), v.clone());
    }
}
