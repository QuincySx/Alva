// INPUT: kernel Tool/ContextHooks contracts and middleware extension point
// OUTPUT: SpawnContextPolicy and per-child SpawnContextBinding
// POS: App-level optional reference/retrieval policy inheritance; no SDK assembly policy.

use std::sync::Arc;

use alva_kernel_abi::scope::context::ContextHooks;
use alva_kernel_abi::{bus_cap, Tool};
use alva_kernel_core::middleware::Middleware;

/// A fresh binding for one actual child AgentSession, not its scope ID.
/// The retrieval tool only reads that child's durable context; it does not
/// grant file, shell, MCP or parent-session authority. Its assembly hook runs
/// before communication hooks, with a middleware check before provider calls.
pub struct SpawnContextBinding {
    pub retrieval_tool: Arc<dyn Tool>,
    pub middleware: Arc<dyn Middleware>,
    pub assembly_hook: Arc<dyn ContextHooks>,
    pub system_prompt: String,
}

/// Bus Capability: bind an optional durable-context policy to each child.
///
/// **Provider**: product reference-first Plugin, during registration.
/// **Consumers**: app-core AgentSpawnTool after creating the child session.
/// **Why bus**: the product repository lives above app-core; recursive spawn
/// must discover its optional policy without an app-to-engine dependency.
#[bus_cap]
pub trait SpawnContextPolicy: Send + Sync {
    fn bind(&self, child_session_id: &str) -> SpawnContextBinding;
}
