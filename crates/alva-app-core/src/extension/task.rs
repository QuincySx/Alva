//! Task management tools: create, update, get, list, output, stop.

use alva_kernel_abi::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct TaskExtension;

#[async_trait]
impl Extension for TaskExtension {
    fn name(&self) -> &str { "task" }
    fn description(&self) -> &str { "Task management" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::task_management() }
}
