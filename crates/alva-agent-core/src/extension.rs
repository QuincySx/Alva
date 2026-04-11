//! Extension — the primary extensibility point for agents.
//!
//! An Extension is a capability package that registers tools, middleware,
//! and configuration into an agent during construction.
//!
//! ```rust,ignore
//! use alva_agent_core::extension::{Extension, ExtensionAPI};
//!
//! struct MyExtension;
//!
//! impl Extension for MyExtension {
//!     fn name(&self) -> &str { "my-extension" }
//!
//!     fn activate(&self, api: &mut ExtensionAPI) {
//!         api.add_tool(Box::new(MyTool));
//!         api.add_middleware(Arc::new(MyMiddleware));
//!     }
//! }
//! ```

use std::sync::Arc;

use alva_types::tool::Tool;

use crate::middleware::Middleware;

/// A capability package that contributes tools and middleware to an agent.
///
/// Extensions are activated once during agent construction. They receive
/// an [`ExtensionAPI`] through which they register their contributions.
///
/// This is the **only** public extensibility point — BaseAgent users
/// interact exclusively with Extensions, not with raw Tool/Middleware types.
pub trait Extension: Send + Sync {
    /// Unique name for this extension.
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str { "" }

    /// Called once during agent construction.
    /// Register tools, middleware, and perform setup here.
    fn activate(&self, api: &mut ExtensionAPI);
}

/// API surface available to Extensions during activation.
///
/// Collects tools and middleware that will be registered into the agent
/// after all extensions have been activated.
pub struct ExtensionAPI {
    tools: Vec<Box<dyn Tool>>,
    middleware: Vec<Arc<dyn Middleware>>,
}

impl ExtensionAPI {
    /// Create an empty API context.
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            middleware: Vec::new(),
        }
    }

    /// Register a tool.
    pub fn add_tool(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Register multiple tools.
    pub fn add_tools(&mut self, tools: Vec<Box<dyn Tool>>) {
        self.tools.extend(tools);
    }

    /// Register a middleware.
    pub fn add_middleware(&mut self, mw: Arc<dyn Middleware>) {
        self.middleware.push(mw);
    }

    /// Register multiple middleware.
    pub fn add_middlewares(&mut self, mws: Vec<Arc<dyn Middleware>>) {
        self.middleware.extend(mws);
    }

    /// How many tools have been registered so far.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// How many middleware have been registered so far.
    pub fn middleware_count(&self) -> usize {
        self.middleware.len()
    }

    /// Drain all collected tools (consumes them).
    pub fn drain_tools(&mut self) -> Vec<Box<dyn Tool>> {
        std::mem::take(&mut self.tools)
    }

    /// Drain all collected middleware (consumes them).
    pub fn drain_middleware(&mut self) -> Vec<Arc<dyn Middleware>> {
        std::mem::take(&mut self.middleware)
    }
}

impl Default for ExtensionAPI {
    fn default() -> Self {
        Self::new()
    }
}
