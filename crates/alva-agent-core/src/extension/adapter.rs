//! ExtensionAsPlugin — transition-period adapter that runs an old `Extension`
//! as a `Plugin`.
//!
//! Phase 6: once all `Extension` implementations have been migrated to
//! `Plugin`, delete this file together with the `Extension` trait.

use async_trait::async_trait;
use std::sync::Arc;

use alva_kernel_abi::tool::Tool;

use super::{ExtensionContext, FinalizeContext, HostAPI};
use super::Extension;
use super::plugin::Plugin;
use super::registrar::{LateContext, Registrar};

/// Wraps a `Box<dyn Extension>` and exposes it as a `Plugin`.
///
/// The register phase replays the old three-step sequence:
/// `tools()` → `activate(HostAPI)` → `configure(ExtensionContext)`.
///
/// The finalize phase maps `LateContext` → `FinalizeContext` 1-to-1.
pub struct ExtensionAsPlugin(pub Box<dyn Extension>);

#[async_trait]
impl Plugin for ExtensionAsPlugin {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    async fn register(&self, r: &Registrar) {
        // Step 1 — collect tools.
        r.tools(self.0.tools().await);

        // Step 2 — activate: hand the extension a HostAPI so it can register
        //           middleware / commands.
        let api = HostAPI::new(r.host_arc(), self.0.name().to_string());
        self.0.activate(&api);

        // Step 3 — configure: give the extension read access to the bus and
        //           workspace so it can initialise internal state.
        let ctx = ExtensionContext {
            bus: r.bus().clone(),
            bus_writer: r.bus_writer().clone(),
            workspace: r.workspace().to_path_buf(),
            tool_names: Vec::new(), // dead field — no reader in the old API
        };
        self.0.configure(&ctx).await;
    }

    async fn finalize(&self, cx: &LateContext) -> Vec<Arc<dyn Tool>> {
        // LateContext and FinalizeContext have identical field layouts; map
        // them by cloning each field.
        let fctx = FinalizeContext {
            bus: cx.bus.clone(),
            bus_writer: cx.bus_writer.clone(),
            workspace: cx.workspace.clone(),
            model: cx.model.clone(),
            tools: cx.tools.clone(),
            max_iterations: cx.max_iterations,
        };
        self.0.finalize(&fctx).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::RwLock;

    use alva_kernel_abi::{AgentError, Bus, ToolExecutionContext, ToolOutput};
    use alva_kernel_abi::tool::Tool as KTool;

    use crate::extension::host::ExtensionHost;
    use crate::extension::registrar::Registrar;
    use crate::extension::Extension;

    // ------------------------------------------------------------------
    // Minimal Tool stub
    // ------------------------------------------------------------------
    struct StubTool {
        label: &'static str,
    }

    #[async_trait]
    impl KTool for StubTool {
        fn name(&self) -> &str {
            self.label
        }
        fn description(&self) -> &str {
            "stub"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &dyn ToolExecutionContext,
        ) -> Result<ToolOutput, AgentError> {
            unimplemented!("stub — not called in tests")
        }
    }

    // ------------------------------------------------------------------
    // Minimal Extension stub — returns one tool; everything else is no-op.
    // ------------------------------------------------------------------
    struct StubExtension;

    #[async_trait]
    impl Extension for StubExtension {
        fn name(&self) -> &str {
            "stub-extension"
        }
        async fn tools(&self) -> Vec<Box<dyn KTool>> {
            vec![Box::new(StubTool { label: "alpha" })]
        }
    }

    fn make_registrar() -> Registrar {
        let host = Arc::new(RwLock::new(ExtensionHost::new()));
        let bus = Bus::new();
        let writer = bus.writer();
        let handle = writer.handle();
        Registrar::new(
            host,
            "stub-extension".to_string(),
            handle,
            writer,
            PathBuf::from("/tmp"),
        )
    }

    #[tokio::test]
    async fn extension_as_plugin_registers_tools() {
        let r = make_registrar();

        let plugin = ExtensionAsPlugin(Box::new(StubExtension));
        plugin.register(&r).await;

        let tools = r.take_tools();
        assert_eq!(tools.len(), 1, "tool registered via Extension should appear in Registrar");
        assert_eq!(tools[0].name(), "alpha");
    }

    // ------------------------------------------------------------------
    // Minimal Middleware stub — all methods default (no-op).
    // ------------------------------------------------------------------
    struct StubMiddleware;

    impl alva_kernel_core::middleware::Middleware for StubMiddleware {
        fn name(&self) -> &str {
            "stub-middleware"
        }
    }

    // Extension that registers a middleware via the HostAPI handed to it in
    // `activate()`. Verifies the key adapter invariant: the HostAPI built by
    // the adapter shares the *same* ExtensionHost that the Registrar owns.
    struct SpyExtension;

    #[async_trait]
    impl Extension for SpyExtension {
        fn name(&self) -> &str {
            "spy-extension"
        }
        fn activate(&self, api: &HostAPI) {
            api.middleware(Arc::new(StubMiddleware));
        }
    }

    #[tokio::test]
    async fn extension_as_plugin_wires_middleware_into_shared_host() {
        let r = make_registrar();

        let plugin = ExtensionAsPlugin(Box::new(SpyExtension));
        plugin.register(&r).await;

        // The middleware must have landed in the *same* host the Registrar owns.
        let mws = r.host_arc().write().unwrap().take_middlewares();
        assert_eq!(mws.len(), 1, "middleware registered via HostAPI must reach the Registrar's host");
        assert_eq!(mws[0].name(), "stub-middleware");
    }
}
