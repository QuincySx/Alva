//! Task management tools: create, update, get, list, output, stop.

use alva_agent_core::extension::Extension;
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct TaskExtension;

#[async_trait]
impl Extension for TaskExtension {
    fn name(&self) -> &str { "task" }
    fn description(&self) -> &str { "Task management" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { crate::tool_presets::task_management() }
}
