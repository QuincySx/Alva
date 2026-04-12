//! EvaluationExtension — wires a SprintContract in as middleware.

use std::sync::Arc;

use async_trait::async_trait;

use crate::extension::{Extension, HostAPI};
use crate::extension::evaluation::{SprintContract, SprintContractMiddleware};

/// QA evaluation with sprint contract enforcement.
pub struct EvaluationExtension {
    contract: Option<SprintContract>,
}

impl EvaluationExtension {
    pub fn new() -> Self {
        Self { contract: None }
    }

    pub fn with_contract(mut self, contract: SprintContract) -> Self {
        self.contract = Some(contract);
        self
    }
}

impl Default for EvaluationExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for EvaluationExtension {
    fn name(&self) -> &str { "evaluation" }
    fn description(&self) -> &str { "QA evaluation and sprint contract enforcement" }

    fn activate(&self, api: &HostAPI) {
        if let Some(contract) = &self.contract {
            api.middleware(Arc::new(SprintContractMiddleware::new(contract.clone())));
        }
    }
}
