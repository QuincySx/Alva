// INPUT:  schemars, serde_json::Value
// OUTPUT: normalize_llm_tool_schema
// POS:    Normalize schemars-derived JSON Schema to LLM tool-calling spec.

//! Transforms schemars-generated JSON Schema so it matches what LLM
//! tool-calling APIs (OpenAI function calling, Anthropic tools,
//! Gemini function declarations) actually expect.
//!
//! The transform is the core building block of the
//! `#[derive(Tool)]` macro: every derived tool's
//! `parameters_schema()` method calls `schemars::schema_for!` on its
//! input struct, then pipes the result through
//! [`normalize_llm_tool_schema`] before returning.
//!
//! Kept as a free function (not a trait method) so:
//! - The proc macro in `alva-macros` can reference it by absolute path
//!   without touching the `Tool` trait.
//! - It's trivially unit-testable with hand-written JSON fixtures.
//! - Anyone writing a hand-rolled `Tool` impl can opt in to the same
//!   normalization without going through the derive.

use serde_json::Value;

/// Normalize a `schemars`-generated JSON Schema in-place so it matches
/// the shape LLM tool-calling APIs expect.
///
/// The transformations are:
///
/// 1. **Strip root meta fields** — `$schema`, `title`, root-level
///    `description`. The tool's `Tool::description()` already carries
///    the human-facing prose; the LLM doesn't consume `$schema`/`title`.
///
/// 2. **Strip `default` from properties** — harmless but bloat. LLM
///    tool-calling APIs ignore `default`; the field's defaulted-ness
///    is already encoded in its absence from `required`.
///
/// 3. **Collapse `type: ["T", "null"]` → `type: "T"`** on properties —
///    schemars emits the union form for `Option<T>` fields, but LLM
///    tool APIs prefer the simpler form. The field's optionality is
///    conveyed by absence from `required`, not by a null union.
///
/// # Example
///
/// ```
/// use serde_json::json;
/// use alva_types::tool::schema::normalize_llm_tool_schema;
///
/// let mut schema = json!({
///     "$schema": "https://json-schema.org/draft/2020-12/schema",
///     "title": "Foo",
///     "type": "object",
///     "properties": {
///         "name": { "type": ["string", "null"], "default": null }
///     }
/// });
/// normalize_llm_tool_schema(&mut schema);
/// assert!(schema.get("title").is_none());
/// assert_eq!(schema["properties"]["name"]["type"], "string");
/// assert!(schema["properties"]["name"].get("default").is_none());
/// ```
pub fn normalize_llm_tool_schema(schema: &mut Value) {
    // Root-level cleanups.
    if let Some(obj) = schema.as_object_mut() {
        obj.remove("$schema");
        obj.remove("title");
        obj.remove("description");
    }

    // Per-property cleanups.
    if let Some(props) = schema
        .pointer_mut("/properties")
        .and_then(Value::as_object_mut)
    {
        for (_name, prop) in props.iter_mut() {
            let Some(prop_obj) = prop.as_object_mut() else {
                continue;
            };
            prop_obj.remove("default");

            // Collapse `type: ["T", "null"]` → `type: "T"`.
            if let Some(Value::Array(types)) = prop_obj.get("type").cloned() {
                let non_null: Vec<Value> = types
                    .into_iter()
                    .filter(|t| t.as_str() != Some("null"))
                    .collect();
                if non_null.len() == 1 {
                    prop_obj.insert("type".into(), non_null.into_iter().next().unwrap());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_root_meta() {
        let mut schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Foo",
            "description": "struct-level doc",
            "type": "object",
            "properties": {}
        });
        normalize_llm_tool_schema(&mut schema);
        assert!(schema.get("$schema").is_none());
        assert!(schema.get("title").is_none());
        assert!(schema.get("description").is_none());
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn collapses_option_union() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": ["string", "null"] },
                "count": { "type": "integer" }
            }
        });
        normalize_llm_tool_schema(&mut schema);
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["count"]["type"], "integer");
    }

    #[test]
    fn strips_property_defaults() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "foo": { "type": "string", "default": "hi" },
                "bar": { "type": "array", "default": [] }
            }
        });
        normalize_llm_tool_schema(&mut schema);
        assert!(schema["properties"]["foo"].get("default").is_none());
        assert!(schema["properties"]["bar"].get("default").is_none());
    }

    #[test]
    fn leaves_multi_type_unions_alone() {
        // A union with more than one non-null type isn't an `Option<T>`,
        // so we don't collapse it.
        let mut schema = json!({
            "type": "object",
            "properties": {
                "either": { "type": ["string", "integer"] }
            }
        });
        normalize_llm_tool_schema(&mut schema);
        assert_eq!(
            schema["properties"]["either"]["type"],
            json!(["string", "integer"])
        );
    }
}
