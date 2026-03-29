// INPUT:  alva_types (Message, MessageRole, ContentBlock), async_trait,
//         super::super::middleware::{Middleware, MiddlewareError, MiddlewarePriority}, super::super::state::AgentState
// OUTPUT: DanglingToolCallMiddleware
// POS:    V2 dangling tool call fixer — same logic as v1 but adapted for v2 Middleware trait
//         that receives &mut AgentState instead of &mut MiddlewareContext.

use std::collections::HashSet;

use alva_types::{ContentBlock, Message, MessageRole};
use async_trait::async_trait;

use super::super::middleware::{Middleware, MiddlewareError, MiddlewarePriority};
use super::super::state::AgentState;

/// V2 middleware that fixes dangling tool calls on conversation resume.
///
/// When a conversation is interrupted mid-tool-execution, the message history
/// may contain Assistant messages with `ToolUse` blocks that have no
/// corresponding `Tool` message (containing a `ToolResult`). Most LLM APIs
/// reject such sequences.
///
/// This middleware scans messages in `before_llm_call` and inserts synthetic
/// `Tool` messages with `is_error=true` for any dangling tool calls.
pub struct DanglingToolCallMiddleware;

impl DanglingToolCallMiddleware {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DanglingToolCallMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for DanglingToolCallMiddleware {
    async fn before_llm_call(
        &self,
        _state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        // 1. Collect all tool_use IDs from Assistant messages
        let mut pending_tool_ids: Vec<String> = Vec::new();
        // 2. Collect all tool_result IDs from Tool messages
        let mut resolved_ids: HashSet<String> = HashSet::new();

        for msg in messages.iter() {
            match msg.role {
                MessageRole::Assistant => {
                    for block in &msg.content {
                        if let ContentBlock::ToolUse { id, .. } = block {
                            pending_tool_ids.push(id.clone());
                        }
                    }
                }
                MessageRole::Tool => {
                    // Tool messages have tool_call_id set, and may contain ToolResult blocks
                    if let Some(ref tc_id) = msg.tool_call_id {
                        resolved_ids.insert(tc_id.clone());
                    }
                    for block in &msg.content {
                        if let ContentBlock::ToolResult { id, .. } = block {
                            resolved_ids.insert(id.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        // 3. Find dangling IDs (in pending but not resolved)
        let dangling: Vec<String> = pending_tool_ids
            .into_iter()
            .filter(|id| !resolved_ids.contains(id))
            .collect();

        if dangling.is_empty() {
            return Ok(());
        }

        tracing::info!(
            count = dangling.len(),
            "dangling_tool_call: inserting synthetic ToolResult for interrupted tool calls"
        );

        // 4. Insert synthetic Tool messages right after the Assistant message
        //    that contains each dangling tool_use.
        //    We process from end to start so insertion indices stay valid.
        let mut insertions: Vec<(usize, Message)> = Vec::new();
        let dangling_set: HashSet<String> = dangling.into_iter().collect();

        for (msg_idx, msg) in messages.iter().enumerate() {
            if msg.role != MessageRole::Assistant {
                continue;
            }

            for block in &msg.content {
                if let ContentBlock::ToolUse { id, .. } = block {
                    if dangling_set.contains(id) {
                        let synthetic = Message {
                            id: format!("synthetic-{id}"),
                            role: MessageRole::Tool,
                            content: vec![ContentBlock::ToolResult {
                                id: id.clone(),
                                content:
                                    "[Tool call was interrupted and did not return a result.]"
                                        .to_string(),
                                is_error: true,
                            }],
                            tool_call_id: Some(id.clone()),
                            usage: None,
                            timestamp: msg.timestamp,
                        };
                        // Insert after this assistant message
                        insertions.push((msg_idx + 1, synthetic));
                    }
                }
            }
        }

        // Insert in reverse order to preserve indices
        insertions.sort_by(|a, b| b.0.cmp(&a.0));
        for (idx, msg) in insertions {
            messages.insert(idx, msg);
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "dangling_tool_call"
    }

    fn priority(&self) -> i32 {
        MiddlewarePriority::SECURITY + 1
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::session::InMemorySession;
    use crate::middleware::Extensions;
    use std::sync::Arc;

    // -- helpers --

    fn make_state() -> AgentState {
        use alva_types::base::error::AgentError;
        use alva_types::base::message::Message;
        use alva_types::base::stream::StreamEvent;
        use alva_types::model::LanguageModel;
        use alva_types::tool::Tool;
        use alva_types::ModelConfig;

        struct StubModel;
        #[async_trait]
        impl LanguageModel for StubModel {
            async fn complete(
                &self,
                _: &[Message],
                _: &[&dyn Tool],
                _: &ModelConfig,
            ) -> Result<Message, AgentError> {
                unreachable!()
            }
            fn stream(
                &self,
                _: &[Message],
                _: &[&dyn Tool],
                _: &ModelConfig,
            ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
                Box::pin(futures::stream::empty())
            }
            fn model_id(&self) -> &str {
                "stub"
            }
        }

        AgentState {
            model: Arc::new(StubModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        }
    }

    fn assistant_msg_with_tool_use(tool_id: &str, tool_name: &str) -> Message {
        Message {
            id: format!("asst-{tool_id}"),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "I'll use a tool.".to_string(),
                },
                ContentBlock::ToolUse {
                    id: tool_id.to_string(),
                    name: tool_name.to_string(),
                    input: serde_json::json!({"arg": "value"}),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 1000,
        }
    }

    fn tool_result_msg(tool_id: &str, content: &str) -> Message {
        Message {
            id: format!("tool-{tool_id}"),
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                id: tool_id.to_string(),
                content: content.to_string(),
                is_error: false,
            }],
            tool_call_id: Some(tool_id.to_string()),
            usage: None,
            timestamp: 1001,
        }
    }

    // -- tests --

    #[tokio::test]
    async fn test_no_dangling_no_change() {
        let mw = DanglingToolCallMiddleware::new();
        let mut state = make_state();

        let mut messages = vec![
            Message::system("system"),
            Message::user("do something"),
            assistant_msg_with_tool_use("tc_1", "grep"),
            tool_result_msg("tc_1", "found it"),
        ];
        let original_len = messages.len();

        mw.before_llm_call(&mut state, &mut messages).await.unwrap();
        assert_eq!(messages.len(), original_len);
    }

    #[tokio::test]
    async fn test_dangling_tool_call_gets_synthetic_result() {
        let mw = DanglingToolCallMiddleware::new();
        let mut state = make_state();

        let mut messages = vec![
            Message::system("system"),
            Message::user("do something"),
            assistant_msg_with_tool_use("tc_1", "grep"),
            // No tool result for tc_1 — this is dangling
        ];

        mw.before_llm_call(&mut state, &mut messages).await.unwrap();

        // Should have inserted a synthetic Tool message
        assert_eq!(messages.len(), 4);
        let synthetic = &messages[3];
        assert_eq!(synthetic.role, MessageRole::Tool);
        assert_eq!(synthetic.tool_call_id, Some("tc_1".to_string()));

        // Check the content
        let (id, content, is_error) = synthetic.content[0].as_tool_result().unwrap();
        assert_eq!(id, "tc_1");
        assert!(content.contains("interrupted"));
        assert!(is_error);
    }

    #[tokio::test]
    async fn test_multiple_dangling_tool_calls() {
        let mw = DanglingToolCallMiddleware::new();
        let mut state = make_state();

        let mut messages = vec![
            Message::system("system"),
            Message::user("do two things"),
            // Assistant with two tool uses
            Message {
                id: "asst-multi".to_string(),
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::ToolUse {
                        id: "tc_1".to_string(),
                        name: "grep".to_string(),
                        input: serde_json::json!({}),
                    },
                    ContentBlock::ToolUse {
                        id: "tc_2".to_string(),
                        name: "read".to_string(),
                        input: serde_json::json!({}),
                    },
                ],
                tool_call_id: None,
                usage: None,
                timestamp: 1000,
            },
            // Only tc_1 has a result
            tool_result_msg("tc_1", "result for tc_1"),
        ];

        mw.before_llm_call(&mut state, &mut messages).await.unwrap();

        // Should have 5 messages (original 4 + 1 synthetic for tc_2)
        assert_eq!(messages.len(), 5);

        // Find the synthetic message
        let synthetic_msgs: Vec<&Message> = messages
            .iter()
            .filter(|m| m.id.starts_with("synthetic-"))
            .collect();
        assert_eq!(synthetic_msgs.len(), 1);
        assert_eq!(
            synthetic_msgs[0].tool_call_id,
            Some("tc_2".to_string())
        );
    }

    #[tokio::test]
    async fn test_no_tool_calls_no_change() {
        let mw = DanglingToolCallMiddleware::new();
        let mut state = make_state();

        let mut messages = vec![
            Message::system("system"),
            Message::user("hello"),
        ];
        let original_len = messages.len();

        mw.before_llm_call(&mut state, &mut messages).await.unwrap();
        assert_eq!(messages.len(), original_len);
    }

    #[tokio::test]
    async fn test_multi_assistant_sequences() {
        let mw = DanglingToolCallMiddleware::new();
        let mut state = make_state();

        let mut messages = vec![
            Message::system("system"),
            // First assistant + result (complete)
            assistant_msg_with_tool_use("tc_1", "grep"),
            tool_result_msg("tc_1", "ok"),
            // Second assistant, dangling
            assistant_msg_with_tool_use("tc_2", "read"),
            // Third assistant, also dangling
            assistant_msg_with_tool_use("tc_3", "write"),
        ];

        mw.before_llm_call(&mut state, &mut messages).await.unwrap();

        // Original 5 + 2 synthetic = 7
        assert_eq!(messages.len(), 7);

        let synthetic_ids: Vec<String> = messages
            .iter()
            .filter(|m| m.id.starts_with("synthetic-"))
            .filter_map(|m| m.tool_call_id.clone())
            .collect();
        assert!(synthetic_ids.contains(&"tc_2".to_string()));
        assert!(synthetic_ids.contains(&"tc_3".to_string()));
    }

    #[tokio::test]
    async fn test_priority() {
        let mw = DanglingToolCallMiddleware::new();
        assert_eq!(mw.priority(), MiddlewarePriority::SECURITY + 1);
    }

    #[tokio::test]
    async fn test_name() {
        let mw = DanglingToolCallMiddleware::new();
        assert_eq!(mw.name(), "dangling_tool_call");
    }
}
