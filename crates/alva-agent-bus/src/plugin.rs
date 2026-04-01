// INPUT:  crate::handle::BusHandle, crate::writer::BusWriter, std::sync::Arc
// OUTPUT: PluginRegistrar, BusPlugin (trait)
// POS:    Plugin system for bus capability registration — controlled write-only registrar + two-phase plugin lifecycle.
use std::sync::Arc;

use crate::handle::BusHandle;
use crate::writer::BusWriter;

/// Controlled capability registrar given to plugins.
///
/// Wraps a `BusWriter` internally but tracks the plugin name for
/// logging and debugging. Plugins receive this during registration,
/// NOT a raw `BusWriter`.
///
/// **Compile-time guarantees:**
/// - No `get()` / `require()` — register phase is write-only.
///   Plugins cannot consume capabilities during registration because
///   other plugins may not have registered yet.
/// - No `emit()` / `subscribe()` — events are for the start phase.
///
/// **Runtime guarantees:**
/// - If a type was already registered (by framework or another plugin),
///   `provide()` logs a warning with both plugin names before overwriting.
pub struct PluginRegistrar<'a> {
    writer: &'a BusWriter,
    plugin_name: &'a str,
    registered: Vec<&'static str>,
}

impl<'a> PluginRegistrar<'a> {
    /// Create a new registrar. Called by the framework, not by plugins.
    pub fn new(writer: &'a BusWriter, plugin_name: &'a str) -> Self {
        Self {
            writer,
            plugin_name,
            registered: Vec::new(),
        }
    }

    /// Register a capability. Logged with the plugin name automatically.
    ///
    /// If the same type was already registered (by framework or another plugin),
    /// a warning is logged and the old value is overwritten.
    pub fn provide<T: Send + Sync + ?Sized + 'static>(&mut self, value: Arc<T>) {
        let type_name = std::any::type_name::<T>();

        // Detect conflict: warn if overwriting
        if self.writer.has::<T>() {
            tracing::warn!(
                plugin = %self.plugin_name,
                capability = %type_name,
                "bus plugin overwriting existing capability — was this intentional?"
            );
        }

        tracing::debug!(
            plugin = %self.plugin_name,
            capability = %type_name,
            "bus plugin registering capability"
        );
        self.writer.provide(value);
        self.registered.push(type_name);
    }

    /// What capabilities this plugin registered (for framework logging).
    pub fn registered_capabilities(&self) -> &[&'static str] {
        &self.registered
    }

    /// Plugin name.
    pub fn plugin_name(&self) -> &str {
        self.plugin_name
    }

    // NOTE: No get(), require(), emit(), subscribe() methods.
    // Register phase is write-only by design.
    // Plugins consume capabilities in start(), not register().
}

/// Trait for bus plugins — declares capabilities and event subscriptions.
///
/// Plugins implement this trait instead of touching BusWriter directly.
/// The framework calls `register()` with a controlled `PluginRegistrar`,
/// then `start()` with a read-only `BusHandle`.
///
/// # Example
///
/// ```rust,ignore
/// struct MyPlugin { workspace: PathBuf }
///
/// impl BusPlugin for MyPlugin {
///     fn name(&self) -> &str { "my-plugin" }
///
///     fn register(&self, reg: &mut PluginRegistrar) {
///         reg.provide::<dyn MyService>(Arc::new(MyServiceImpl::new()));
///     }
///
///     fn start(&self, bus: &BusHandle) {
///         let mut rx = bus.subscribe::<SomeEvent>();
///         tokio::spawn(async move {
///             while let Ok(evt) = rx.recv().await {
///                 // handle event
///             }
///         });
///     }
/// }
/// ```
pub trait BusPlugin: Send + Sync {
    /// Plugin name for logging and debugging.
    fn name(&self) -> &str;

    /// Register capabilities on the bus via the controlled registrar.
    ///
    /// Called once during initialization. After this returns, the plugin
    /// cannot register new capabilities — it only has BusHandle (read-only).
    fn register(&self, registrar: &mut PluginRegistrar);

    /// Start background tasks and event subscriptions.
    ///
    /// Called after all plugins have registered, so all capabilities
    /// are available on the bus. The `bus` handle is read-only.
    fn start(&self, _bus: &BusHandle) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;

    trait Greeter: Send + Sync {
        fn greet(&self) -> &str;
    }

    struct HelloGreeter;
    impl Greeter for HelloGreeter {
        fn greet(&self) -> &str {
            "hello"
        }
    }

    struct TestPlugin;
    impl BusPlugin for TestPlugin {
        fn name(&self) -> &str {
            "test-plugin"
        }

        fn register(&self, reg: &mut PluginRegistrar) {
            reg.provide::<dyn Greeter>(Arc::new(HelloGreeter));
        }
    }

    #[test]
    fn plugin_registers_capability() {
        let bus = Bus::new();
        let writer = bus.writer();
        let handle = bus.handle();

        let plugin = TestPlugin;
        let mut reg = PluginRegistrar::new(&writer, plugin.name());
        plugin.register(&mut reg);

        assert_eq!(reg.registered_capabilities().len(), 1);
        assert!(handle.has::<dyn Greeter>());
        assert_eq!(handle.require::<dyn Greeter>().greet(), "hello");
    }

    #[test]
    fn plugin_name_tracked() {
        let bus = Bus::new();
        let writer = bus.writer();

        let plugin = TestPlugin;
        let reg = PluginRegistrar::new(&writer, plugin.name());
        assert_eq!(reg.plugin_name(), "test-plugin");
    }

    #[test]
    fn start_receives_readonly_handle() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let bus = Bus::new();
        let handle = bus.handle();

        static STARTED: AtomicBool = AtomicBool::new(false);

        struct StartPlugin;
        impl BusPlugin for StartPlugin {
            fn name(&self) -> &str {
                "start-test"
            }
            fn register(&self, _reg: &mut PluginRegistrar) {}
            fn start(&self, _bus: &BusHandle) {
                STARTED.store(true, Ordering::SeqCst);
                // _bus has no provide() — compile enforced
            }
        }

        let plugin = StartPlugin;
        plugin.start(&handle);
        assert!(STARTED.load(Ordering::SeqCst));
    }
}
