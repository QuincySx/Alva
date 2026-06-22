//! PluginHost — runtime container for plugin middleware and commands.

use alva_kernel_abi::CancellationToken;
use std::sync::Arc;

use super::phase::PhaseContribution;

/// Command registered by a plugin (metadata only).
pub struct RegisteredCommand {
    pub name: String,
    pub description: String,
    pub source_plugin: String,
}

/// Runtime container for plugin middleware and commands.
pub struct PluginHost {
    middlewares: Vec<Arc<dyn alva_kernel_core::middleware::Middleware>>,
    phase_contributions: Vec<(String, PhaseContribution)>,
    commands: Vec<RegisteredCommand>,
    cancel_token: Option<Arc<std::sync::Mutex<CancellationToken>>>,
    /// System-prompt fragments contributed by plugins during
    /// `register()`. Each entry is `(plugin_name, layer, text)` in
    /// the order it was appended. The builder drains the collection
    /// after configure/finalize and groups by layer when assembling the
    /// final prompt: stable layers (L0 / L1 / L3) form the cacheable
    /// prefix, RuntimeInject is appended last (volatile bucket).
    system_prompt_additions: Vec<(
        String,
        alva_kernel_abi::scope::context::ContextLayer,
        String,
    )>,
}

impl PluginHost {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
            phase_contributions: Vec::new(),
            commands: Vec::new(),
            cancel_token: None,
            system_prompt_additions: Vec::new(),
        }
    }

    pub fn register_middleware(&mut self, mw: Arc<dyn alva_kernel_core::middleware::Middleware>) {
        self.middlewares.push(mw);
    }

    pub fn register_phase_contribution(
        &mut self,
        plugin_name: String,
        contribution: PhaseContribution,
    ) {
        self.phase_contributions.push((plugin_name, contribution));
    }

    pub fn take_phase_contributions(&mut self) -> Vec<(String, PhaseContribution)> {
        std::mem::take(&mut self.phase_contributions)
    }

    /// Take all registered middleware (drains the collection).
    pub fn take_middlewares(&mut self) -> Vec<Arc<dyn alva_kernel_core::middleware::Middleware>> {
        std::mem::take(&mut self.middlewares)
    }

    pub fn register_command(&mut self, cmd: RegisteredCommand) {
        self.commands.push(cmd);
    }

    /// Record a system-prompt fragment contributed by a plugin at
    /// a given context layer. The builder uses the layer to decide
    /// whether the fragment ends up in the stable (cacheable) bucket
    /// or the dynamic (per-turn) tail.
    pub fn append_system_prompt(
        &mut self,
        plugin_name: String,
        layer: alva_kernel_abi::scope::context::ContextLayer,
        text: String,
    ) {
        self.system_prompt_additions
            .push((plugin_name, layer, text));
    }

    /// Take all accumulated system-prompt fragments (drains the collection).
    pub fn take_system_prompt_additions(
        &mut self,
    ) -> Vec<(
        String,
        alva_kernel_abi::scope::context::ContextLayer,
        String,
    )> {
        std::mem::take(&mut self.system_prompt_additions)
    }

    pub fn bind_agent(&mut self, cancel: Arc<std::sync::Mutex<CancellationToken>>) {
        self.cancel_token = Some(cancel);
    }

    pub fn commands(&self) -> &[RegisteredCommand] {
        &self.commands
    }
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}
