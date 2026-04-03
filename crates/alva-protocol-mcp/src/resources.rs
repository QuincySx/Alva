//! MCP Resource types.
//!
//! Resources represent data that MCP servers expose to clients,
//! such as files, database records, or API responses.

use serde::{Deserialize, Serialize};

/// An MCP resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    /// Resource URI (e.g., "file:///path/to/file")
    pub uri: String,
    /// Human-readable name
    pub name: String,
    /// Optional description
    pub description: Option<String>,
    /// MIME type
    pub mime_type: Option<String>,
}

/// Resource contents returned by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceContent {
    pub uri: String,
    pub mime_type: Option<String>,
    pub text: Option<String>,
    /// Base64 encoded binary content.
    pub blob: Option<String>,
}

/// Resource template for dynamic resources.
///
/// Templates use URI Template syntax (RFC 6570) to define parameterized
/// resource URIs, allowing clients to construct valid resource URIs
/// dynamically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceTemplate {
    pub uri_template: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_serde_roundtrip() {
        let resource = McpResource {
            uri: "file:///tmp/test.txt".into(),
            name: "Test File".into(),
            description: Some("A test file".into()),
            mime_type: Some("text/plain".into()),
        };

        let json = serde_json::to_string(&resource).unwrap();
        let parsed: McpResource = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.uri, "file:///tmp/test.txt");
        assert_eq!(parsed.name, "Test File");
        assert_eq!(parsed.description.as_deref(), Some("A test file"));
        assert_eq!(parsed.mime_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn resource_optional_fields_none() {
        let resource = McpResource {
            uri: "file:///data".into(),
            name: "Data".into(),
            description: None,
            mime_type: None,
        };

        let json = serde_json::to_string(&resource).unwrap();
        let parsed: McpResource = serde_json::from_str(&json).unwrap();
        assert!(parsed.description.is_none());
        assert!(parsed.mime_type.is_none());
    }

    #[test]
    fn resource_content_with_text() {
        let content = McpResourceContent {
            uri: "file:///test.txt".into(),
            mime_type: Some("text/plain".into()),
            text: Some("Hello, world!".into()),
            blob: None,
        };

        let json = serde_json::to_string(&content).unwrap();
        let parsed: McpResourceContent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.text.as_deref(), Some("Hello, world!"));
        assert!(parsed.blob.is_none());
    }

    #[test]
    fn resource_content_with_blob() {
        let content = McpResourceContent {
            uri: "file:///image.png".into(),
            mime_type: Some("image/png".into()),
            text: None,
            blob: Some("aGVsbG8=".into()),
        };

        let json = serde_json::to_string(&content).unwrap();
        let parsed: McpResourceContent = serde_json::from_str(&json).unwrap();
        assert!(parsed.text.is_none());
        assert_eq!(parsed.blob.as_deref(), Some("aGVsbG8="));
    }

    #[test]
    fn resource_template_serde_roundtrip() {
        let template = McpResourceTemplate {
            uri_template: "file:///data/{id}.json".into(),
            name: "Data Record".into(),
            description: Some("A data record by ID".into()),
            mime_type: Some("application/json".into()),
        };

        let json = serde_json::to_string(&template).unwrap();
        let parsed: McpResourceTemplate = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.uri_template, "file:///data/{id}.json");
        assert_eq!(parsed.name, "Data Record");
        assert_eq!(parsed.description.as_deref(), Some("A data record by ID"));
    }
}
