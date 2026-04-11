//! Extension — the primary extensibility point for agents.
//!
//! An Extension is a capability package that provides tools and middleware.
//!
//! ```rust,ignore
//! use alva_agent_core::extension::Extension;
//!
//! struct MyExtension;
//!
//! impl Extension for MyExtension {
//!     fn name(&self) -> &str { "my-extension" }
//!
//!     fn tools(&self) -> Vec<Box<dyn Tool>> {
//!         vec![Box::new(MyTool)]
//!     }
//!
//!     fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
//!         vec![Arc::new(MyMiddleware)]
//!     }
//! }
//! ```

use std::sync::Arc;

use alva_types::tool::Tool;

use crate::middleware::Middleware;

/// A capability package that provides tools and middleware to an agent.
///
/// This is the **only** public extensibility point — BaseAgent users
/// interact exclusively with Extensions, not with raw Tool/Middleware types.
pub trait Extension: Send + Sync {
    /// Unique name for this extension.
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str { "" }

    /// Tools this extension provides.
    fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }

    /// Middleware this extension provides.
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> { vec![] }
}
