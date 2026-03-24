use alva_agent_core::AgentMessage;

/// A single transformation over the agent's message context.
///
/// Transforms are applied in order by [`TransformPipeline`] before the
/// context is sent to the model. Common uses include filtering, redacting,
/// summarising, or rewriting messages.
pub trait ContextTransform: Send + Sync {
    /// Transform the message slice, returning a new (potentially modified)
    /// message list.
    fn transform(&self, messages: &[AgentMessage]) -> Vec<AgentMessage>;
}

/// An ordered pipeline of context transforms.
///
/// Transforms execute sequentially — the output of one becomes the input
/// of the next.
pub struct TransformPipeline {
    transforms: Vec<Box<dyn ContextTransform>>,
}

impl TransformPipeline {
    pub fn new() -> Self {
        Self {
            transforms: vec![],
        }
    }

    /// Append a transform to the end of the pipeline.
    pub fn push(&mut self, transform: Box<dyn ContextTransform>) {
        self.transforms.push(transform);
    }

    /// Apply all transforms in order, returning the final message list.
    pub fn apply(&self, messages: &[AgentMessage]) -> Vec<AgentMessage> {
        let mut result = messages.to_vec();
        for transform in &self.transforms {
            result = transform.transform(&result);
        }
        result
    }
}

impl Default for TransformPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::Message;

    /// Test transform that appends a marker message.
    struct AppendMarker {
        marker: String,
    }

    impl ContextTransform for AppendMarker {
        fn transform(&self, messages: &[AgentMessage]) -> Vec<AgentMessage> {
            let mut out = messages.to_vec();
            out.push(AgentMessage::Standard(Message::user(&self.marker)));
            out
        }
    }

    /// Test transform that keeps only messages matching a predicate.
    struct KeepOnlyCustom;

    impl ContextTransform for KeepOnlyCustom {
        fn transform(&self, messages: &[AgentMessage]) -> Vec<AgentMessage> {
            messages
                .iter()
                .filter(|m| matches!(m, AgentMessage::Custom { .. }))
                .cloned()
                .collect()
        }
    }

    #[test]
    fn single_transform() {
        let mut pipeline = TransformPipeline::new();
        pipeline.push(Box::new(AppendMarker {
            marker: "ADDED".into(),
        }));

        let messages = vec![AgentMessage::Standard(Message::user("hello"))];
        let result = pipeline.apply(&messages);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn chained_transforms_execute_in_order() {
        let mut pipeline = TransformPipeline::new();

        // First: append marker "A"
        pipeline.push(Box::new(AppendMarker {
            marker: "A".into(),
        }));
        // Second: append marker "B"
        pipeline.push(Box::new(AppendMarker {
            marker: "B".into(),
        }));

        let messages: Vec<AgentMessage> = vec![];
        let result = pipeline.apply(&messages);

        // [] -> [A] -> [A, B]
        assert_eq!(result.len(), 2);

        // Verify ordering by checking content
        match &result[0] {
            AgentMessage::Standard(msg) => assert_eq!(msg.text_content(), "A"),
            _ => panic!("expected Standard message"),
        }
        match &result[1] {
            AgentMessage::Standard(msg) => assert_eq!(msg.text_content(), "B"),
            _ => panic!("expected Standard message"),
        }
    }

    #[test]
    fn empty_pipeline_returns_unchanged() {
        let pipeline = TransformPipeline::new();
        let messages = vec![
            AgentMessage::Standard(Message::user("one")),
            AgentMessage::Standard(Message::user("two")),
        ];
        let result = pipeline.apply(&messages);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_transform() {
        let mut pipeline = TransformPipeline::new();
        pipeline.push(Box::new(KeepOnlyCustom));

        let messages = vec![
            AgentMessage::Standard(Message::user("standard")),
            AgentMessage::Custom {
                type_name: "custom".into(),
                data: serde_json::json!({}),
            },
            AgentMessage::Standard(Message::user("also standard")),
        ];

        let result = pipeline.apply(&messages);
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], AgentMessage::Custom { .. }));
    }
}
