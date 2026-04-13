// INPUT:  serde, chrono, uuid
// OUTPUT: BoardMessage, MessageKind, TaskPhase
// POS:    Message types for the shared blackboard.

use serde::{Deserialize, Serialize};

/// A single message posted to the blackboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardMessage {
    /// Unique message ID.
    pub id: String,
    /// When it was posted (epoch millis).
    pub timestamp: i64,
    /// Who sent it (agent id).
    pub from: String,
    /// Who is @mentioned (empty = broadcast to all).
    pub mentions: Vec<String>,
    /// What kind of message this is.
    pub kind: MessageKind,
    /// Natural-language content (LLM-readable).
    pub content: String,
    /// Structured attachments (evaluation scores, code snippets, etc.)
    pub attachments: Vec<serde_json::Value>,
}

impl BoardMessage {
    /// Create a new message from a given agent.
    pub fn new(from: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            from: from.into(),
            mentions: Vec::new(),
            kind: MessageKind::Chat,
            content: content.into(),
            attachments: Vec::new(),
        }
    }

    pub fn with_kind(mut self, kind: MessageKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_mention(mut self, agent_id: impl Into<String>) -> Self {
        self.mentions.push(agent_id.into());
        self
    }

    pub fn with_mentions<I, S>(mut self, ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.mentions.extend(ids.into_iter().map(Into::into));
        self
    }

    pub fn with_attachment(mut self, data: serde_json::Value) -> Self {
        self.attachments.push(data);
        self
    }

    /// Check if this message mentions a specific agent.
    pub fn mentions_agent(&self, agent_id: &str) -> bool {
        self.mentions.iter().any(|m| m == agent_id)
    }

    /// Check if this is a broadcast (no specific mentions).
    pub fn is_broadcast(&self) -> bool {
        self.mentions.is_empty()
    }

    /// Render as a chat-room style line for LLM context injection.
    pub fn to_chat_line(&self) -> String {
        let mention_str = if self.mentions.is_empty() {
            String::new()
        } else {
            let tags: Vec<String> = self.mentions.iter().map(|m| format!("@{}", m)).collect();
            format!(" {}", tags.join(" "))
        };

        let kind_tag = match &self.kind {
            MessageKind::Introduction => " [intro]".to_string(),
            MessageKind::Chat => String::new(),
            MessageKind::Artifact { name } => format!(" [artifact: {}]", name),
            MessageKind::Question { .. } => " [question]".to_string(),
            MessageKind::Answer { .. } => " [answer]".to_string(),
            MessageKind::Status { phase } => format!(" [{}]", phase.label()),
        };

        format!("[{}]{}: {}{}", self.from, kind_tag, self.content, mention_str)
    }
}

/// What kind of board message this is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageKind {
    /// Self-introduction on join.
    Introduction,
    /// General collaboration message.
    Chat,
    /// Artifact submission (code, spec, document).
    Artifact { name: String },
    /// Question expecting an answer.
    Question { question_id: String },
    /// Answer to a prior question.
    Answer { question_id: String },
    /// Status update.
    Status { phase: TaskPhase },
}

/// Agent task lifecycle phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskPhase {
    Started,
    InProgress { percent: f32 },
    Blocked { reason: String },
    Completed,
    Failed { error: String },
}

impl TaskPhase {
    pub fn label(&self) -> &str {
        match self {
            Self::Started => "started",
            Self::InProgress { .. } => "in-progress",
            Self::Blocked { .. } => "blocked",
            Self::Completed => "completed",
            Self::Failed { .. } => "failed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_builder() {
        let msg = BoardMessage::new("gen", "PR is ready")
            .with_kind(MessageKind::Artifact {
                name: "login-feature".into(),
            })
            .with_mention("evaluator");

        assert_eq!(msg.from, "gen");
        assert!(msg.mentions_agent("evaluator"));
        assert!(!msg.is_broadcast());
    }

    #[test]
    fn broadcast_detection() {
        let msg = BoardMessage::new("system", "Budget at 80%")
            .with_kind(MessageKind::Status {
                phase: TaskPhase::InProgress { percent: 0.8 },
            });

        assert!(msg.is_broadcast());
    }

    #[test]
    fn chat_line_format() {
        let msg = BoardMessage::new("planner", "Spec is done, please start coding")
            .with_mention("generator");

        let line = msg.to_chat_line();
        assert!(line.contains("[planner]"));
        assert!(line.contains("@generator"));
        assert!(line.contains("Spec is done"));
    }

    #[test]
    fn introduction_chat_line() {
        let msg = BoardMessage::new("evaluator", "Hi, I review code quality")
            .with_kind(MessageKind::Introduction);

        let line = msg.to_chat_line();
        assert!(line.contains("[intro]"));
    }

    #[test]
    fn chat_line_artifact_no_dangling_ref() {
        // Artifact and Status variants previously used `&format!(...)` which
        // created a temporary String whose reference dangled.
        let msg = BoardMessage::new("gen", "提交代码")
            .with_kind(MessageKind::Artifact {
                name: "你好世界".into(),
            });
        let line = msg.to_chat_line();
        assert!(line.contains("[artifact: 你好世界]"));
        assert!(line.contains("提交代码"));
    }

    #[test]
    fn chat_line_status_no_dangling_ref() {
        let msg = BoardMessage::new("worker", "任务完成了")
            .with_kind(MessageKind::Status {
                phase: TaskPhase::Completed,
            });
        let line = msg.to_chat_line();
        assert!(line.contains("[completed]"));
        assert!(line.contains("任务完成了"));
    }

    #[test]
    fn question_answer_round_trip() {
        let q_id = "q-123".to_string();
        let question = BoardMessage::new("gen", "What's the API schema?")
            .with_kind(MessageKind::Question {
                question_id: q_id.clone(),
            })
            .with_mention("planner");

        let answer = BoardMessage::new("planner", "See attached spec")
            .with_kind(MessageKind::Answer {
                question_id: q_id.clone(),
            })
            .with_mention("gen");

        assert!(question.mentions_agent("planner"));
        assert!(answer.mentions_agent("gen"));

        match (&question.kind, &answer.kind) {
            (
                MessageKind::Question { question_id: qid },
                MessageKind::Answer { question_id: aid },
            ) => assert_eq!(qid, aid),
            _ => panic!("kind mismatch"),
        }
    }
}
