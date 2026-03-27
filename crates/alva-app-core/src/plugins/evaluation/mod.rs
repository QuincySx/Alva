//! Evaluation subsystem — Evaluator role, Sprint Contracts, and Criteria-Driven Prompting.
//!
//! Inspired by the GAN-style generator-evaluator pattern described in
//! Anthropic's "Harness Design for Long-Running Application Development".
//!
//! Three components, each plugging into a different extension point:
//!
//! - [`EvaluatorNode`] — a Graph node function for QA evaluation loops
//! - [`SprintContractMiddleware`] — a Middleware that injects completion criteria
//! - [`CriteriaDrivenPlugin`] — a ContextHooks plugin for scoring-dimension prompting

pub mod criteria;
pub mod evaluator;
pub mod sprint_contract;

pub use criteria::{CriteriaDrivenPlugin, GradingCriterion};
pub use evaluator::{EvaluationResult, EvaluationScore, EvaluatorConfig, EvaluatorNode};
pub use sprint_contract::{SprintContract, SprintContractMiddleware};
