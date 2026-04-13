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
/// 2. **Inline `$defs` + `$ref`** — schemars emits nested struct types
///    as a top-level `$defs` map plus `$ref` pointers. LLM tool-calling
///    APIs either don't support `$ref` or support it inconsistently,
///    so we replace every `$ref: "#/$defs/Name"` with a deep copy of
///    `$defs[Name]` and then delete `$defs` from the root.
///
/// 3. **Strip `default`** from every object node — harmless but bloat.
///    LLM tool-calling APIs ignore `default`; the field's defaulted-ness
///    is already encoded in its absence from `required`.
///
/// 4. **Collapse `type: ["T", "null"]` → `type: "T"`** everywhere —
///    schemars emits the union form for `Option<T>` fields, but LLM
///    tool APIs prefer the simpler form. The field's optionality is
///    conveyed by absence from `required`, not by a null union.
///
/// Transforms 3 and 4 are applied recursively to every object node in
/// the tree (not just the root `properties`), so they take effect on
/// inlined subschemas as well as flat top-level fields.
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
    // Extract `$defs` from the root before walking so child `$ref`
    // lookups can find them. Removing it now also ensures the final
    // schema has no leftover `$defs` key regardless of whether any
    // `$ref` actually resolved.
    let defs = schema
        .as_object_mut()
        .and_then(|obj| obj.remove("$defs"))
        .and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();

    // Root-level cleanups.
    if let Some(obj) = schema.as_object_mut() {
        obj.remove("$schema");
        obj.remove("title");
        obj.remove("description");
    }

    // Recursive transform: inline `$ref`s, strip `default`, collapse
    // `Option<T>` type unions. Applied everywhere, not just root props.
    normalize_node(schema, &defs);
}

/// Recursive walker — applied to every object/array node in the
/// schema tree.
fn normalize_node(node: &mut Value, defs: &serde_json::Map<String, Value>) {
    match node {
        Value::Object(obj) => {
            // If this object is a `$ref`, replace the whole node with
            // the deref target (a deep clone) and recurse into the
            // new node so its nested `$ref`s get inlined too.
            if let Some(Value::String(ref_path)) = obj.get("$ref").cloned() {
                if let Some(name) = ref_path.strip_prefix("#/$defs/") {
                    if let Some(target) = defs.get(name).cloned() {
                        *node = target;
                        normalize_node(node, defs);
                        return;
                    }
                }
            }

            // Non-$ref object: clean up this level, then descend.
            obj.remove("default");

            // Collapse `type: ["T", "null"]` → `type: "T"`.
            if let Some(Value::Array(types)) = obj.get("type").cloned() {
                let non_null: Vec<Value> = types
                    .into_iter()
                    .filter(|t| t.as_str() != Some("null"))
                    .collect();
                if non_null.len() == 1 {
                    obj.insert("type".into(), non_null.into_iter().next().unwrap());
                }
            }

            for (_, v) in obj.iter_mut() {
                normalize_node(v, defs);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_node(v, defs);
            }
        }
        _ => {}
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

    #[test]
    fn inlines_refs_and_removes_defs() {
        // Schemars-style output with a nested struct type in `$defs`.
        let mut schema = json!({
            "$defs": {
                "Option": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" },
                        "value": { "type": ["string", "null"], "default": null }
                    },
                    "required": ["label"]
                }
            },
            "type": "object",
            "properties": {
                "options": {
                    "type": "array",
                    "items": { "$ref": "#/$defs/Option" }
                }
            },
            "required": []
        });
        normalize_llm_tool_schema(&mut schema);

        // $defs gone
        assert!(schema.get("$defs").is_none());
        // items now carries the inlined Option schema
        let items = &schema["properties"]["options"]["items"];
        assert_eq!(items["type"], "object");
        assert_eq!(items["required"], json!(["label"]));
        // Nested normalization also applied to the inlined content:
        // - `default: null` stripped
        // - `type: ["string","null"]` collapsed to `type: "string"`
        assert_eq!(items["properties"]["label"]["type"], "string");
        assert_eq!(items["properties"]["value"]["type"], "string");
        assert!(items["properties"]["value"].get("default").is_none());
    }

    #[test]
    fn inlines_refs_recursively() {
        // `$defs/A` contains a `$ref` to `$defs/B`. Both should inline.
        let mut schema = json!({
            "$defs": {
                "Inner": { "type": "object", "properties": { "x": { "type": "integer" } } },
                "Outer": {
                    "type": "object",
                    "properties": { "inner": { "$ref": "#/$defs/Inner" } }
                }
            },
            "type": "object",
            "properties": {
                "node": { "$ref": "#/$defs/Outer" }
            }
        });
        normalize_llm_tool_schema(&mut schema);

        assert!(schema.get("$defs").is_none());
        let node = &schema["properties"]["node"];
        assert_eq!(node["type"], "object");
        let inner = &node["properties"]["inner"];
        assert_eq!(inner["type"], "object");
        assert_eq!(inner["properties"]["x"]["type"], "integer");
    }
}
