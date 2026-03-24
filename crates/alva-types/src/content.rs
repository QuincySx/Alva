use serde::{Deserialize, Serialize};

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, media_type: String },
    #[serde(rename = "reasoning")]
    Reasoning { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(alias = "tool_use_id")]
        id: String,
        content: String,
        is_error: bool,
    },
}

impl ContentBlock {
    /// Returns the text content if this is a Text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Returns true if this is a Text block.
    pub fn is_text(&self) -> bool {
        matches!(self, ContentBlock::Text { .. })
    }

    /// Returns `(id, name, input)` if this is a ToolUse block.
    pub fn as_tool_use(&self) -> Option<(&str, &str, &serde_json::Value)> {
        match self {
            ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
            _ => None,
        }
    }

    /// Returns true if this is a ToolUse block.
    pub fn is_tool_use(&self) -> bool {
        matches!(self, ContentBlock::ToolUse { .. })
    }

    /// Returns `(id, content, is_error)` if this is a ToolResult block.
    pub fn as_tool_result(&self) -> Option<(&str, &str, bool)> {
        match self {
            ContentBlock::ToolResult {
                id,
                content,
                is_error,
            } => Some((id, content, *is_error)),
            _ => None,
        }
    }

    /// Returns true if this is a ToolResult block.
    pub fn is_tool_result(&self) -> bool {
        matches!(self, ContentBlock::ToolResult { .. })
    }

    /// Returns the reasoning text if this is a Reasoning block.
    pub fn as_reasoning(&self) -> Option<&str> {
        match self {
            ContentBlock::Reasoning { text } => Some(text),
            _ => None,
        }
    }

    /// Returns true if this is a Reasoning block.
    pub fn is_reasoning(&self) -> bool {
        matches!(self, ContentBlock::Reasoning { .. })
    }

    /// Returns `(data, media_type)` if this is an Image block.
    pub fn as_image(&self) -> Option<(&str, &str)> {
        match self {
            ContentBlock::Image { data, media_type } => Some((data, media_type)),
            _ => None,
        }
    }

    /// Returns true if this is an Image block.
    pub fn is_image(&self) -> bool {
        matches!(self, ContentBlock::Image { .. })
    }

    /// Estimate token count for this content block.
    ///
    /// Uses a simple character-based heuristic (~4 chars per token).
    pub fn estimated_tokens(&self) -> usize {
        let char_len = match self {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::Reasoning { text } => text.len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
            ContentBlock::ToolUse { input, .. } => input.to_string().len(),
            ContentBlock::Image { data, .. } => data.len(),
        };
        // ~4 chars per token is a common rough estimate
        (char_len + 3) / 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_block() -> ContentBlock {
        ContentBlock::Text {
            text: "hello".into(),
        }
    }

    fn tool_use_block() -> ContentBlock {
        ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "grep".into(),
            input: serde_json::json!({"q": "foo"}),
        }
    }

    fn tool_result_block() -> ContentBlock {
        ContentBlock::ToolResult {
            id: "tu_1".into(),
            content: "found foo".into(),
            is_error: false,
        }
    }

    fn reasoning_block() -> ContentBlock {
        ContentBlock::Reasoning {
            text: "thinking...".into(),
        }
    }

    fn image_block() -> ContentBlock {
        ContentBlock::Image {
            data: "iVBOR...".into(),
            media_type: "image/png".into(),
        }
    }

    // ── as_text / is_text ───────────────────────────────────────────

    #[test]
    fn as_text_returns_some_for_text() {
        assert_eq!(text_block().as_text(), Some("hello"));
    }

    #[test]
    fn as_text_returns_none_for_others() {
        assert!(tool_use_block().as_text().is_none());
        assert!(tool_result_block().as_text().is_none());
        assert!(reasoning_block().as_text().is_none());
        assert!(image_block().as_text().is_none());
    }

    #[test]
    fn is_text_discriminates() {
        assert!(text_block().is_text());
        assert!(!tool_use_block().is_text());
        assert!(!reasoning_block().is_text());
    }

