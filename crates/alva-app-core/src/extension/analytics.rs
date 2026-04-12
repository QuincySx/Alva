//! Telemetry and event tracking.

use std::path::PathBuf;

use async_trait::async_trait;

use super::{Extension, ExtensionContext};

pub struct AnalyticsExtension {
    log_path: Option<PathBuf>,
}

impl AnalyticsExtension {
    pub fn new(log_path: Option<PathBuf>) -> Self {
        Self { log_path }
    }
}

#[async_trait]
impl Extension for AnalyticsExtension {
    fn name(&self) -> &str { "analytics" }
    fn description(&self) -> &str { "Telemetry and event tracking" }

    async fn configure(&self, ctx: &ExtensionContext) {
        let path = self.log_path.clone()
            .unwrap_or_else(|| ctx.workspace.join(".alva/analytics.jsonl"));
        tracing::debug!(path = %path.display(), "analytics sink configured");
    }
}
