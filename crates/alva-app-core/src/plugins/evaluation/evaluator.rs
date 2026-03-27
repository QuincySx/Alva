// INPUT:  alva_agent_graph::{StateGraph, graph::END}, serde, serde_json
// OUTPUT: EvaluatorNode, EvaluationScore, EvaluationResult, EvaluatorConfig
// POS:    Graph node that evaluates generator output against grading criteria and decides pass/retry.

//! Evaluator role — a StateGraph node that scores generator output and routes
//! back for retry or forward to completion.
//!
//! # Usage
//!
//! ```rust,ignore
//! use alva_app_core::evaluation::{EvaluatorNode, EvaluatorConfig, GradingCriterion};
//! use alva_agent_graph::{StateGraph, graph::END};
//!
//! let evaluator = EvaluatorNode::new(EvaluatorConfig {
//!     pass_threshold: 0.7,
//!     max_retries: 3,
//!     criteria: vec![
//!         GradingCriterion::new("correctness", "Output is functionally correct", 0.4),
//!         GradingCriterion::new("quality", "Code is clean and idiomatic", 0.3),
//!         GradingCriterion::new("completeness", "All requirements are addressed", 0.3),
//!     ],
//! });
//!
//! let mut graph = StateGraph::new();
//! graph.add_node("generator", generator_fn);
//! graph.add_node("evaluator", evaluator.as_node_fn());
//! graph.add_edge("generator", "evaluator");
//! graph.add_conditional_edge("evaluator", EvaluatorNode::router());
//! graph.set_entry_point("generator");
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

use super::criteria::GradingCriterion;

/// Graph termination sentinel — matches `alva_agent_graph::END`.
const END: &str = "__end__";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the evaluator node.
#[derive(Debug, Clone)]
pub struct EvaluatorConfig {
    /// Weighted score threshold to pass (0.0–1.0).
    pub pass_threshold: f32,
    /// Maximum number of generator retries before forced completion.
    pub max_retries: u32,
    /// Grading criteria with weights. Weights should sum to 1.0.
    pub criteria: Vec<GradingCriterion>,
}

impl Default for EvaluatorConfig {
    fn default() -> Self {
        Self {
            pass_threshold: 0.7,
            max_retries: 3,
            criteria: vec![
                GradingCriterion::new("correctness", "Output is functionally correct", 0.4),
                GradingCriterion::new("quality", "Code is clean and idiomatic", 0.3),
                GradingCriterion::new("completeness", "All requirements are addressed", 0.3),
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Score types
// ---------------------------------------------------------------------------

/// Score for a single criterion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionScore {
    /// Criterion name.
    pub name: String,
    /// Raw score (0.0–1.0).
    pub score: f32,
    /// Free-form feedback explaining the score.
    pub feedback: String,
}

/// Aggregated evaluation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationScore {
    /// Per-criterion scores.
    pub criteria_scores: Vec<CriterionScore>,
    /// Weighted aggregate (0.0–1.0).
    pub weighted_score: f32,
    /// Whether the threshold was met.
    pub passed: bool,
    /// High-level summary of findings.
    pub summary: String,
    /// Specific issues found (for generator feedback).
    pub issues: Vec<String>,
}

impl fmt::Display for EvaluationScore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] score={:.2} | {}",
            if self.passed { "PASS" } else { "FAIL" },
            self.weighted_score,
            self.summary,
        )
    }
}

/// Outcome of an evaluator node execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    /// The evaluation score.
    pub score: EvaluationScore,
    /// Current retry count (incremented on each failure).
    pub retry_count: u32,
    /// Whether max retries have been exhausted.
    pub retries_exhausted: bool,
}

// ---------------------------------------------------------------------------
// EvaluatorNode
// ---------------------------------------------------------------------------

/// Evaluator node for use in a `StateGraph`.
///
/// This struct holds configuration and provides:
/// - `as_node_fn()` — returns a closure suitable for `StateGraph::add_node`
/// - `router()` — returns a routing function for `add_conditional_edge`
///
/// The node function is generic over any state `S` that implements
/// `EvaluatorState`. This trait lets the evaluator read generator output
/// and write evaluation results without knowing the concrete state type.
pub struct EvaluatorNode {
    config: EvaluatorConfig,
}

impl EvaluatorNode {
    pub fn new(config: EvaluatorConfig) -> Self {
        Self { config }
    }

    /// Access the config (useful for building prompts externally).
    pub fn config(&self) -> &EvaluatorConfig {
        &self.config
    }

