// INPUT:  proxy::RemotePluginProxy, protocol::ToolDef, alva_kernel_abi::Tool
// OUTPUT: RemoteToolAdapter
// POS:    Adapts AEP-declared subprocess tools into kernel Tool implementations.

use std::sync::Arc;

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde_json::Value;

use crate::protocol::ToolDef;
use crate::proxy::RemotePluginProxy;

/// Kernel `Tool` wrapper for one tool declared by a remote AEP plugin.
pub struct RemoteToolAdapter {
    plugin: Arc<RemotePluginProxy>,
    name: String,
    description: String,
    input_schema: Value,
}

impl RemoteToolAdapter {
    pub fn new(plugin: Arc<RemotePluginProxy>, def: ToolDef) -> Self {
        Self {
            plugin,
            name: def.name,
            description: def.description,
            input_schema: def.input_schema,
        }
    }
}

#[async_trait]
impl Tool for RemoteToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        self.plugin
            .call_tool(&self.name, input)
            .await
            .map_err(|error| {
                AgentError::Other(format!("remote tool '{}' failed: {error}", self.name))
            })
    }
}
