// INPUT:  async_trait, serde, std::collections, tokio::sync::RwLock, std::sync::atomic,
//         crate::extension::evaluation::evaluator::GradingCriterion
// OUTPUT: Outcome, OutcomeStatus, Rubric, OutcomeScore, OutcomeParams, OutcomePatch,
//         OutcomeFilter, OutcomeError, OutcomeRegistry, InMemoryOutcomeRegistry,
//         render_outcomes_for_session
// POS:    **Harness-level** outcome registry. Mirrors Anthropic Managed Agents
//         `session.outcome_evaluations[]` — each outcome is a typed goal the agent works
//         toward, with a rubric, iteration limit, and observable state machine
//         (pending → running → evaluating → satisfied | max_iterations_reached | failed |
//         interrupted). Lives in `alva-app-core` because outcomes are an App concern:
//         the SDK agent loop runs; harness-layer code (today: `EvaluationExtension` +
//         `agent-graph`) decides whether an outcome was met. Storing the result on the
//         session as a first-class entity lets UI / REST surface it without bespoke
//         per-app plumbing.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::extension::evaluation::evaluator::GradingCriterion;

// ===========================================================================
// Status state machine
// ===========================================================================

/// Lifecycle of one outcome. Mirrors Anthropic Managed Agents
/// `BetaManagedAgentsOutcomeEvaluationResource.result`:
///
/// - `Pending`: outcome was defined; the agent hasn't started work yet.
/// - `Running`: agent is producing / revising output for this outcome.
/// - `Evaluating`: grader is scoring the latest iteration.
/// - `NeedsRevision`: latest evaluation failed; the agent will revise
///   and try again (intermediate state, not terminal).
/// - `Satisfied`: terminal — criteria met.
/// - `MaxIterationsReached`: terminal — hit `max_iterations` cap.
/// - `Failed`: terminal — unrecoverable error (grader couldn't score,
///   etc.).
/// - `Interrupted`: terminal — user interrupt or session termination.
///
/// `OutcomeStatus::is_terminal` is true for the last four variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeStatus {
    Pending,
    Running,
    Evaluating,
    NeedsRevision,
    Satisfied,
    MaxIterationsReached,
    Failed,
    Interrupted,
}

impl OutcomeStatus {
    /// True for the four terminal variants. Used by `OutcomeFilter::terminal_only`.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Satisfied | Self::MaxIterationsReached | Self::Failed | Self::Interrupted
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Evaluating => "evaluating",
            Self::NeedsRevision => "needs_revision",
            Self::Satisfied => "satisfied",
            Self::MaxIterationsReached => "max_iterations_reached",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

// ===========================================================================
// Rubric
// ===========================================================================

/// How an outcome is to be graded. Mirrors Anthropic's two variants
/// (`text` / `file`) and adds a `Criteria` variant for alva's structured
/// weighted-criteria rubric (matches `EvaluatorNode`'s native shape).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Rubric {
    /// Inline text describing what "satisfied" looks like. The grader LLM
    /// reads this directly.
    Text { content: String },
    /// Reference to an uploaded file holding the rubric. The harness
    /// loads the bytes when scoring.
    File { file_id: String },
    /// Structured grading criteria with weights — alva's native rubric.
    /// The grader scores each criterion individually.
    Criteria { criteria: Vec<GradingCriterion> },
}

// ===========================================================================
// Score
// ===========================================================================

/// One iteration's score, attached to an outcome by `record_iteration`.
/// `per_criterion` is empty for `Rubric::Text` / `Rubric::File` outcomes
/// where the grader returns a single weighted score without breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutcomeScore {
    /// Weighted aggregate (0.0–1.0).
    pub weighted_score: f32,
    /// Whether the threshold was met. Whoever computes the score decides
    /// the threshold; the registry just stores the answer.
    pub passed: bool,
    /// Per-criterion score breakdown. Empty for non-structured rubrics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub per_criterion: Vec<(String, f32)>,
}

impl OutcomeScore {
    pub fn new(weighted_score: f32, passed: bool) -> Self {
        Self {
            weighted_score,
            passed,
            per_criterion: Vec::new(),
        }
    }

    pub fn with_criterion(mut self, name: impl Into<String>, score: f32) -> Self {
        self.per_criterion.push((name.into(), score));
        self
    }
}

// ===========================================================================
// Outcome record
// ===========================================================================