    /// Evaluate artifacts against criteria.
    ///
    /// This is the core scoring logic, separated from the graph node machinery
    /// so it can be used standalone (e.g., in tests or non-graph contexts).
    ///
    /// `score_fn` is called for each criterion with `(criterion, artifacts)` and
    /// returns `(score, feedback)`. This allows the caller to plug in any scoring
    /// backend (LLM call, rule-based, hybrid).
    pub fn evaluate<F>(
        &self,
        artifacts: &str,
        retry_count: u32,
        mut score_fn: F,
    ) -> EvaluationResult
    where
        F: FnMut(&GradingCriterion, &str) -> (f32, String),
    {
        let mut criteria_scores = Vec::with_capacity(self.config.criteria.len());
        let mut weighted_sum = 0.0f32;
        let mut issues = Vec::new();

        for criterion in &self.config.criteria {
            let (raw_score, feedback) = score_fn(criterion, artifacts);
            let score = raw_score.clamp(0.0, 1.0);

            weighted_sum += score * criterion.weight;

            if score < self.config.pass_threshold {
                issues.push(format!(
                    "{}: {:.0}% — {}",
                    criterion.name,
                    score * 100.0,
                    feedback
                ));
            }

            criteria_scores.push(CriterionScore {
                name: criterion.name.clone(),
                score,
                feedback,
            });
        }

        let passed = weighted_sum >= self.config.pass_threshold;
        let retries_exhausted = retry_count >= self.config.max_retries;

        let summary = if passed {
            format!(
                "Passed with weighted score {:.2} (threshold {:.2})",
                weighted_sum, self.config.pass_threshold
            )
        } else if retries_exhausted {
            format!(
                "Failed with score {:.2} after {} retries (max {}). Proceeding anyway.",
                weighted_sum, retry_count, self.config.max_retries
            )
        } else {
            format!(
                "Failed with score {:.2} (threshold {:.2}). {} issue(s) found. Retry {}/{}.",
                weighted_sum,
                self.config.pass_threshold,
                issues.len(),
                retry_count + 1,
                self.config.max_retries
            )
        };

        EvaluationResult {
            score: EvaluationScore {
                criteria_scores,
                weighted_score: weighted_sum,
                passed: passed || retries_exhausted,
                summary,
                issues,
            },
            retry_count: if passed || retries_exhausted {
                retry_count
            } else {
                retry_count + 1
            },
            retries_exhausted,
        }
    }

    /// Build the evaluator feedback prompt that gets injected into the
    /// generator's next context when the evaluation fails.
    ///
    /// The generator sees exactly what failed and why, enabling targeted fixes.
    pub fn build_feedback_prompt(result: &EvaluationResult) -> String {
        let mut prompt = String::from("## Evaluation Feedback\n\n");
        prompt.push_str(&format!(
            "**Score**: {:.0}% | **Status**: {}\n\n",
            result.score.weighted_score * 100.0,
            if result.score.passed { "PASS" } else { "NEEDS REVISION" },
        ));

        if !result.score.issues.is_empty() {
            prompt.push_str("### Issues to Fix\n\n");
            for issue in &result.score.issues {
                prompt.push_str(&format!("- {}\n", issue));
            }
            prompt.push('\n');
        }

        prompt.push_str("### Per-Criterion Breakdown\n\n");
        for cs in &result.score.criteria_scores {
            prompt.push_str(&format!(
                "- **{}**: {:.0}% — {}\n",
                cs.name,
                cs.score * 100.0,
                cs.feedback
            ));
        }

        prompt
    }
}

// ---------------------------------------------------------------------------
// Trait for state interop with StateGraph
// ---------------------------------------------------------------------------

/// Trait that workflow states implement to participate in evaluator routing.
///
/// The evaluator node reads generator output via `generator_output()` and writes
/// the evaluation result via `set_evaluation()`. The router reads `evaluation()`
/// to decide the next node.
pub trait EvaluatorState: Send + 'static {
    /// The generator's output artifacts as a string (code, HTML, etc.)
    fn generator_output(&self) -> &str;

    /// Current retry count.
    fn retry_count(&self) -> u32;

    /// Read the evaluation result (set by the evaluator node).
    fn evaluation(&self) -> Option<&EvaluationResult>;

    /// Store the evaluation result.
    fn set_evaluation(&mut self, result: EvaluationResult);

    /// Store feedback for the generator's next iteration.
    fn set_evaluator_feedback(&mut self, feedback: String);
}

/// Name of the generator node (used by the router to send retries).
pub const GENERATOR_NODE: &str = "generator";

