// INPUT:  alva_kernel_abi::context::*, alva_kernel_abi::{AgentMessage, Message}, super::{Blackboard, AgentProfile, BoardMessage, MessageKind, TaskPhase}
// OUTPUT: BlackboardPlugin
// POS:    ContextHooks plugin that bridges a single agent to the shared blackboard.

//! Blackboard ContextHooks plugin — one instance per agent, all sharing
//! the same `Arc<Blackboard>`.
//!
//! Lifecycle:
//! - `bootstrap`: register profile, read peers, inject team prompt, post introduction
//! - `assemble`: inject recent board messages into agent's context
//! - `after_turn`: sync agent's latest output back to the board

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use alva_kernel_abi::context::{
    CompressAction, ContextEntry, ContextError, ContextHandle, ContextHooks, ContextLayer,
    ContextSnapshot, IngestAction, Injection,
};
use alva_kernel_abi::{AgentMessage, Message};
use async_trait::async_trait;

use super::board::Blackboard;
use super::message::{BoardMessage, MessageKind, TaskPhase};
use super::profile::AgentProfile;

/// Configuration for how the plugin injects board messages.
#[derive(Debug, Clone)]
pub struct BlackboardPluginConfig {
    /// Maximum number of board messages to inject per assemble call.
    pub max_messages_in_context: usize,
    /// Whether to auto-post status updates after each turn.
    pub auto_post_status: bool,
    /// Whether to post the agent's assistant response to the board.
    pub auto_post_output: bool,
}

impl Default for BlackboardPluginConfig {
    fn default() -> Self {
        Self {
            max_messages_in_context: 50,
            auto_post_status: true,
            auto_post_output: false,
        }
    }
}

/// A ContextHooks plugin that connects one agent to the shared blackboard.
///
/// Each agent in the collaboration creates its own `BlackboardPlugin` with
/// its own `AgentProfile`, all pointing to the same `Arc<Blackboard>`.
pub struct BlackboardPlugin {
    profile: AgentProfile,
    board: Arc<Blackboard>,
    config: BlackboardPluginConfig,
    /// Tracks how many board messages have been injected so far,
    /// so `assemble` only injects new ones incrementally.
    last_seen_index: AtomicUsize,
}

impl BlackboardPlugin {
    pub fn new(profile: AgentProfile, board: Arc<Blackboard>) -> Self {
        Self {
            profile,
            board,
            config: BlackboardPluginConfig::default(),
            last_seen_index: AtomicUsize::new(0),
        }
    }

    pub fn with_config(mut self, config: BlackboardPluginConfig) -> Self {
        self.config = config;
        self
    }

    /// Post a message to the board on behalf of this agent.
    pub async fn post(&self, content: impl Into<String>) {
        self.board
            .post(BoardMessage::new(&self.profile.id, content))
            .await;
    }

    /// Post a message with mentions.
    pub async fn post_to(&self, content: impl Into<String>, mentions: &[&str]) {
        let msg = BoardMessage::new(&self.profile.id, content)
            .with_mentions(mentions.iter().copied());
        self.board.post(msg).await;
    }

    /// Post a status update.
    pub async fn post_status(&self, phase: TaskPhase) {
        let content = match &phase {
            TaskPhase::Started => "Started working on my task.".to_string(),
            TaskPhase::InProgress { percent } => {
                format!("In progress ({:.0}% complete).", percent * 100.0)
            }
            TaskPhase::Blocked { reason } => format!("Blocked: {}", reason),
            TaskPhase::Completed => "Task completed.".to_string(),
            TaskPhase::Failed { error } => format!("Failed: {}", error),
        };

        let mut msg = BoardMessage::new(&self.profile.id, content)
            .with_kind(MessageKind::Status { phase });

        // Notify dependents on completion.
        if matches!(msg.kind, MessageKind::Status { phase: TaskPhase::Completed }) {
            msg = msg.with_mentions(self.profile.provides_to.iter().map(|s| s.as_str()));
        }

        self.board.post(msg).await;
    }

