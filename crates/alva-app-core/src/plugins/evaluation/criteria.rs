// INPUT:  alva_types::context::{ContextHooks, ContextHandle, ContextError, ContextLayer, Priority, Injection, InjectionContent, PromptSection, ContextSnapshot, CompressAction, ContextEntry, IngestAction}, alva_types::AgentMessage
// OUTPUT: CriteriaDrivenPlugin, GradingCriterion
// POS:    ContextHooks plugin that injects grading criteria into the system prompt and steers self-evaluation.

//! Criteria-Driven Prompting — a ContextHooks plugin that injects explicit
//! scoring dimensions into the agent's context, steering output quality
//! through structured self-assessment.
//!
//! # Usage
//!
//! ```rust,ignore
//! use alva_app_core::evaluation::{CriteriaDrivenPlugin, GradingCriterion};
//! use alva_types::context::ContextSystem;
//! use std::sync::Arc;
//!
//! let plugin = CriteriaDrivenPlugin::new(vec![
//!     GradingCriterion::new("design_quality", "Coherent visual identity", 0.3),
//!     GradingCriterion::new("originality", "Avoids generic/template patterns", 0.3),
//!     GradingCriterion::new("craft", "Precise typography, spacing, color", 0.2),
//!     GradingCriterion::new("functionality", "Usable and accessible", 0.2),
//! ]);
//!
//! let context_system = ContextSystem::new(
//!     Arc::new(plugin),
//!     Arc::new(NoopContextHandle),
//! );
//! ```

use alva_types::context::{
    ContextEntry, ContextError, ContextHandle, ContextHooks, ContextSnapshot, CompressAction,
    Injection, IngestAction, ContextLayer,
};
use alva_types::{AgentMessage, Message};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// GradingCriterion
// ---------------------------------------------------------------------------

/// A single grading dimension with a name, description, and weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradingCriterion {
    /// Short identifier (e.g., "design_quality").
    pub name: String,
    /// What this criterion measures, in plain language. This text is
    /// shown directly to the LLM so it should be clear and actionable.
    pub description: String,
    /// Weight for scoring (0.0–1.0). All weights should sum to ~1.0.
    pub weight: f32,
}

impl GradingCriterion {
    pub fn new(name: impl Into<String>, description: impl Into<String>, weight: f32) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            weight,
        }
    }
}

// ---------------------------------------------------------------------------
// CriteriaDrivenPlugin
// ---------------------------------------------------------------------------

/// A ContextHooks plugin that injects grading criteria into the system prompt.
///
/// On `bootstrap`, it adds a structured scoring rubric to the AlwaysPresent
/// layer so the agent sees the quality bar from the first message.
///
/// On `after_turn`, it can optionally inject a self-review reminder (when
/// `self_review` is enabled) prompting the agent to reflect on its output
/// against the criteria before proceeding.
pub struct CriteriaDrivenPlugin {
    criteria: Vec<GradingCriterion>,
    /// Whether to inject a self-review prompt after each turn.
    self_review: bool,
    /// Optional preamble text inserted before the criteria table.
    preamble: Option<String>,
}

impl CriteriaDrivenPlugin {
    /// Create a new plugin with the given criteria.
    pub fn new(criteria: Vec<GradingCriterion>) -> Self {
        Self {
            criteria,
            self_review: false,
            preamble: None,
        }
    }

    /// Enable self-review injection after each turn.
    pub fn with_self_review(mut self) -> Self {
        self.self_review = true;
        self
    }

    /// Set a custom preamble that appears before the criteria table.
    pub fn with_preamble(mut self, preamble: impl Into<String>) -> Self {
        self.preamble = Some(preamble.into());
        self
    }

    /// Render criteria as a prompt section.
    fn render_criteria_prompt(&self) -> String {
        let mut s = String::new();

        if let Some(preamble) = &self.preamble {
            s.push_str(preamble);
            s.push_str("\n\n");
        }

        s.push_str("## Quality Criteria\n\n");
        s.push_str("Your output will be evaluated against the following dimensions:\n\n");
        s.push_str("| Criterion | Weight | Description |\n");
        s.push_str("|-----------|--------|-------------|\n");

        for c in &self.criteria {
            s.push_str(&format!(
                "| {} | {:.0}% | {} |\n",
                c.name,
                c.weight * 100.0,
                c.description,
            ));
        }

        s.push_str("\nAim for the highest quality across all dimensions. \
                     Prioritize criteria with higher weights when trade-offs are necessary.\n");
        s
    }