    // ── as_tool_use / is_tool_use ───────────────────────────────────

    #[test]
    fn as_tool_use_returns_fields() {
        let block = tool_use_block();
        let (id, name, input) = block.as_tool_use().unwrap();
        assert_eq!(id, "tu_1");
        assert_eq!(name, "grep");
        assert_eq!(input["q"], "foo");
    }

    #[test]
    fn as_tool_use_returns_none_for_others() {
        assert!(text_block().as_tool_use().is_none());
        assert!(tool_result_block().as_tool_use().is_none());
    }

    #[test]
    fn is_tool_use_discriminates() {
        assert!(tool_use_block().is_tool_use());
        assert!(!text_block().is_tool_use());
    }

    // ── as_tool_result / is_tool_result ─────────────────────────────

    #[test]
    fn as_tool_result_returns_fields() {
        let block = tool_result_block();
        let (id, content, is_error) = block.as_tool_result().unwrap();
        assert_eq!(id, "tu_1");
        assert_eq!(content, "found foo");
        assert!(!is_error);
    }

    #[test]
    fn as_tool_result_returns_none_for_others() {
        assert!(text_block().as_tool_result().is_none());
        assert!(tool_use_block().as_tool_result().is_none());
    }

    #[test]
    fn is_tool_result_discriminates() {
        assert!(tool_result_block().is_tool_result());
        assert!(!text_block().is_tool_result());
    }

    // ── as_reasoning / is_reasoning ─────────────────────────────────

    #[test]
    fn as_reasoning_returns_some() {
        assert_eq!(reasoning_block().as_reasoning(), Some("thinking..."));
    }

    #[test]
    fn as_reasoning_returns_none_for_others() {
        assert!(text_block().as_reasoning().is_none());
        assert!(image_block().as_reasoning().is_none());
    }

    #[test]
    fn is_reasoning_discriminates() {
        assert!(reasoning_block().is_reasoning());
        assert!(!text_block().is_reasoning());
    }

    // ── as_image / is_image ─────────────────────────────────────────

    #[test]
    fn as_image_returns_fields() {
        let block = image_block();
        let (data, media) = block.as_image().unwrap();
        assert_eq!(data, "iVBOR...");
        assert_eq!(media, "image/png");
    }

    #[test]
    fn as_image_returns_none_for_others() {
        assert!(text_block().as_image().is_none());
        assert!(reasoning_block().as_image().is_none());
    }

    #[test]
    fn is_image_discriminates() {
        assert!(image_block().is_image());
        assert!(!text_block().is_image());
    }

    // ── estimated_tokens ────────────────────────────────────────────

    #[test]
    fn estimated_tokens_text() {
        // "hello" = 5 chars => (5+3)/4 = 2
        assert_eq!(text_block().estimated_tokens(), 2);
    }

    #[test]
    fn estimated_tokens_reasoning() {
        // "thinking..." = 11 chars => (11+3)/4 = 3
        assert_eq!(reasoning_block().estimated_tokens(), 3);
    }

    #[test]
    fn estimated_tokens_tool_result() {
        // "found foo" = 9 chars => (9+3)/4 = 3
        assert_eq!(tool_result_block().estimated_tokens(), 3);
    }

    #[test]
    fn estimated_tokens_tool_use() {
        let block = tool_use_block();
        // ToolUse token count is based on JSON serialization of input
        assert!(block.estimated_tokens() > 0);
    }

    #[test]
    fn estimated_tokens_image() {
        let block = image_block();
        // "iVBOR..." = 7 chars => (7+3)/4 = 2
        assert_eq!(block.estimated_tokens(), 2);
    }

    #[test]
    fn estimated_tokens_empty_text() {
        let block = ContentBlock::Text {
            text: String::new(),
        };
        // 0 chars => (0+3)/4 = 0
        assert_eq!(block.estimated_tokens(), 0);
    }
}