/// Router function for conditional edges after the evaluator node.
///
/// Routes to `END` if passed, or back to `GENERATOR_NODE` if failed.
///
/// ```rust,ignore
/// graph.add_conditional_edge("evaluator", evaluator_router::<MyState>);
/// ```
pub fn evaluator_router<S: EvaluatorState>(state: &S) -> String {
    match state.evaluation() {
        Some(result) if result.score.passed => END.to_string(),
        _ => GENERATOR_NODE.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_passes_when_above_threshold() {
        let node = EvaluatorNode::new(EvaluatorConfig {
            pass_threshold: 0.7,
            max_retries: 3,
            criteria: vec![
                GradingCriterion::new("a", "test", 0.5),
                GradingCriterion::new("b", "test", 0.5),
            ],
        });

        let result = node.evaluate("artifacts", 0, |criterion, _| {
            match criterion.name.as_str() {
                "a" => (0.9, "great".into()),
                "b" => (0.8, "good".into()),
                _ => (0.5, "ok".into()),
            }
        });

        assert!(result.score.passed);
        assert!(result.score.weighted_score >= 0.7);
        assert!(result.score.issues.is_empty());
    }

    #[test]
    fn evaluation_fails_when_below_threshold() {
        let node = EvaluatorNode::new(EvaluatorConfig {
            pass_threshold: 0.7,
            max_retries: 3,
            criteria: vec![
                GradingCriterion::new("a", "test", 0.5),
                GradingCriterion::new("b", "test", 0.5),
            ],
        });

        let result = node.evaluate("artifacts", 0, |_, _| (0.3, "poor".into()));

        assert!(!result.score.passed);
        assert_eq!(result.retry_count, 1);
        assert!(!result.score.issues.is_empty());
    }

    #[test]
    fn retries_exhaust_forces_pass() {
        let node = EvaluatorNode::new(EvaluatorConfig {
            pass_threshold: 0.7,
            max_retries: 2,
            criteria: vec![GradingCriterion::new("a", "test", 1.0)],
        });

        let result = node.evaluate("artifacts", 2, |_, _| (0.3, "still poor".into()));

        assert!(result.score.passed); // forced pass
        assert!(result.retries_exhausted);
    }

    #[test]
    fn scores_are_clamped() {
        let node = EvaluatorNode::new(EvaluatorConfig::default());

        let result = node.evaluate("artifacts", 0, |_, _| (1.5, "overflow".into()));

        for cs in &result.score.criteria_scores {
            assert!(cs.score <= 1.0);
        }
    }

    #[test]
    fn feedback_prompt_contains_issues() {
        let node = EvaluatorNode::new(EvaluatorConfig {
            pass_threshold: 0.8,
            max_retries: 3,
            criteria: vec![GradingCriterion::new("quality", "Code quality", 1.0)],
        });

        let result = node.evaluate("bad code", 0, |_, _| {
            (0.4, "functions too long, no error handling".into())
        });

        let prompt = EvaluatorNode::build_feedback_prompt(&result);
        assert!(prompt.contains("Issues to Fix"));
        assert!(prompt.contains("functions too long"));
        assert!(prompt.contains("NEEDS REVISION"));
    }

    struct MockState {
        output: String,
        retries: u32,
        eval: Option<EvaluationResult>,
        feedback: Option<String>,
    }

    impl EvaluatorState for MockState {
        fn generator_output(&self) -> &str { &self.output }
        fn retry_count(&self) -> u32 { self.retries }
        fn evaluation(&self) -> Option<&EvaluationResult> { self.eval.as_ref() }
        fn set_evaluation(&mut self, result: EvaluationResult) { self.eval = Some(result); }
        fn set_evaluator_feedback(&mut self, feedback: String) { self.feedback = Some(feedback); }
    }

    #[test]
    fn router_returns_end_on_pass() {
        let mut state = MockState {
            output: "good output".into(),
            retries: 0,
            eval: None,
            feedback: None,
        };

        let node = EvaluatorNode::new(EvaluatorConfig::default());
        let result = node.evaluate(&state.output, state.retries, |_, _| (0.9, "great".into()));
        state.set_evaluation(result);

        assert_eq!(
            evaluator_router(&state),
            END.to_string()
        );
    }

    #[test]
    fn router_returns_generator_on_fail() {
        let mut state = MockState {
            output: "bad output".into(),
            retries: 0,
            eval: None,
            feedback: None,
        };

        let node = EvaluatorNode::new(EvaluatorConfig::default());
        let result = node.evaluate(&state.output, state.retries, |_, _| (0.2, "bad".into()));
        state.set_evaluation(result);

        assert_eq!(evaluator_router(&state), GENERATOR_NODE);
    }
}
