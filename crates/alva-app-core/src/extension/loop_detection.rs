//! Loop detection middleware.

use std::sync::Arc;

use async_trait::async_trait;

use super::{Extension, HostAPI};

pub struct LoopDetectionExtension;

#[async_trait]
impl Extension for LoopDetectionExtension {
    fn name(&self) -> &str { "loop-detection" }
    fn description(&self) -> &str { "Detect repeated tool calls and break loops" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_kernel_core::builtins::LoopDetectionMiddleware::new()));
    }
}
