//! MCP Elicitation types.
//!
//! Elicitation allows MCP servers to request structured information from
//! the client (user) during a tool execution or other interaction. This
//! enables interactive workflows where the server needs additional input.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCP elicitation request (server asking client for information).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationRequest {
    /// Human-readable message explaining what information is needed.
    pub message: String,
    /// JSON Schema describing the expected response format.
    pub requested_schema: serde_json::Value,
}

/// Elicitation response from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResponse {
    /// The action taken by the user.
    pub action: ElicitationAction,
    /// The response content (present when action is Accept).
    pub content: Option<HashMap<String, serde_json::Value>>,
}

/// User's action in response to an elicitation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationAction {
    /// User provided the requested information.
    Accept,
    /// User declined to provide information.
    Decline,
    /// User cancelled the operation.
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elicitation_request_serde_roundtrip() {
        let request = ElicitationRequest {
            message: "Please provide your API key".into(),
            requested_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "api_key": { "type": "string" }
                },
                "required": ["api_key"]
            }),
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: ElicitationRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.message, "Please provide your API key");
        assert!(parsed.requested_schema["properties"]["api_key"]["type"]
            .as_str()
            .unwrap()
            == "string");
    }

    #[test]
    fn elicitation_response_accept_with_content() {
        let response = ElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(HashMap::from([(
                "api_key".to_string(),
                serde_json::json!("sk-12345"),
            )])),
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: ElicitationResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.action, ElicitationAction::Accept);
        assert_eq!(
            parsed.content.unwrap()["api_key"],
            serde_json::json!("sk-12345")
        );
    }

    #[test]
    fn elicitation_response_decline_no_content() {
        let response = ElicitationResponse {
            action: ElicitationAction::Decline,
            content: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: ElicitationResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.action, ElicitationAction::Decline);
        assert!(parsed.content.is_none());
    }

    #[test]
    fn elicitation_response_cancel() {
        let response = ElicitationResponse {
            action: ElicitationAction::Cancel,
            content: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: ElicitationResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.action, ElicitationAction::Cancel);
    }

    #[test]
    fn elicitation_action_serde_values() {
        let accept_json = serde_json::to_string(&ElicitationAction::Accept).unwrap();
        assert_eq!(accept_json, "\"accept\"");

        let decline_json = serde_json::to_string(&ElicitationAction::Decline).unwrap();
        assert_eq!(decline_json, "\"decline\"");

        let cancel_json = serde_json::to_string(&ElicitationAction::Cancel).unwrap();
        assert_eq!(cancel_json, "\"cancel\"");
    }

    #[test]
    fn elicitation_response_with_multiple_fields() {
        let response = ElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(HashMap::from([
                ("name".to_string(), serde_json::json!("Alice")),
                ("age".to_string(), serde_json::json!(30)),
                ("confirm".to_string(), serde_json::json!(true)),
            ])),
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: ElicitationResponse = serde_json::from_str(&json).unwrap();

        let content = parsed.content.unwrap();
        assert_eq!(content["name"], serde_json::json!("Alice"));
        assert_eq!(content["age"], serde_json::json!(30));
        assert_eq!(content["confirm"], serde_json::json!(true));
    }
}
