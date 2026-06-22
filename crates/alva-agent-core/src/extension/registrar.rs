//! Registrar — single cross-layer plugin setup handle.
//! Plugin::register() uses it to register tools / middleware / bus services /
//! system-prompt fragments / commands.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use alva_kernel_abi::scope::context::ContextLayer;
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::{BusHandle, BusWriter, LanguageModel};
use alva_kernel_core::middleware::Middleware;

use super::host::{PluginHost, RegisteredCommand};
use super::phase::{PhaseContribution, PhaseHandler, PhaseHandlerMiddleware};

#[derive(Debug, Clone, Default)]
pub(crate) struct PluginContribution {
    pub registered_tool_names: Vec<String>,
    pub middleware_names: Vec<String>,
    pub phase_contribution_names: Vec<String>,
    pub command_names: Vec<String>,
    pub system_prompt_fragments: usize,
}

/// Single assembly handle passed to `Plugin::register()`.
///
/// Internal mutability is used throughout (`Mutex` / `RwLock`) so all
/// registration methods take `&self` — callers never need `&mut self`.
pub struct Registrar {
    host: Arc<RwLock<PluginHost>>,
    plugin_name: String,
    bus: BusHandle,
    bus_writer: BusWriter,
    workspace: PathBuf,
    // 装配期发生 panic 才会中毒,此阶段无可恢复,unwrap 传播是有意的。
    tools: Mutex<Vec<Box<dyn Tool>>>,
    contribution: Mutex<PluginContribution>,
}

impl Registrar {
    pub fn new(
        host: Arc<RwLock<PluginHost>>,
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
            contribution: Mutex::new(PluginContribution::default()),
        }
    }

    /// 注册一个 LLM 可调用的 tool。
    ///
    /// 取具体类型,内部装箱——调用点直接 `r.tool(MyTool::new())`,无需手动 `Box::new`。
    pub fn tool<T: Tool + 'static>(&self, t: T) {
        self.contribution
            .lock()
            .unwrap()
            .registered_tool_names
            .push(t.name().to_string());
        self.tools.lock().unwrap().push(Box::new(t));
    }

    /// 注册一组 LLM 可调用的 tool(批量,已装箱)。
    ///
    /// 适用于返回 `Vec<Box<dyn Tool>>` 的 preset 函数。
    pub fn tools(&self, ts: Vec<Box<dyn Tool>>) {
        let names = ts.iter().map(|t| t.name().to_string());
        self.contribution
            .lock()
            .unwrap()
            .registered_tool_names
            .extend(names);
        self.tools.lock().unwrap().extend(ts);
    }

    /// 注册一个运行期洋葱中间件。
    pub fn middleware(&self, mw: Arc<dyn Middleware>) {
        self.contribution
            .lock()
            .unwrap()
            .middleware_names
            .push(mw.name().to_string());
        self.host.write().unwrap().register_middleware(mw);
    }

    /// Register a contribution to the stable runtime phase timeline.
    ///
    /// Phase contributions are the common assembly product for semantic
    /// plugin helpers such as context, observer, policy, and remote AEP
    /// event subscriptions. The current kernel still executes many of
    /// these through `MiddlewareStack`, but plugins should target phases
    /// rather than raw middleware when they mean a lifecycle point.
    pub fn phase(&self, contribution: PhaseContribution) {
        self.contribution
            .lock()
            .unwrap()
            .phase_contribution_names
            .push(contribution.name.clone());
        self.host
            .write()
            .unwrap()
            .register_phase_contribution(self.plugin_name.clone(), contribution);
    }

    /// Register an executable phase contribution.
    ///
    /// This records the stable phase contribution and, until kernel-core
    /// has a native phase executor, compiles it into a generic middleware
    /// adapter named `phase:<contribution.name>`.
    pub fn phase_handler(&self, handler: Arc<dyn PhaseHandler>) {
        let contribution = handler.contribution();
        self.contribution
            .lock()
            .unwrap()
            .phase_contribution_names
            .push(contribution.name.clone());
        let middleware = Arc::new(PhaseHandlerMiddleware::new(handler, contribution.clone()));
        let mut host = self.host.write().unwrap();
        host.register_phase_contribution(self.plugin_name.clone(), contribution);
        host.register_middleware(middleware);
    }

    /// 向 typed bus 提供一个能力(供运行期/晚期读取)。
    ///
    /// **单值语义**:bus 按类型 `T` 存单个值,后 `provide::<T>` 覆盖先前的——
    /// 不会报错也不会合并。同一 `T` 应只由一个 plugin 提供;若需"多个同类型贡献者"
    /// (如多种通信渠道),让那个 `T` 自身是个注册表(如 `SpawnCommunicationRegistry`),
    /// 贡献者在 `finalize()` 里往注册表登记,而不是各自 `provide` 同一 `T`。
    pub fn provide<T: Send + Sync + ?Sized + 'static>(&self, value: Arc<T>) {
        self.bus_writer.provide(value);
    }

    /// 在指定 ContextLayer 追加一段 system prompt。
    ///
    /// The layer controls cache placement:
    /// - `AlwaysPresent` / `OnDemand` / `Memory` → stable (cacheable) bucket
    /// - `RuntimeInject` → dynamic (per-turn, not cached) bucket
    pub fn system_prompt(&self, layer: ContextLayer, text: impl Into<String>) {
        self.contribution.lock().unwrap().system_prompt_fragments += 1;
        self.host.write().unwrap().append_system_prompt(
            self.plugin_name.clone(),
            layer,
            text.into(),
        );
    }

    /// 注册一个 /command(元数据)。
    pub fn command(&self, name: impl Into<String>, description: impl Into<String>) {
        let name = name.into();
        self.contribution
            .lock()
            .unwrap()
            .command_names
            .push(name.clone());
        self.host
            .write()
            .unwrap()
            .register_command(RegisteredCommand {
                name,
                description: description.into(),
                source_plugin: self.plugin_name.clone(),
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

    /// Drain and return all tools that were registered via [`tool`] / [`tools`].
    ///
    /// Called by the builder after all plugins have run `register()`.
    pub(crate) fn take_tools(&self) -> Vec<Box<dyn Tool>> {
        let mut guard = self.tools.lock().unwrap();
        std::mem::take(&mut *guard)
    }

    pub(crate) fn contribution(&self) -> PluginContribution {
        self.contribution.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// LateContext — available after all plugins have registered and the full
// tool set + model are known (passed to Plugin::finalize()).
// ---------------------------------------------------------------------------

/// Late-phase context: all plugin `register()` calls have finished; model and
/// the complete tool list are now available. Passed to [`Plugin::finalize`].
///
/// [`Plugin::finalize`]: super::plugin::Plugin::finalize
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
    use alva_kernel_abi::{AgentError, Bus, ToolExecutionContext, ToolOutput};
    use async_trait::async_trait;

    // ------------------------------------------------------------------
    // Minimal Tool stub — only `name` matters for the round-trip test.
    // ------------------------------------------------------------------
    struct StubTool {
        label: &'static str,
    }

    #[async_trait]
    impl Tool for StubTool {
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

    fn make_registrar() -> Registrar {
        let host = Arc::new(RwLock::new(PluginHost::new()));
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

        r.tool(StubTool { label: "alpha" });
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
