//! Checkpoint middleware — file backups before tool execution.

use std::sync::Arc;

use async_trait::async_trait;

use super::{Extension, HostAPI};

pub struct CheckpointExtension;

#[async_trait]
impl Extension for CheckpointExtension {
    fn name(&self) -> &str { "checkpoint" }
    fn description(&self) -> &str { "File checkpoint before tool execution" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_host_native::middleware::CheckpointMiddleware::new()));
    }
}
