// INPUT:  serde_json::Value
// OUTPUT: schema_fix::fill_missing_type, tool_id::{to_normalized, to_provider}
// POS:    Cross-adapter shared utilities — YLR schema patch, toolu_* id prefix handling.

//! Shared utilities used by multiple `ToolAdapter` implementations.
//!
//! None of these leak into the public trait; they are internal helpers
//! that individual adapters call as needed.

use serde_json::Value;

// ---------------------------------------------------------------------------
// schema_fix
// ---------------------------------------------------------------------------

/// JSON Schema patching used by strict providers (OpenAI Chat Completions
/// rejects `properties` that omit `type`; Anthropic tolerates it).
pub mod schema_fix {
    use super::*;

    /// Recursively fill missing `type` keys on every `properties` child.
    /// Rule (from AMP `YLR`): if the node already has `type`, leave it;
    /// else if it has `items`, it's an array; otherwise default to object.
    ///
    /// Operates in place on `schema` — safe to call on a schema that's
    /// already well-formed (no-op for nodes that already have `type`).
    pub fn fill_missing_types(schema: &mut Value) {
        let Value::Object(obj) = schema else { return };

        // First, fix this node if it looks like a schema node missing `type`.
        if !obj.contains_key("type") {
            if obj.contains_key("items") {
                obj.insert("type".to_string(), Value::String("array".to_string()));
            } else if obj.contains_key("properties") {
                obj.insert("type".to_string(), Value::String("object".to_string()));
            }
            // No `items` / `properties` either → don't guess; some providers
            // accept `{description: "..."}` as a free-form value.
        }

        // Recurse into properties.*
        if let Some(Value::Object(props)) = obj.get_mut("properties") {
            for (_, child) in props.iter_mut() {
                fill_missing_types(child);
            }
        }

        // Recurse into items (arrays).
        if let Some(items) = obj.get_mut("items") {
            fill_missing_types(items);
        }

        // Recurse into oneOf / anyOf / allOf variants.
        for key in &["oneOf", "anyOf", "allOf"] {
            if let Some(Value::Array(variants)) = obj.get_mut(*key) {
                for v in variants.iter_mut() {
                    fill_missing_types(v);
                }
            }
        }
    }

    /// Ensure `additionalProperties` is present on every `type: "object"`
    /// node (defaulting to the passed value). Used to set the default
    /// explicitly — some providers error if it's left implicit.
    pub fn force_additional_properties(schema: &mut Value, value: bool) {
        let Value::Object(obj) = schema else { return };

        let is_object = obj
            .get("type")
            .and_then(Value::as_str)
            .map(|s| s == "object")
            .unwrap_or(false);

        if is_object && !obj.contains_key("additionalProperties") {
            obj.insert(
                "additionalProperties".to_string(),
                Value::Bool(value),
            );
        }

        if let Some(Value::Object(props)) = obj.get_mut("properties") {
            for (_, child) in props.iter_mut() {
                force_additional_properties(child, value);
            }
        }
        if let Some(items) = obj.get_mut("items") {
            force_additional_properties(items, value);
        }
    }
}

// ---------------------------------------------------------------------------
// tool_id
// ---------------------------------------------------------------------------

/// Unified tool-use id handling. AMP's `KDR` + inverse.
///
/// Alva internally prefixes every tool-use id with `toolu_` so the agent
/// loop doesn't care whether the id came from Anthropic (already `toolu_…`),
/// OpenAI (bare `call_abc…`), or Gemini (no id at all — we generate one).
///
/// Adapters strip the prefix when sending `tool_result` back to the provider
/// whose native format doesn't use it.
pub mod tool_id {
    const PREFIX: &str = "toolu_";

    /// Add `toolu_` prefix if missing, sanitize to `[A-Za-z0-9_-]`.
    pub fn to_normalized(raw: &str) -> String {
        let sanitized: String = raw
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .collect();
        if sanitized.starts_with(PREFIX) {
            sanitized
        } else {
            format!("{PREFIX}{sanitized}")
        }
    }

    /// Strip the `toolu_` prefix to produce the provider-native id to echo
    /// back in `tool_result`. Idempotent for ids that were never prefixed.
    pub fn to_provider(normalized: &str) -> &str {
        normalized.strip_prefix(PREFIX).unwrap_or(normalized)
    }

    /// Generate a fresh normalized tool-use id (for providers like Gemini
    /// that don't emit one). Uses a random suffix.
    pub fn generate() -> String {
        let uuid = uuid::Uuid::new_v4().simple().to_string();
        format!("{PREFIX}{}", &uuid[..12])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_fix_fills_array_and_object() {
        let mut schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": { "items": { "type": "string" } },
                "meta": { "properties": { "x": { "type": "string" } } },
                "name": { "type": "string" }
            }
        });
        schema_fix::fill_missing_types(&mut schema);
        assert_eq!(schema["properties"]["tags"]["type"], "array");
        assert_eq!(schema["properties"]["meta"]["type"], "object");
        assert_eq!(schema["properties"]["name"]["type"], "string"); // untouched
    }

    #[test]
    fn tool_id_roundtrip() {
        let raw = "call_abc123";
        let normalized = tool_id::to_normalized(raw);
        assert_eq!(normalized, "toolu_call_abc123");
        assert_eq!(tool_id::to_provider(&normalized), "call_abc123");
    }

    #[test]
    fn tool_id_already_prefixed_is_idempotent() {
        let already = "toolu_01abc";
        let normalized = tool_id::to_normalized(already);
        assert_eq!(normalized, "toolu_01abc");
    }

    #[test]
    fn tool_id_sanitizes_bad_chars() {
        let bad = "call/abc:123";
        let normalized = tool_id::to_normalized(bad);
        assert_eq!(normalized, "toolu_call_abc_123");
    }

    #[test]
    fn tool_id_generate_is_prefixed() {
        let id = tool_id::generate();
        assert!(id.starts_with("toolu_"));
        assert!(id.len() > 6);
    }
}
