//! Evaluation subsystem — Evaluator role and Sprint Contracts.
//!
//! Inspired by the GAN-style generator-evaluator pattern described in
//! Anthropic's "Harness Design for Long-Running Application Development".
//!
//! Two components, each plugging into a different extension point:
//!
//! - [`EvaluatorNode`] — a Graph node function for QA evaluation loops
//! - [`SprintContractMiddleware`] — a Middleware that injects completion criteria

pub mod evaluator;
pub mod sprint_contract;

pub use evaluator::{EvaluationResult, EvaluationScore, EvaluatorConfig, EvaluatorNode, GradingCriterion};
pub use sprint_contract::{SprintContract, SprintContractMiddleware};