    /// Render the self-review prompt injected after turns.
    fn render_self_review_prompt(&self) -> String {
        let criteria_list = self
            .criteria
            .iter()
            .map(|c| format!("- **{}** ({:.0}%): {}", c.name, c.weight * 100.0, c.description))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "## Self-Review Checkpoint\n\n\
             Before proceeding, briefly assess your last output against these criteria:\n\n\
             {}\n\n\
             If any criterion scores below 70%, revise before moving on.\n",
            criteria_list
        )
    }
}

#[async_trait]
impl ContextHooks for CriteriaDrivenPlugin {
    fn name(&self) -> &str {
        "criteria_driven"
    }

    async fn bootstrap(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
    ) -> Result<(), ContextError> {
        // Inject criteria into the system prompt layer.
        let content = self.render_criteria_prompt();
        sdk.inject_message(
            agent_id,
            ContextLayer::AlwaysPresent,
            AgentMessage::Standard(Message::system(&content)),
        );
        Ok(())
    }

    async fn on_message(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        _message: &AgentMessage,
    ) -> Vec<Injection> {
        // No per-message injection; criteria are injected at bootstrap.
        vec![]
    }

    async fn on_budget_exceeded(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        _snapshot: &ContextSnapshot,
    ) -> Vec<CompressAction> {
        // Criteria prompt is small (~200 tokens), no special compression needed.
        // Fall back to default sliding window.
        vec![CompressAction::SlidingWindow { keep_recent: 20 }]
    }

    async fn assemble(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        entries: Vec<ContextEntry>,
        _token_budget: usize,
    ) -> Vec<ContextEntry> {
        // Pass through — no reordering needed.
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

    async fn after_turn(
        &self,
        sdk: &dyn ContextHandle,
        agent_id: &str,
    ) {
        if self.self_review {
            let review_prompt = self.render_self_review_prompt();
            sdk.inject_message(
                agent_id,
                ContextLayer::RuntimeInject,
                AgentMessage::Standard(Message::system(&review_prompt)),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_criteria() -> Vec<GradingCriterion> {
        vec![
            GradingCriterion::new("correctness", "Output is functionally correct", 0.4),
            GradingCriterion::new("quality", "Code is clean and idiomatic", 0.3),
            GradingCriterion::new("completeness", "All requirements addressed", 0.3),
        ]
    }

    #[test]
    fn criteria_prompt_contains_all_criteria() {
        let plugin = CriteriaDrivenPlugin::new(sample_criteria());
        let prompt = plugin.render_criteria_prompt();

        assert!(prompt.contains("correctness"));
        assert!(prompt.contains("quality"));
        assert!(prompt.contains("completeness"));
        assert!(prompt.contains("40%"));
        assert!(prompt.contains("30%"));
    }

    #[test]
    fn criteria_prompt_includes_preamble() {
        let plugin = CriteriaDrivenPlugin::new(sample_criteria())
            .with_preamble("The best designs are museum quality.");

        let prompt = plugin.render_criteria_prompt();
        assert!(prompt.contains("museum quality"));
    }

    #[test]
    fn self_review_prompt_lists_criteria() {
        let plugin = CriteriaDrivenPlugin::new(sample_criteria())
            .with_self_review();

        let prompt = plugin.render_self_review_prompt();
        assert!(prompt.contains("Self-Review Checkpoint"));
        assert!(prompt.contains("correctness"));
        assert!(prompt.contains("70%"));
    }

    #[test]
    fn grading_criterion_builder() {
        let c = GradingCriterion::new("test", "A test criterion", 0.5);
        assert_eq!(c.name, "test");
        assert_eq!(c.weight, 0.5);
    }

    #[test]
    fn weights_display_as_percentages() {
        let plugin = CriteriaDrivenPlugin::new(vec![
            GradingCriterion::new("a", "desc", 0.25),
        ]);
        let prompt = plugin.render_criteria_prompt();
        assert!(prompt.contains("25%"));
    }
}