/// One outcome attached to a session. Created by `OutcomeRegistry::define`;
/// the registry assigns `id`, timestamps, and seeds `status = Pending`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    /// `outc_<hex>` id assigned by the registry. Mirrors Anthropic's
    /// `outc_…` prefix style.
    pub id: String,

    pub session_id: String,

    /// What the agent should produce. Free-form text from the App / user.
    pub description: String,

    pub rubric: Rubric,

    pub status: OutcomeStatus,

    /// 0-indexed revision cycle the outcome is on. Bumped by
    /// `record_iteration`.
    pub current_iteration: u32,

    /// Hard cap on revise cycles. Mirrors Anthropic's `max_iterations`
    /// (default 3, max 20). The registry doesn't enforce — the caller's
    /// evaluation loop checks `current_iteration >= max_iterations` and
    /// transitions to `MaxIterationsReached`.
    pub max_iterations: u32,

    /// Score from the latest evaluation iteration, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_score: Option<OutcomeScore>,

    /// Grader's verdict text from the most recent evaluation. Mirrors
    /// Anthropic's `explanation` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,

    pub created_at: i64,
    pub updated_at: i64,

    /// Set when the outcome reaches a terminal state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

// ===========================================================================
// Params / Patch / Filter
// ===========================================================================

#[derive(Debug, Clone)]
pub struct OutcomeParams {
    pub description: String,
    pub rubric: Rubric,
    pub max_iterations: u32,
}

