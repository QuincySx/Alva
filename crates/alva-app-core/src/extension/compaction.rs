//! Context compaction middleware.

use std::sync::Arc;

use async_trait::async_trait;

use super::{Extension, HostAPI};

pub struct CompactionExtension;

#[async_trait]
impl Extension for CompactionExtension {
    fn name(&self) -> &str { "compaction" }
    fn description(&self) -> &str { "Context compaction" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_host_native::middleware::CompactionMiddleware::default()));
    }
}
