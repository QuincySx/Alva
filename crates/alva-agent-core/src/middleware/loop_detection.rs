// INPUT:  alva_types (Message, ContentBlock), async_trait, std::sync::Mutex, std::collections::HashMap,
//         std::collections::hash_map::DefaultHasher, super::{Middleware, MiddlewareContext, MiddlewareError, MiddlewarePriority}
// OUTPUT: LoopDetectionMiddleware
// POS:    Detects repetitive tool-call loops by hashing tool_calls per turn, tracking a sliding
//         window per session, and intervening when thresholds are reached.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use alva_types::{ContentBlock, Message};
use async_trait::async_trait;

use super::{Middleware, MiddlewareContext, MiddlewareError, MiddlewarePriority};

/// Middleware that detects and breaks repetitive tool-call loops.
///
/// Hashes tool_calls from each LLM response using a sliding window per session.
/// When the same hash appears repeatedly:
/// - `warn_threshold` (default 3): logs a warning
/// - `hard_limit` (default 5): strips all tool_calls from the response, forcing text-only output
pub struct LoopDetectionMiddleware {
    warn_threshold: u32,
    hard_limit: u32,
    window_size: usize,
    history: Mutex<HashMap<String, Vec<String>>>,
}

impl LoopDetectionMiddleware {
    /// Create with default thresholds: warn at 3, hard limit at 5, window of 20.
    pub fn new() -> Self {
        Self {
            warn_threshold: 3,
            hard_limit: 5,
            window_size: 20,
            history: Mutex::new(HashMap::new()),
        }
    }

    /// Create with custom thresholds.
    pub fn with_thresholds(warn_threshold: u32, hard_limit: u32, window_size: usize) -> Self {
        Self {
            warn_threshold,
            hard_limit,
            window_size,
            history: Mutex::new(HashMap::new()),
        }
    }

    /// Hash the tool_calls from a message.
    ///
    /// Sorts tool-use blocks by (name, input_json) and hashes them.
    /// Returns the first 12 hex chars of the hash.
    fn hash_tool_calls(response: &Message) -> String {
        let mut tool_uses: Vec<(String, String)> = response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { name, input, .. } => {
                    Some((name.clone(), input.to_string()))
                }
                _ => None,
            })
            .collect();

        tool_uses.sort();

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for (name, args) in &tool_uses {
            name.hash(&mut hasher);
            args.hash(&mut hasher);
        }
        let hash_value = hasher.finish();
        format!("{:012x}", hash_value)[..12].to_string()
    }

    /// Count occurrences of `hash` in the history window.
    fn count_hash(history: &[String], hash: &str) -> u32 {
        history.iter().filter(|h| h.as_str() == hash).count() as u32
    }
}

impl Default for LoopDetectionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for LoopDetectionMiddleware {
    async fn after_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        // Only process responses that contain tool_calls
        if !response.has_tool_calls() {
            return Ok(());
        }

        let hash = Self::hash_tool_calls(response);
        let session_id = ctx.session_id.clone();

        let count = {
            let mut history_map = self.history.lock().unwrap();
            let session_history = history_map.entry(session_id).or_default();

            // Append and trim to window size
            session_history.push(hash.clone());
            if session_history.len() > self.window_size {
                let excess = session_history.len() - self.window_size;
                session_history.drain(..excess);
            }

            Self::count_hash(session_history, &hash)
        };

        if count >= self.hard_limit {
            // Strip all tool_calls from the response, forcing text-only output
            tracing::warn!(
                count,
                hash = %hash,
                "loop detection: hard limit reached, stripping tool_calls"
            );

            response.content.retain(|block| !block.is_tool_use());
            response.content.push(ContentBlock::Text {
                text: "[Loop detected: repeated identical tool calls were stripped. \
                       Please respond with text only or try a different approach.]"
                    .to_string(),
            });
        } else if count >= self.warn_threshold {
            tracing::warn!(
                count,
                hash = %hash,
                "loop detection: repetitive tool_calls detected (warn threshold)"
            );
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "loop_detection"
    }

