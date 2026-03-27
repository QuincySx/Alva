// INPUT:  alva_agent_core::middleware, alva_types::Message
// OUTPUT: SprintContract, SprintContractMiddleware
// POS:    Middleware that injects sprint completion contracts into the LLM context.

//! Sprint Contract middleware — injects structured completion criteria into the
//! agent's context so both generator and evaluator share an explicit "definition
//! of done" for each work unit.
//!
//! # Usage
//!
//! ```rust,ignore
//! use alva_app_core::evaluation::{SprintContract, SprintContractMiddleware};
//!
//! let contract = SprintContract::new("Implement user login")
//!     .with_deliverable("POST /api/login endpoint accepting email+password")
//!     .with_deliverable("JWT token returned on success")
//!     .with_deliverable("401 response on invalid credentials")
//!     .with_verification("curl -X POST with valid credentials returns 200 + token")
//!     .with_verification("curl -X POST with wrong password returns 401");
//!
//! let middleware = SprintContractMiddleware::new(contract);
//! hooks.middleware.push_sorted(Arc::new(middleware));
//! ```

use std::fmt;

use alva_agent_core::middleware::{Middleware, MiddlewareContext, MiddlewareError, MiddlewarePriority};
use alva_types::Message;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// SprintContract
// ---------------------------------------------------------------------------

/// A structured agreement between generator and evaluator defining what
/// constitutes "done" for a unit of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SprintContract {
    /// Short description of the sprint goal.
    pub goal: String,
    /// Concrete deliverables the generator must produce.
    pub deliverables: Vec<String>,
    /// How to verify each deliverable is met.
    pub verifications: Vec<String>,
    /// Hard constraints that must not be violated.
    pub constraints: Vec<String>,
}

impl SprintContract {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            deliverables: Vec::new(),
            verifications: Vec::new(),
            constraints: Vec::new(),
        }
    }

    pub fn with_deliverable(mut self, deliverable: impl Into<String>) -> Self {
        self.deliverables.push(deliverable.into());
        self
    }

    pub fn with_verification(mut self, verification: impl Into<String>) -> Self {
        self.verifications.push(verification.into());
        self
    }

    pub fn with_constraint(mut self, constraint: impl Into<String>) -> Self {
        self.constraints.push(constraint.into());
        self
    }

    /// Render the contract as a prompt section.
    pub fn to_prompt(&self) -> String {
        let mut s = format!("## Sprint Contract\n\n**Goal**: {}\n", self.goal);

        if !self.deliverables.is_empty() {
            s.push_str("\n### Deliverables\n");
            for (i, d) in self.deliverables.iter().enumerate() {
                s.push_str(&format!("{}. {}\n", i + 1, d));
            }
        }

        if !self.verifications.is_empty() {
            s.push_str("\n### Verification\n");
            for v in &self.verifications {
                s.push_str(&format!("- [ ] {}\n", v));
            }
        }

        if !self.constraints.is_empty() {
            s.push_str("\n### Constraints\n");
            for c in &self.constraints {
                s.push_str(&format!("- {}\n", c));
            }
        }

        s.push_str("\n**Important**: Do not consider this sprint complete until ALL deliverables \
                     are met and ALL verifications pass. If you cannot meet a deliverable, \
                     explain what is blocking you.\n");
        s
    }
}

impl fmt::Display for SprintContract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SprintContract({}, {} deliverables)", self.goal, self.deliverables.len())
    }
}

// ---------------------------------------------------------------------------
// SprintContractMiddleware
// ---------------------------------------------------------------------------

/// Middleware that injects a `SprintContract` into the system prompt before
/// each LLM call, and stores it in `Extensions` for downstream consumers
/// (e.g., the evaluator node).
pub struct SprintContractMiddleware {
    contract: SprintContract,
}

impl SprintContractMiddleware {
    pub fn new(contract: SprintContract) -> Self {
        Self { contract }
    }
}

#[async_trait]
impl Middleware for SprintContractMiddleware {
    async fn on_agent_start(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        // Store contract in extensions for evaluator / other middleware to read.
        ctx.extensions.insert(self.contract.clone());
        Ok(())
    }

    async fn before_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        _messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        // Append contract to system prompt so the LLM sees the completion criteria.
        let prompt_section = self.contract.to_prompt();
        ctx.system_prompt.push_str("\n\n");
        ctx.system_prompt.push_str(&prompt_section);
        Ok(())
    }

    fn name(&self) -> &str {
        "sprint_contract"
    }

    fn priority(&self) -> i32 {
        // Run after security (1000) and guardrails (2000), at context level (3000).
        MiddlewarePriority::CONTEXT
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alva_agent_core::middleware::Extensions;

    #[test]
    fn contract_builder_works() {
        let contract = SprintContract::new("Build login")
            .with_deliverable("POST endpoint")
            .with_verification("returns 200")
            .with_constraint("no plaintext passwords");

        assert_eq!(contract.goal, "Build login");
        assert_eq!(contract.deliverables.len(), 1);
        assert_eq!(contract.verifications.len(), 1);
        assert_eq!(contract.constraints.len(), 1);
    }

    #[test]
    fn contract_prompt_contains_all_sections() {
        let contract = SprintContract::new("Test goal")
            .with_deliverable("Deliverable A")
            .with_verification("Check A")
            .with_constraint("Constraint X");

        let prompt = contract.to_prompt();
        assert!(prompt.contains("Test goal"));
        assert!(prompt.contains("Deliverable A"));
        assert!(prompt.contains("Check A"));
        assert!(prompt.contains("Constraint X"));
        assert!(prompt.contains("Sprint Contract"));
    }

    #[tokio::test]
    async fn middleware_injects_contract_into_system_prompt() {
        let contract = SprintContract::new("Implement feature X")
            .with_deliverable("New endpoint")
            .with_verification("Returns 200");

        let mw = SprintContractMiddleware::new(contract);

        let mut ctx = MiddlewareContext {
            session_id: "test-session".into(),
            system_prompt: "You are a helpful assistant.".into(),
            messages: vec![],
            extensions: Extensions::new(),
        };
        let mut messages = vec![];

        mw.on_agent_start(&mut ctx).await.unwrap();
        mw.before_llm_call(&mut ctx, &mut messages).await.unwrap();

        assert!(ctx.system_prompt.contains("Sprint Contract"));
        assert!(ctx.system_prompt.contains("Implement feature X"));
        assert!(ctx.system_prompt.contains("New endpoint"));
    }

    #[tokio::test]
    async fn middleware_stores_contract_in_extensions() {
        let contract = SprintContract::new("Test")
            .with_deliverable("D1");

        let mw = SprintContractMiddleware::new(contract);

        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: String::new(),
            messages: vec![],
            extensions: Extensions::new(),
        };

        mw.on_agent_start(&mut ctx).await.unwrap();

        let stored = ctx.extensions.get::<SprintContract>();
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().goal, "Test");
    }
}