    /// Build the board context section for injection into agent messages.
    async fn build_board_context(&self) -> Option<String> {
        let (log, total) = self
            .board
            .render_chat_log_for(&self.profile.id, self.config.max_messages_in_context)
            .await;

        if total == 0 {
            return None;
        }

        let mut section = String::from("## Team Chat\n\n");
        section.push_str(&log);
        section.push_str("\n\n---\n");
        section.push_str(&format!(
            "You are **{}**. Respond to messages that @mention you. \
             Use @name to address teammates.\n",
            self.profile.id
        ));

        self.last_seen_index.store(total, Ordering::Relaxed);

        Some(section)
    }
}

#[async_trait]
impl ContextHooks for BlackboardPlugin {
    fn name(&self) -> &str {
        "blackboard"
    }

    async fn bootstrap(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
    ) -> Result<(), ContextError> {
        // ① Register self.
        self.board.register(self.profile.clone()).await;

        // ② Build team prompt from all registered profiles.
        let all_profiles = self.board.profiles().await;
        let team_prompt = self.profile.build_team_prompt(&all_profiles);
        sdk.inject_message(
            agent_id,
            ContextLayer::AlwaysPresent,
            AgentMessage::Standard(Message::system(&team_prompt)),
        );

        // ③ Post introduction.
        let intro_content = format!(
            "Hi everyone, I'm {}, responsible for: {}.",
            self.profile.id, self.profile.role,
        );
        self.board
            .post(
                BoardMessage::new(&self.profile.id, intro_content)
                    .with_kind(MessageKind::Introduction),
            )
            .await;

        Ok(())
    }

    async fn on_message(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        _message: &AgentMessage,
    ) -> Vec<Injection> {
        // No per-message injection; board messages are injected in assemble().
        vec![]
    }

    async fn assemble(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        mut entries: Vec<ContextEntry>,
        _token_budget: usize,
    ) -> Vec<ContextEntry> {
        // Inject board messages as a system message in the RuntimeInject layer.
        if let Some(board_context) = self.build_board_context().await {
            let entry = ContextEntry {
                id: format!("blackboard-{}", self.profile.id),
                message: AgentMessage::Standard(Message::system(&board_context)),
                metadata: alva_kernel_abi::context::ContextMetadata::new(ContextLayer::RuntimeInject)
                    .with_priority(alva_kernel_abi::context::Priority::High),
            };
            entries.push(entry);
        }
        entries
    }

    async fn ingest(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        _entry: &ContextEntry,
    ) -> IngestAction {
        IngestAction::Keep
    }

