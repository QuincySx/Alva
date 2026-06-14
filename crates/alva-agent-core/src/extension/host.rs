//! ExtensionHost — runtime container for extension middleware and commands.

use std::sync::{Arc, RwLock};
use alva_kernel_abi::CancellationToken;

/// Command registered by an extension (metadata only).
pub struct RegisteredCommand {
    pub name: String,
    pub description: String,
    pub source_extension: String,
}

/// Runtime container for extension middleware and commands.
pub struct ExtensionHost {
    middlewares: Vec<Arc<dyn alva_kernel_core::middleware::Middleware>>,
    commands: Vec<RegisteredCommand>,
    cancel_token: Option<Arc<std::sync::Mutex<CancellationToken>>>,
    /// System-prompt fragments contributed by extensions during
    /// `configure()`. Each entry is `(extension_name, layer, text)` in
    /// the order it was appended. The builder drains the collection
    /// after configure/finalize and groups by layer when assembling the
    /// final prompt: stable layers (L0 / L1 / L3) form the cacheable
    /// prefix, RuntimeInject is appended last (volatile bucket).
    system_prompt_additions:
        Vec<(String, alva_kernel_abi::scope::context::ContextLayer, String)>,
}

impl ExtensionHost {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
            commands: Vec::new(),
            cancel_token: None,
            system_prompt_additions: Vec::new(),
        }
    }

    pub fn register_middleware(&mut self, mw: Arc<dyn alva_kernel_core::middleware::Middleware>) {
        self.middlewares.push(mw);
    }

    /// Take all registered middleware (drains the collection).
    pub fn take_middlewares(&mut self) -> Vec<Arc<dyn alva_kernel_core::middleware::Middleware>> {
        std::mem::take(&mut self.middlewares)
    }

    pub fn register_command(&mut self, cmd: RegisteredCommand) {
        self.commands.push(cmd);
    }

    /// Record a system-prompt fragment contributed by an extension at
    /// a given context layer. The builder uses the layer to decide
    /// whether the fragment ends up in the stable (cacheable) bucket
    /// or the dynamic (per-turn) tail.
    pub fn append_system_prompt(
        &mut self,
        extension_name: String,
        layer: alva_kernel_abi::scope::context::ContextLayer,
        text: String,
    ) {
        self.system_prompt_additions
            .push((extension_name, layer, text));
    }

    /// Take all accumulated system-prompt fragments (drains the collection).
    pub fn take_system_prompt_additions(
        &mut self,
    ) -> Vec<(String, alva_kernel_abi::scope::context::ContextLayer, String)> {
        std::mem::take(&mut self.system_prompt_additions)
    }

    pub fn bind_agent(&mut self, cancel: Arc<std::sync::Mutex<CancellationToken>>) {
        self.cancel_token = Some(cancel);
    }

    pub fn commands(&self) -> &[RegisteredCommand] {
        &self.commands
    }
}

impl Default for ExtensionHost {
    fn default() -> Self {
        Self::new()
    }
}

/// API handle given to extensions during activate().
///
/// `Clone` is cheap — `host` is `Arc` and `extension_name` is a short
/// string. Aggregator extensions (e.g. `SubprocessLoaderPlugin`)
/// clone the handle in `activate()` and reuse it in `configure()`.
#[derive(Clone)]
pub struct HostAPI {
    host: Arc<RwLock<ExtensionHost>>,
    extension_name: String,
}

impl HostAPI {
    pub fn new(host: Arc<RwLock<ExtensionHost>>, extension_name: String) -> Self {
        Self { host, extension_name }
    }

    /// Register a middleware. Called during activate(), collected by the builder.
    pub fn middleware(&self, mw: Arc<dyn alva_kernel_core::middleware::Middleware>) {
        let mut host = self.host.write().unwrap();
        host.register_middleware(mw);
    }

    /// Register a /command (metadata only, routing is P3).
    pub fn register_command(&self, name: &str, description: &str) {
        let mut host = self.host.write().unwrap();
        host.register_command(RegisteredCommand {
            name: name.to_string(),
            description: description.to_string(),
            source_extension: self.extension_name.clone(),
        });
    }

    /// Append a fragment to the agent's system prompt at a given
    /// context layer. The layer decides cache placement:
    ///   - `AlwaysPresent` / `OnDemand` / `Memory` → stable bucket
    ///     (cacheable prefix)
    ///   - `RuntimeInject` → dynamic bucket (per-turn volatile, not
    ///     cached)
    ///
    /// Use `AlwaysPresent` for things like `<project_context>` /
    /// CLAUDE.md / skill indexes. Use `RuntimeInject` for git status,
    /// today's date, current directory listings, anything that varies
    /// per turn.
    pub fn append_system_prompt(
        &self,
        layer: alva_kernel_abi::scope::context::ContextLayer,
        text: impl Into<String>,
    ) {
        let mut host = self.host.write().unwrap();
        host.append_system_prompt(self.extension_name.clone(), layer, text.into());
    }

    /// Cancel the current agent loop.
    pub fn shutdown(&self) {
        let host = self.host.read().unwrap();
        if let Some(ref cancel) = host.cancel_token {
            let token = cancel.lock().unwrap();
            token.cancel();
        }
    }

    /// Get the extension name this API belongs to.
    pub fn extension_name(&self) -> &str {
        &self.extension_name
    }
}
