//! Dangling tool call validation middleware.

use std::sync::Arc;

use async_trait::async_trait;

use super::{Extension, HostAPI};

pub struct DanglingToolCallExtension;

#[async_trait]
impl Extension for DanglingToolCallExtension {
    fn name(&self) -> &str { "dangling-tool-call" }
    fn description(&self) -> &str { "Validate tool call format and existence" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_kernel_core::builtins::DanglingToolCallMiddleware::new()));
    }
}
