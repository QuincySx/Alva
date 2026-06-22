//! EvaluationPlugin — wires a SprintContract in as middleware.

use std::sync::Arc;

use async_trait::async_trait;

use crate::extension::evaluation::{SprintContract, SprintContractMiddleware};
use crate::extension::{Plugin, Registrar};

/// QA evaluation with sprint contract enforcement.
pub struct EvaluationPlugin {
    contract: Option<SprintContract>,
}

impl EvaluationPlugin {
    pub fn new() -> Self {
        Self { contract: None }
    }

    pub fn with_contract(mut self, contract: SprintContract) -> Self {
        self.contract = Some(contract);
        self
    }
}

impl Default for EvaluationPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for EvaluationPlugin {
    fn name(&self) -> &str {
        "evaluation"
    }
    fn description(&self) -> &str {
        "QA evaluation and sprint contract enforcement"
    }

    async fn register(&self, r: &Registrar) {
        if let Some(contract) = &self.contract {
            r.middleware(Arc::new(SprintContractMiddleware::new(contract.clone())));
        }
    }
}