    async fn on_budget_exceeded(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        _snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> {
        // Board messages are re-injected each assemble() call from the board's
        // own storage, so they can be safely evicted from the agent's context.
        // The board itself is never compressed — it's external.
        vec![]
    }

    async fn after_turn(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
    ) {
        if self.config.auto_post_status {
            // Check if agent produced output and post a brief status.
            let snapshot = sdk.snapshot(agent_id);
            let turn_tokens = snapshot.total_tokens;
            let ratio = snapshot.usage_ratio;

            // If budget is getting tight, post a heads-up.
            if ratio > 0.8 {
                self.board
                    .post(
                        BoardMessage::new(
                            &self.profile.id,
                            format!(
                                "Context budget at {:.0}% ({} tokens). May need to wrap up soon.",
                                ratio * 100.0,
                                turn_tokens,
                            ),
                        )
                        .with_kind(MessageKind::Status {
                            phase: TaskPhase::InProgress { percent: ratio },
                        }),
                    )
                    .await;
            }
        }
    }

    async fn dispose(&self) -> Result<(), ContextError> {
        // Post farewell status.
        self.board
            .post(
                BoardMessage::new(&self.profile.id, "Signing off.")
                    .with_kind(MessageKind::Status {
                        phase: TaskPhase::Completed,
                    }),
            )
            .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::context::NoopContextHandle;

    fn test_board() -> Arc<Blackboard> {
        Arc::new(Blackboard::new())
    }

    fn planner_profile() -> AgentProfile {
        AgentProfile::new("planner", "requirements analysis")
            .with_capability("write specs")
            .provides_to(["generator"])
    }

    fn generator_profile() -> AgentProfile {
        AgentProfile::new("generator", "code implementation")
            .depends_on(["planner"])
            .provides_to(["evaluator"])
    }

    #[tokio::test]
    async fn bootstrap_registers_and_introduces() {
        let board = test_board();
        let plugin = BlackboardPlugin::new(planner_profile(), board.clone());
        let handle = NoopContextHandle;

        plugin.bootstrap(&handle, "agent-1").await.unwrap();

        // Profile registered
        assert_eq!(board.agent_count().await, 1);
        assert!(board.profile("planner").await.is_some());

        // Introduction posted
        let msgs = board.all_messages().await;
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0].kind, MessageKind::Introduction));
        assert!(msgs[0].content.contains("planner"));
    }

    #[tokio::test]
    async fn two_agents_see_each_other() {
        let board = test_board();
        let handle = NoopContextHandle;

        let p1 = BlackboardPlugin::new(planner_profile(), board.clone());
        let p2 = BlackboardPlugin::new(generator_profile(), board.clone());

        p1.bootstrap(&handle, "agent-planner").await.unwrap();
        p2.bootstrap(&handle, "agent-generator").await.unwrap();

        assert_eq!(board.agent_count().await, 2);
        assert_eq!(board.message_count().await, 2); // two introductions
    }

    #[tokio::test]
    async fn post_and_read_via_plugin() {
        let board = test_board();
        let plugin = BlackboardPlugin::new(planner_profile(), board.clone());

        plugin.post("spec is ready").await;
        plugin.post_to("please start", &["generator"]).await;

        let msgs = board.all_messages().await;
        assert_eq!(msgs.len(), 2);
        assert!(msgs[1].mentions_agent("generator"));
    }

    #[tokio::test]
    async fn post_status_notifies_dependents() {
        let board = test_board();
        let plugin = BlackboardPlugin::new(
            AgentProfile::new("gen", "code").provides_to(["evaluator"]),
            board.clone(),
        );

        plugin.post_status(TaskPhase::Completed).await;

        let msgs = board.all_messages().await;
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].mentions_agent("evaluator"));
    }

    #[tokio::test]
    async fn build_board_context_renders_chat() {
        let board = test_board();

        board
            .post(BoardMessage::new("planner", "spec done").with_kind(MessageKind::Introduction))
            .await;
        board
            .post(BoardMessage::new("planner", "start coding").with_mention("gen"))
            .await;

        let plugin = BlackboardPlugin::new(
            AgentProfile::new("gen", "code").depends_on(["planner"]),
            board.clone(),
        );

        let ctx = plugin.build_board_context().await;
        assert!(ctx.is_some());

        let text = ctx.unwrap();
        assert!(text.contains("Team Chat"));
        assert!(text.contains("planner"));
        assert!(text.contains("@gen"));
    }

    #[tokio::test]
    async fn empty_board_returns_none() {
        let board = test_board();
        let plugin = BlackboardPlugin::new(
            AgentProfile::new("gen", "code"),
            board.clone(),
        );

        let ctx = plugin.build_board_context().await;
        assert!(ctx.is_none());
    }

    #[tokio::test]
    async fn dispose_posts_farewell() {
        let board = test_board();
        let plugin = BlackboardPlugin::new(planner_profile(), board.clone());

        plugin.dispose().await.unwrap();

        let msgs = board.all_messages().await;
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].content.contains("Signing off"));
    }
}