    fn priority(&self) -> i32 {
        MiddlewarePriority::GUARDRAIL + 1
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::MessageRole;
    use crate::middleware::Extensions;

    fn test_ctx() -> MiddlewareContext {
        MiddlewareContext {
            session_id: "test-session".to_string(),
            system_prompt: "test".to_string(),
            messages: Vec::new(),
            extensions: Extensions::new(),
        }
    }

    fn assistant_with_tool_calls(tool_calls: Vec<(&str, serde_json::Value)>) -> Message {
        let mut content: Vec<ContentBlock> = tool_calls
            .into_iter()
            .enumerate()
            .map(|(i, (name, input))| ContentBlock::ToolUse {
                id: format!("tc_{i}"),
                name: name.to_string(),
                input,
            })
            .collect();
        content.insert(
            0,
            ContentBlock::Text {
                text: "Let me do this.".to_string(),
            },
        );

        Message {
            id: "msg-1".to_string(),
            role: MessageRole::Assistant,
            content,
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }
    }

    fn text_only_response() -> Message {
        Message {
            id: "msg-text".to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: "Just a text response.".to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn test_no_tool_calls_passes_through() {
        let mw = LoopDetectionMiddleware::new();
        let mut ctx = test_ctx();
        let mut response = text_only_response();
        let original_text = response.text_content();

        mw.after_llm_call(&mut ctx, &mut response).await.unwrap();

        assert_eq!(response.text_content(), original_text);
        assert!(!response.has_tool_calls());
    }

    #[tokio::test]
    async fn test_below_warn_threshold_no_change() {
        let mw = LoopDetectionMiddleware::with_thresholds(3, 5, 20);
        let mut ctx = test_ctx();

        // First two identical calls: no action
        for _ in 0..2 {
            let mut response = assistant_with_tool_calls(vec![
                ("grep", serde_json::json!({"pattern": "foo"})),
            ]);
            mw.after_llm_call(&mut ctx, &mut response).await.unwrap();
            assert!(response.has_tool_calls(), "tool_calls should still be present");
        }
    }

    #[tokio::test]
    async fn test_warn_threshold_keeps_tool_calls() {
        let mw = LoopDetectionMiddleware::with_thresholds(3, 5, 20);
        let mut ctx = test_ctx();

        // Reach warn threshold (3 identical calls)
        for _ in 0..3 {
            let mut response = assistant_with_tool_calls(vec![
                ("grep", serde_json::json!({"pattern": "foo"})),
            ]);
            mw.after_llm_call(&mut ctx, &mut response).await.unwrap();
        }
        // At warn threshold, tool_calls are still present (only a log warning)
        let mut response = assistant_with_tool_calls(vec![
            ("grep", serde_json::json!({"pattern": "foo"})),
        ]);
        mw.after_llm_call(&mut ctx, &mut response).await.unwrap();
        // 4th call: above warn but below hard limit
        assert!(response.has_tool_calls());
    }

    #[tokio::test]
    async fn test_hard_limit_strips_tool_calls() {
        let mw = LoopDetectionMiddleware::with_thresholds(3, 5, 20);
        let mut ctx = test_ctx();

        // First 4 identical calls
        for _ in 0..4 {
            let mut response = assistant_with_tool_calls(vec![
                ("grep", serde_json::json!({"pattern": "foo"})),
            ]);
            mw.after_llm_call(&mut ctx, &mut response).await.unwrap();
        }

        // 5th identical call: hard limit reached
        let mut response = assistant_with_tool_calls(vec![
            ("grep", serde_json::json!({"pattern": "foo"})),
        ]);
        mw.after_llm_call(&mut ctx, &mut response).await.unwrap();

        assert!(!response.has_tool_calls(), "tool_calls should be stripped");
        let text = response.text_content();
        assert!(text.contains("Loop detected"), "should contain loop warning, got: {text}");
    }

    #[tokio::test]
    async fn test_different_tool_calls_no_loop() {
        let mw = LoopDetectionMiddleware::with_thresholds(3, 5, 20);
        let mut ctx = test_ctx();

        // Different tool calls each time
        for i in 0..10 {
            let mut response = assistant_with_tool_calls(vec![
                ("grep", serde_json::json!({"pattern": format!("query_{i}")})),
            ]);
            mw.after_llm_call(&mut ctx, &mut response).await.unwrap();
            assert!(response.has_tool_calls(), "tool_calls should remain for unique calls");
        }
    }

    #[tokio::test]
    async fn test_separate_sessions_independent() {
        let mw = LoopDetectionMiddleware::with_thresholds(2, 3, 20);

        let tool_calls = vec![("grep", serde_json::json!({"pattern": "foo"}))];

        // Session A: 2 calls (reaches warn)
        let mut ctx_a = MiddlewareContext {
            session_id: "session-a".to_string(),
            system_prompt: "test".to_string(),
            messages: Vec::new(),
            extensions: Extensions::new(),
        };
        for _ in 0..2 {
            let mut response = assistant_with_tool_calls(tool_calls.clone());
            mw.after_llm_call(&mut ctx_a, &mut response).await.unwrap();
        }

        // Session B: 1 call (below warn)
        let mut ctx_b = MiddlewareContext {
            session_id: "session-b".to_string(),
            system_prompt: "test".to_string(),
            messages: Vec::new(),
            extensions: Extensions::new(),
        };
        let mut response_b = assistant_with_tool_calls(tool_calls.clone());
        mw.after_llm_call(&mut ctx_b, &mut response_b).await.unwrap();

        // Session B should not be affected by session A's history
        assert!(response_b.has_tool_calls());
    }

    #[tokio::test]
    async fn test_sliding_window_evicts_old_hashes() {
        let mw = LoopDetectionMiddleware::with_thresholds(3, 5, 5);
        let mut ctx = test_ctx();

        let repeated = vec![("grep", serde_json::json!({"pattern": "same"}))];

        // Add 2 identical calls
        for _ in 0..2 {
            let mut response = assistant_with_tool_calls(repeated.clone());
            mw.after_llm_call(&mut ctx, &mut response).await.unwrap();
        }

        // Add 5 different calls to push the old ones out of the window
        for i in 0..5 {
            let mut response = assistant_with_tool_calls(vec![
                ("read", serde_json::json!({"file": format!("file_{i}")})),
            ]);
            mw.after_llm_call(&mut ctx, &mut response).await.unwrap();
        }

        // Now add the same repeated call again — should only count 1 in window
        let mut response = assistant_with_tool_calls(repeated.clone());
        mw.after_llm_call(&mut ctx, &mut response).await.unwrap();
        assert!(response.has_tool_calls(), "old hashes should have been evicted");
    }

    #[tokio::test]
    async fn test_priority() {
        let mw = LoopDetectionMiddleware::new();
        assert_eq!(mw.priority(), MiddlewarePriority::GUARDRAIL + 1);
    }

    #[tokio::test]
    async fn test_name() {
        let mw = LoopDetectionMiddleware::new();
        assert_eq!(mw.name(), "loop_detection");
    }

    #[tokio::test]
    async fn test_hash_stability() {
        // Same tool calls should produce the same hash regardless of content block order
        // (because we sort by name + input)
        let msg1 = assistant_with_tool_calls(vec![
            ("alpha", serde_json::json!({"x": 1})),
            ("beta", serde_json::json!({"y": 2})),
        ]);
        let msg2 = assistant_with_tool_calls(vec![
            ("beta", serde_json::json!({"y": 2})),
            ("alpha", serde_json::json!({"x": 1})),
        ]);

        let hash1 = LoopDetectionMiddleware::hash_tool_calls(&msg1);
        let hash2 = LoopDetectionMiddleware::hash_tool_calls(&msg2);
        assert_eq!(hash1, hash2, "hash should be order-independent");
    }
}
