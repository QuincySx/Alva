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
pub struct Registrar {
    host: Arc<RwLock<ExtensionHost>>,
    plugin_name: String,
    bus: BusHandle,
    bus_writer: BusWriter,
    workspace: PathBuf,
    // 装配期发生 panic 才会中毒,此阶段无可恢复,unwrap 传播是有意的。
    tools: Mutex<Vec<Box<dyn Tool>>>,
}

impl Registrar {
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
        }
    }

    /// 注册一个 LLM 可调用的 tool。
    pub fn tool(&self, t: Box<dyn Tool>) {
        self.tools.lock().unwrap().push(t);
    }

    /// 注册一组 LLM 可调用的 tool。
    pub fn tools(&self, ts: Vec<Box<dyn Tool>>) {
        self.tools.lock().unwrap().extend(ts);
    }

    /// 注册一个运行期洋葱中间件。
    pub fn middleware(&self, mw: Arc<dyn Middleware>) {
        self.host.write().unwrap().register_middleware(mw);
    }

    /// 向 typed bus 提供一个能力(供运行期/晚期读取)。
    pub fn provide<T: Send + Sync + ?Sized + 'static>(&self, value: Arc<T>) {
        self.bus_writer.provide(value);
    }

    /// 在指定 ContextLayer 追加一段 system prompt。
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

    /// 注册一个 /command(元数据)。
    pub fn command(&self, name: &str, description: &str) {
        self.host.write().unwrap().register_command(RegisteredCommand {
            name: name.to_string(),
            description: description.to_string(),
            source_extension: self.plugin_name.clone(),
        });
    }

    // ---- accessors ----------------------------------------------------------

    /// 本次装配的 workspace 根目录。
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// 只读 bus 句柄(读取已注册能力 / 订阅事件)。
    pub fn bus(&self) -> &BusHandle {
        &self.bus
    }

    /// 装配期可写 bus 句柄(可注册能力)。
    pub fn bus_writer(&self) -> &BusWriter {
        &self.bus_writer
    }

    /// 当前 plugin 的名字。
    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    /// Inner `Arc<RwLock<ExtensionHost>>` clone — used by the transition-period
    /// `ExtensionAsPlugin` adapter to construct a `HostAPI` without exposing
    /// the host field as `pub`.
    pub(crate) fn host_arc(&self) -> Arc<RwLock<super::host::ExtensionHost>> {
        self.host.clone()
    }

    /// Drain and return all tools that were registered via [`tool`] / [`tools`].
    ///
    /// Called by the builder after all plugins have run `register()`.
    pub(crate) fn take_tools(&self) -> Vec<Box<dyn Tool>> {
        let mut guard = self.tools.lock().unwrap();
        std::mem::take(&mut *guard)
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

    fn make_registrar() -> Registrar {
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
