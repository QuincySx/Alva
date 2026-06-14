//! Registrar — single cross-layer setup handle (replaces HostAPI + ExtensionContext).
//! Plugin::register() uses it to register tools / middleware / bus services /
//! system-prompt fragments / commands.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use alva_kernel_abi::scope::context::ContextLayer;
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::{BusHandle, BusWriter, LanguageModel};
use alva_kernel_core::middleware::Middleware;

use super::host::{ExtensionHost, RegisteredCommand};

/// Single assembly handle passed to `Plugin::register()`.
///
/// Internal mutability is used throughout (`Mutex` / `RwLock`) so all
/// registration methods take `&self` — callers never need `&mut self`.
pub struct Registrar<'a> {
    host: Arc<RwLock<ExtensionHost>>,
    plugin_name: String,
    bus: BusHandle,
    bus_writer: BusWriter,
    workspace: PathBuf,
    tools: Mutex<Vec<Box<dyn Tool>>>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Registrar<'a> {
    pub fn new(
        host: Arc<RwLock<ExtensionHost>>,
        plugin_name: String,
        bus: BusHandle,
        bus_writer: BusWriter,
        workspace: PathBuf,
    ) -> Self {
        Self {
            host,
            plugin_name,
            bus,
            bus_writer,
            workspace,
            tools: Mutex::new(Vec::new()),
            _marker: std::marker::PhantomData,
        }
    }

    /// Register a single tool.
    pub fn tool(&self, t: Box<dyn Tool>) {
        self.tools.lock().unwrap().push(t);
    }

    /// Register multiple tools at once.
    pub fn tools(&self, ts: Vec<Box<dyn Tool>>) {
        self.tools.lock().unwrap().extend(ts);
    }

    /// Register a middleware layer.
    pub fn middleware(&self, mw: Arc<dyn Middleware>) {
        self.host.write().unwrap().register_middleware(mw);
    }

    /// Publish a capability on the bus (available to all downstream layers).
    pub fn provide<T: Send + Sync + ?Sized + 'static>(&self, value: Arc<T>) {
        self.bus_writer.provide(value);
    }

    /// Append a system-prompt fragment at the given context layer.
    ///
    /// The layer controls cache placement:
    /// - `AlwaysPresent` / `OnDemand` / `Memory` → stable (cacheable) bucket
    /// - `RuntimeInject` → dynamic (per-turn, not cached) bucket
    pub fn system_prompt(&self, layer: ContextLayer, text: impl Into<String>) {
        self.host
            .write()
            .unwrap()
            .append_system_prompt(self.plugin_name.clone(), layer, text.into());
    }

    /// Register a slash-command (metadata only; routing is handled later).
    pub fn command(&self, name: &str, description: &str) {
        self.host.write().unwrap().register_command(RegisteredCommand {
            name: name.to_string(),
            description: description.to_string(),
            source_extension: self.plugin_name.clone(),
        });
    }

    // ---- accessors ----------------------------------------------------------

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn bus(&self) -> &BusHandle {
        &self.bus
    }

    pub fn bus_writer(&self) -> &BusWriter {
        &self.bus_writer
    }

    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    /// Drain and return all tools that were registered via [`tool`] / [`tools`].
    ///
    /// Called by the builder after all plugins have run `register()`.
    pub fn take_tools(&self) -> Vec<Box<dyn Tool>> {
        std::mem::take(&mut self.tools.lock().unwrap())
    }
}

// ---------------------------------------------------------------------------
// LateContext — available after all plugins have registered and the full
// tool set + model are known (passed to Plugin::finalize()).
// ---------------------------------------------------------------------------

/// Late-phase context: all plugin `register()` calls have finished; model and
/// the complete tool list are now available.
///
/// Field layout mirrors [`FinalizeContext`](super::context::FinalizeContext)
/// so the two types can be aligned in the future if the old Extension API is
/// retired.
pub struct LateContext {
    pub bus: BusHandle,
    pub bus_writer: BusWriter,
    pub workspace: PathBuf,
    pub model: Arc<dyn LanguageModel>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub max_iterations: u32,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use alva_kernel_abi::{AgentError, Bus, ToolExecutionContext, ToolOutput};

    // ------------------------------------------------------------------
    // Minimal Tool stub — only `name` matters for the round-trip test.
    // ------------------------------------------------------------------
    struct StubTool {
        label: &'static str,
    }

    #[async_trait]
    impl Tool for StubTool {
        fn name(&self) -> &str { self.label }
        fn description(&self) -> &str { "stub" }
        fn parameters_schema(&self) -> serde_json::Value { serde_json::json!({}) }
        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &dyn ToolExecutionContext,
        ) -> Result<ToolOutput, AgentError> {
            unimplemented!("stub — not called in tests")
        }
    }

    fn make_registrar() -> Registrar<'static> {
        let host = Arc::new(RwLock::new(ExtensionHost::new()));
        let bus = Bus::new();
        let writer = bus.writer();
        let handle = writer.handle();
        Registrar::new(
            host,
            "test-plugin".to_string(),
            handle,
            writer,
            PathBuf::from("/tmp"),
        )
    }

    #[test]
    fn tool_take_round_trip() {
        let r = make_registrar();
        assert!(r.take_tools().is_empty(), "initially empty");

        r.tool(Box::new(StubTool { label: "alpha" }));
        r.tools(vec![
            Box::new(StubTool { label: "beta" }),
            Box::new(StubTool { label: "gamma" }),
        ]);

        let taken = r.take_tools();
        assert_eq!(taken.len(), 3);
        assert_eq!(taken[0].name(), "alpha");
        assert_eq!(taken[1].name(), "beta");
        assert_eq!(taken[2].name(), "gamma");

        // After take, the internal list is drained.
        assert!(r.take_tools().is_empty(), "drained after take");
    }

    #[test]
    fn plugin_name_and_workspace_accessors() {
        let r = make_registrar();
        assert_eq!(r.plugin_name(), "test-plugin");
        assert_eq!(r.workspace(), Path::new("/tmp"));
    }
}
