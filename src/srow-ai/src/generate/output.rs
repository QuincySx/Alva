use serde::de::DeserializeOwned;
use srow_core::error::ChatError;

/// Trait for parsing LLM output into typed values.
pub trait Output: Send + Sync {
    type Complete;
    type Partial: Clone;

    fn name(&self) -> &str;

    fn json_schema(&self) -> Option<serde_json::Value> {
        None
    }

    fn parse_complete(&self, text: &str) -> Result<Self::Complete, ChatError>;

    fn parse_partial(&self, text: &str) -> Option<Self::Partial>;
}

/// Plain text output — returns the raw string.
pub struct TextOutput;

impl Output for TextOutput {
    type Complete = String;
    type Partial = String;

    fn name(&self) -> &str {
        "text"
    }

    fn parse_complete(&self, text: &str) -> Result<String, ChatError> {
        Ok(text.to_string())
    }

    fn parse_partial(&self, text: &str) -> Option<String> {
        Some(text.to_string())
    }
}

/// Structured object output — parses JSON into a typed value.
pub struct ObjectOutput<T: DeserializeOwned + Clone + Send + Sync> {
    schema: Option<serde_json::Value>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: DeserializeOwned + Clone + Send + Sync> ObjectOutput<T> {
    pub fn new() -> Self {
        Self {
            schema: None,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.schema = Some(schema);
        self
    }
}

impl<T: DeserializeOwned + Clone + Send + Sync> Default for ObjectOutput<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: DeserializeOwned + Clone + Send + Sync + 'static> Output for ObjectOutput<T> {
    type Complete = T;
    type Partial = serde_json::Value;

    fn name(&self) -> &str {
        "object"
    }

    fn json_schema(&self) -> Option<serde_json::Value> {
        self.schema.clone()
    }

    fn parse_complete(&self, text: &str) -> Result<T, ChatError> {
        serde_json::from_str(text).map_err(|e| ChatError::Serialization(e.to_string()))
    }

    fn parse_partial(&self, text: &str) -> Option<serde_json::Value> {
        serde_json::from_str(text).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_output_parse_complete_returns_string() {
        let output = TextOutput;
        let result = output.parse_complete("hello").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn object_output_parse_complete_returns_value() {
        let output = ObjectOutput::<serde_json::Value>::new();
        let result = output.parse_complete(r#"{"a":1}"#).unwrap();
        assert_eq!(result, serde_json::json!({"a": 1}));
    }

    #[test]
    fn object_output_parse_partial_returns_none_for_invalid_json() {
        let output = ObjectOutput::<serde_json::Value>::new();
        assert!(output.parse_partial("invalid").is_none());
    }

    #[test]
    fn object_output_parse_partial_returns_some_for_valid_json() {
        let output = ObjectOutput::<serde_json::Value>::new();
        let result = output.parse_partial(r#"{"a":1}"#);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), serde_json::json!({"a": 1}));
    }
}
