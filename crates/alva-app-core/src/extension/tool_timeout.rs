//! Tool timeout middleware (120s default).

use std::sync::Arc;

use async_trait::async_trait;

use super::{Extension, HostAPI};

pub struct ToolTimeoutExtension;

#[async_trait]
impl Extension for ToolTimeoutExtension {
    fn name(&self) -> &str { "tool-timeout" }
    fn description(&self) -> &str { "120s timeout per tool execution" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()));
    }
}