impl OutcomeParams {
    /// Convenience: Anthropic default is 3, max is 20. Callers pick their
    /// own; this just bundles the required fields.
    pub fn new(description: impl Into<String>, rubric: Rubric, max_iterations: u32) -> Self {
        Self {
            description: description.into(),
            rubric,
            max_iterations,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OutcomePatch {
    pub status: Option<OutcomeStatus>,
    pub current_iteration: Option<u32>,
    pub latest_score: Option<Option<OutcomeScore>>,
    pub explanation: Option<Option<String>>,
}

impl OutcomePatch {
    pub fn status(mut self, status: OutcomeStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn current_iteration(mut self, n: u32) -> Self {
        self.current_iteration = Some(n);
        self
    }

    pub fn latest_score(mut self, score: OutcomeScore) -> Self {
        self.latest_score = Some(Some(score));
        self
    }

    pub fn clear_latest_score(mut self) -> Self {
        self.latest_score = Some(None);
        self
    }

    pub fn explanation(mut self, text: impl Into<String>) -> Self {
        self.explanation = Some(Some(text.into()));
        self
    }

    pub fn clear_explanation(mut self) -> Self {
        self.explanation = Some(None);
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct OutcomeFilter {
    /// Filter to a set of statuses. Empty / `None` = any.
    pub statuses: Option<Vec<OutcomeStatus>>,
    /// When true, only terminal outcomes (`Satisfied` /
    /// `MaxIterationsReached` / `Failed` / `Interrupted`).
    pub terminal_only: bool,
}

// ===========================================================================
// Errors
// ===========================================================================

#[derive(Debug, thiserror::Error)]
pub enum OutcomeError {
    #[error("outcome not found: {0}")]
    NotFound(String),
    #[error("outcome error: {0}")]
    Other(String),
}

// ===========================================================================
// Trait
// ===========================================================================

/// Session-scoped outcome collection. Maps onto Anthropic Managed Agents
/// `session.outcome_evaluations[]` field + the evaluation state machine
/// implied by `user.define_outcome` / `span.outcome_evaluation_*` events.
///
/// **Who writes**: an evaluation harness (today: `EvaluationExtension`
/// driving `agent-graph`) calls `define` when the agent receives a
/// `user.define_outcome` event, then `record_iteration` after each
/// grader pass, then transitions through `update` to a terminal status.
///
/// **Who reads**: UI / REST endpoints rendering the session summary;
/// system prompt rendering (`render_outcomes_for_session`); future
/// `Sessions.retrieve` endpoint that wants to surface
/// `outcome_evaluations[]` as part of the session response.
#[async_trait]
pub trait OutcomeRegistry: Send + Sync {
    /// Define a new outcome on `session_id`. Status starts at `Pending`,
    /// `current_iteration = 0`. Returns the inserted record.
    async fn define(
        &self,
        session_id: &str,
        params: OutcomeParams,
    ) -> Result<Outcome, OutcomeError>;

    async fn retrieve(&self, outcome_id: &str) -> Option<Outcome>;

    /// Patch. Fields set in the patch are written; others preserved.
    /// `updated_at` is bumped; if `patch.status` transitions to a
    /// terminal state and `completed_at` was `None`, it's set to "now".
    async fn update(
        &self,
        outcome_id: &str,
        patch: OutcomePatch,
    ) -> Result<(), OutcomeError>;

    /// Atomic helper: bumps `current_iteration` by 1, stores `score`
    /// and `explanation`, transitions status based on `passed`:
    /// - if `score.passed` → `Satisfied`
    /// - else if `current_iteration + 1 >= max_iterations` →
    ///   `MaxIterationsReached`
    /// - else → `NeedsRevision`
    ///
    /// Sets `completed_at` on terminal transitions.
    async fn record_iteration(
        &self,
        outcome_id: &str,
        score: OutcomeScore,
        explanation: Option<String>,
    ) -> Result<Outcome, OutcomeError>;

    async fn list_session(
        &self,
        session_id: &str,
        filter: &OutcomeFilter,
    ) -> Vec<Outcome>;

    async fn delete(&self, outcome_id: &str) -> Result<(), OutcomeError>;
}

// ===========================================================================
// Prompt rendering helper
// ===========================================================================

/// Render the active (non-terminal) outcomes on a session into a system-
/// prompt block. Lets the LLM see "you're working on these goals; here's
/// the rubric for each".
pub async fn render_outcomes_for_session(
    registry: &dyn OutcomeRegistry,
    session_id: &str,
) -> String {
    let active = registry
        .list_session(
            session_id,
            &OutcomeFilter {
                statuses: None,
                terminal_only: false,
            },
        )
        .await
        .into_iter()
        .filter(|o| !o.status.is_terminal())
        .collect::<Vec<_>>();
    if active.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Active Outcomes\n\n");
    for o in active {
        out.push_str(&format!(
            "### {} (iteration {}/{}, {})\n",
            o.id,
            o.current_iteration,
            o.max_iterations,
            o.status.as_str(),
        ));
        out.push_str(&o.description);
        out.push_str("\n\n**Rubric**: ");
        match &o.rubric {
            Rubric::Text { content } => {
                out.push_str(content);
                out.push('\n');
            }
            Rubric::File { file_id } => {
                out.push_str(&format!("(see file `{file_id}`)\n"));
            }
            Rubric::Criteria { criteria } => {
                out.push('\n');
                for c in criteria {
                    out.push_str(&format!(
                        "- **{}** (weight {:.2}): {}\n",
                        c.name, c.weight, c.description,
                    ));
                }
            }
        }
        if let Some(exp) = &o.explanation {
            out.push_str("\n**Last verdict**: ");
            out.push_str(exp);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

// ===========================================================================
// InMemoryOutcomeRegistry
// ===========================================================================

pub struct InMemoryOutcomeRegistry {
    by_id: RwLock<HashMap<String, Outcome>>,
    id_counter: AtomicU64,
}

impl InMemoryOutcomeRegistry {
    pub fn new() -> Self {
        Self {
            by_id: RwLock::new(HashMap::new()),
            id_counter: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> String {
        let n = self.id_counter.fetch_add(1, Ordering::SeqCst);
        format!("outc_{n:08x}")
    }
}

impl Default for InMemoryOutcomeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OutcomeRegistry for InMemoryOutcomeRegistry {
    async fn define(
        &self,
        session_id: &str,
        params: OutcomeParams,
    ) -> Result<Outcome, OutcomeError> {
        let now = chrono::Utc::now().timestamp_millis();
        let outcome = Outcome {
            id: self.next_id(),
            session_id: session_id.to_string(),
            description: params.description,
            rubric: params.rubric,
            status: OutcomeStatus::Pending,
            current_iteration: 0,
            max_iterations: params.max_iterations,
            latest_score: None,
            explanation: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        self.by_id
            .write()
            .await
            .insert(outcome.id.clone(), outcome.clone());
        Ok(outcome)
    }

    async fn retrieve(&self, outcome_id: &str) -> Option<Outcome> {
        self.by_id.read().await.get(outcome_id).cloned()
    }

    async fn update(
        &self,
        outcome_id: &str,
        patch: OutcomePatch,
    ) -> Result<(), OutcomeError> {
        let mut entries = self.by_id.write().await;
        let entry = entries
            .get_mut(outcome_id)
            .ok_or_else(|| OutcomeError::NotFound(outcome_id.to_string()))?;
        let now = chrono::Utc::now().timestamp_millis();
        if let Some(status) = patch.status {
            let was_terminal = entry.status.is_terminal();
            entry.status = status;
            // Transitioning into a terminal state for the first time
            // stamps `completed_at`.
            if status.is_terminal() && !was_terminal && entry.completed_at.is_none() {
                entry.completed_at = Some(now);
            }
        }
        if let Some(it) = patch.current_iteration {
            entry.current_iteration = it;
        }
        if let Some(score) = patch.latest_score {
            entry.latest_score = score;
        }
        if let Some(exp) = patch.explanation {
            entry.explanation = exp;
        }
        entry.updated_at = now;
        Ok(())
    }

    async fn record_iteration(
        &self,
        outcome_id: &str,
        score: OutcomeScore,
        explanation: Option<String>,
    ) -> Result<Outcome, OutcomeError> {
        let mut entries = self.by_id.write().await;
        let entry = entries
            .get_mut(outcome_id)
            .ok_or_else(|| OutcomeError::NotFound(outcome_id.to_string()))?;
        let now = chrono::Utc::now().timestamp_millis();
        let passed = score.passed;
        entry.current_iteration = entry.current_iteration.saturating_add(1);
        entry.latest_score = Some(score);
        entry.explanation = explanation;
        let new_status = if passed {
            OutcomeStatus::Satisfied
        } else if entry.current_iteration >= entry.max_iterations {
            OutcomeStatus::MaxIterationsReached
        } else {
            OutcomeStatus::NeedsRevision
        };
        let was_terminal = entry.status.is_terminal();
        entry.status = new_status;
        if new_status.is_terminal() && !was_terminal && entry.completed_at.is_none() {
            entry.completed_at = Some(now);
        }
        entry.updated_at = now;
        Ok(entry.clone())
    }

    async fn list_session(
        &self,
        session_id: &str,
        filter: &OutcomeFilter,
    ) -> Vec<Outcome> {
        let entries = self.by_id.read().await;
        let mut out: Vec<Outcome> = entries
            .values()
            .filter(|o| o.session_id == session_id)
            .filter(|o| {
                if filter.terminal_only {
                    o.status.is_terminal()
                } else {
                    true
                }
            })
            .filter(|o| match &filter.statuses {
                Some(statuses) if !statuses.is_empty() => statuses.contains(&o.status),
                _ => true,
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
        out
    }

    async fn delete(&self, outcome_id: &str) -> Result<(), OutcomeError> {
        let mut entries = self.by_id.write().await;
        if entries.remove(outcome_id).is_none() {
            return Err(OutcomeError::NotFound(outcome_id.to_string()));
        }
        Ok(())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn text_rubric() -> Rubric {
        Rubric::Text {
            content: "Output must be a working Rust function".into(),
        }
    }

    fn criteria_rubric() -> Rubric {
        Rubric::Criteria {
            criteria: vec![
                GradingCriterion::new("correctness", "Functionally correct", 0.6),
                GradingCriterion::new("style", "Idiomatic Rust", 0.4),
            ],
        }
    }

    #[test]
    fn terminal_statuses_classified_correctly() {
        assert!(!OutcomeStatus::Pending.is_terminal());
        assert!(!OutcomeStatus::Running.is_terminal());
        assert!(!OutcomeStatus::Evaluating.is_terminal());
        assert!(!OutcomeStatus::NeedsRevision.is_terminal());
        assert!(OutcomeStatus::Satisfied.is_terminal());
        assert!(OutcomeStatus::MaxIterationsReached.is_terminal());
        assert!(OutcomeStatus::Failed.is_terminal());
        assert!(OutcomeStatus::Interrupted.is_terminal());
    }

    #[test]
    fn status_strings_match_anthropic() {
        // Anthropic's enum values.
        assert_eq!(OutcomeStatus::Pending.as_str(), "pending");
        assert_eq!(OutcomeStatus::Running.as_str(), "running");
        assert_eq!(OutcomeStatus::Evaluating.as_str(), "evaluating");
        assert_eq!(OutcomeStatus::NeedsRevision.as_str(), "needs_revision");
        assert_eq!(OutcomeStatus::Satisfied.as_str(), "satisfied");
        assert_eq!(
            OutcomeStatus::MaxIterationsReached.as_str(),
            "max_iterations_reached"
        );
        assert_eq!(OutcomeStatus::Failed.as_str(), "failed");
        assert_eq!(OutcomeStatus::Interrupted.as_str(), "interrupted");
    }

    #[tokio::test]
    async fn define_initializes_pending_state() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define(
                "sesn-1",
                OutcomeParams::new("Build a fibonacci function", text_rubric(), 3),
            )
            .await
            .unwrap();
        assert!(o.id.starts_with("outc_"));
        assert_eq!(o.session_id, "sesn-1");
        assert_eq!(o.status, OutcomeStatus::Pending);
        assert_eq!(o.current_iteration, 0);
        assert_eq!(o.max_iterations, 3);
        assert!(o.latest_score.is_none());
        assert!(o.completed_at.is_none());
    }

    #[tokio::test]
    async fn record_iteration_marks_satisfied_on_pass() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define("sesn", OutcomeParams::new("d", text_rubric(), 3))
            .await
            .unwrap();

        let after = r
            .record_iteration(
                &o.id,
                OutcomeScore::new(0.9, true),
                Some("looks great".into()),
            )
            .await
            .unwrap();
        assert_eq!(after.status, OutcomeStatus::Satisfied);
        assert_eq!(after.current_iteration, 1);
        assert_eq!(after.explanation.as_deref(), Some("looks great"));
        assert!(after.latest_score.unwrap().passed);
        assert!(after.completed_at.is_some());
    }

    #[tokio::test]
    async fn record_iteration_marks_needs_revision_on_fail_below_cap() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define("sesn", OutcomeParams::new("d", text_rubric(), 3))
            .await
            .unwrap();

        let after = r
            .record_iteration(&o.id, OutcomeScore::new(0.4, false), Some("missing X".into()))
            .await
            .unwrap();
        assert_eq!(after.status, OutcomeStatus::NeedsRevision);
        assert_eq!(after.current_iteration, 1);
        assert!(after.completed_at.is_none());
    }

    #[tokio::test]
    async fn record_iteration_marks_max_iterations_when_cap_hit() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define("sesn", OutcomeParams::new("d", text_rubric(), 2))
            .await
            .unwrap();

        // First fail → NeedsRevision.
        r.record_iteration(&o.id, OutcomeScore::new(0.4, false), None)
            .await
            .unwrap();
        // Second fail at cap → MaxIterationsReached.
        let after = r
            .record_iteration(&o.id, OutcomeScore::new(0.5, false), None)
            .await
            .unwrap();
        assert_eq!(after.status, OutcomeStatus::MaxIterationsReached);
        assert_eq!(after.current_iteration, 2);
        assert!(after.completed_at.is_some());
    }

    #[tokio::test]
    async fn record_iteration_carries_per_criterion_breakdown() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define("sesn", OutcomeParams::new("d", criteria_rubric(), 3))
            .await
            .unwrap();

        let score = OutcomeScore::new(0.85, true)
            .with_criterion("correctness", 0.9)
            .with_criterion("style", 0.8);
        let after = r
            .record_iteration(&o.id, score, None)
            .await
            .unwrap();
        let latest = after.latest_score.unwrap();
        assert_eq!(latest.per_criterion.len(), 2);
        assert_eq!(latest.per_criterion[0], ("correctness".into(), 0.9));
    }

    #[tokio::test]
    async fn update_to_interrupted_sets_completed_at() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define("sesn", OutcomeParams::new("d", text_rubric(), 3))
            .await
            .unwrap();

        r.update(&o.id, OutcomePatch::default().status(OutcomeStatus::Interrupted))
            .await
            .unwrap();
        let after = r.retrieve(&o.id).await.unwrap();
        assert_eq!(after.status, OutcomeStatus::Interrupted);
        assert!(after.completed_at.is_some());
    }

    #[tokio::test]
    async fn update_terminal_again_does_not_move_completed_at() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define("sesn", OutcomeParams::new("d", text_rubric(), 3))
            .await
            .unwrap();

        // Transition to Satisfied via record_iteration.
        let first = r
            .record_iteration(&o.id, OutcomeScore::new(1.0, true), None)
            .await
            .unwrap();
        let stamped_at = first.completed_at.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(2)).await;

        // Now flip to Failed via update — completed_at should NOT move.
        r.update(&o.id, OutcomePatch::default().status(OutcomeStatus::Failed))
            .await
            .unwrap();
        let after = r.retrieve(&o.id).await.unwrap();
        assert_eq!(after.completed_at, Some(stamped_at));
    }

    #[tokio::test]
    async fn list_session_filters_by_status_and_terminal() {
        let r = InMemoryOutcomeRegistry::new();
        let a = r
            .define("sesn", OutcomeParams::new("a", text_rubric(), 3))
            .await
            .unwrap();
        let b = r
            .define("sesn", OutcomeParams::new("b", text_rubric(), 3))
            .await
            .unwrap();
        let c = r
            .define("sesn", OutcomeParams::new("c", text_rubric(), 3))
            .await
            .unwrap();
        r.define("other-sesn", OutcomeParams::new("d", text_rubric(), 3))
            .await
            .unwrap();

        // Mark a Satisfied (terminal), b NeedsRevision, c Pending.
        r.record_iteration(&a.id, OutcomeScore::new(1.0, true), None)
            .await
            .unwrap();
        r.record_iteration(&b.id, OutcomeScore::new(0.4, false), None)
            .await
            .unwrap();

        // Default filter: everything for this session.
        let all = r.list_session("sesn", &OutcomeFilter::default()).await;
        assert_eq!(all.len(), 3);

        // Terminal only: just a.
        let terminal = r
            .list_session(
                "sesn",
                &OutcomeFilter {
                    terminal_only: true,
                    ..Default::default()
                },
            )
            .await;
        let ids: Vec<_> = terminal.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, [a.id.as_str()]);

        // Status filter.
        let pending = r
            .list_session(
                "sesn",
                &OutcomeFilter {
                    statuses: Some(vec![OutcomeStatus::Pending]),
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, c.id);

        // Other session isolation.
        let other = r.list_session("other-sesn", &OutcomeFilter::default()).await;
        assert_eq!(other.len(), 1);
    }

    #[tokio::test]
    async fn delete_removes_outcome() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define("sesn", OutcomeParams::new("d", text_rubric(), 3))
            .await
            .unwrap();
        r.delete(&o.id).await.unwrap();
        assert!(r.retrieve(&o.id).await.is_none());
        let err = r.delete(&o.id).await.unwrap_err();
        assert!(matches!(err, OutcomeError::NotFound(_)));
    }

    #[tokio::test]
    async fn render_outcomes_includes_active_only_with_rubric_breakdown() {
        let r = InMemoryOutcomeRegistry::new();
        let active = r
            .define(
                "sesn",
                OutcomeParams::new("Build a fib fn", criteria_rubric(), 3),
            )
            .await
            .unwrap();
        let done = r
            .define("sesn", OutcomeParams::new("Old task", text_rubric(), 3))
            .await
            .unwrap();
        // Mark `done` Satisfied — terminal, excluded from active prompt.
        r.record_iteration(&done.id, OutcomeScore::new(1.0, true), None)
            .await
            .unwrap();

        let rendered = render_outcomes_for_session(&r, "sesn").await;
        assert!(rendered.contains("## Active Outcomes"));
        assert!(rendered.contains(&active.id));
        assert!(!rendered.contains(&done.id), "terminal outcomes hidden");
        assert!(rendered.contains("Build a fib fn"));
        // Criteria rubric variant renders each criterion line.
        assert!(rendered.contains("correctness"));
        assert!(rendered.contains("style"));
        assert!(rendered.contains("weight 0.60"));
    }

    #[tokio::test]
    async fn render_returns_empty_when_no_active_outcomes() {
        let r = InMemoryOutcomeRegistry::new();
        let o = r
            .define("sesn", OutcomeParams::new("d", text_rubric(), 3))
            .await
            .unwrap();
        r.record_iteration(&o.id, OutcomeScore::new(1.0, true), None)
            .await
            .unwrap();
        // Only terminal outcomes exist — render should be empty.
        assert!(render_outcomes_for_session(&r, "sesn").await.is_empty());
        assert!(render_outcomes_for_session(&r, "no-such-sesn")
            .await
            .is_empty());
    }

    #[test]
    fn rubric_serde_round_trip() {
        for rubric in [
            text_rubric(),
            criteria_rubric(),
            Rubric::File {
                file_id: "file_abc".into(),
            },
        ] {
            let json = serde_json::to_string(&rubric).unwrap();
            let back: Rubric = serde_json::from_str(&json).unwrap();
            assert_eq!(back, rubric);
        }
    }
}
