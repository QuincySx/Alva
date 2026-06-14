// INPUT:  std::sync::{Arc, Mutex}, std::collections::HashMap, async_trait, alva_kernel_abi::{SpawnCommunication, SpawnCommunicationRegistry}, crate::extension::{Extension, ExtensionContext}
// OUTPUT: DefaultSpawnCommRegistry, SpawnCommRegistryPlugin
// POS:    Default in-process `SpawnCommunicationRegistry` (`Mutex<HashMap<kind, Arc<dyn SpawnCommunication>>>`) plus the opt-in `SpawnCommRegistryPlugin` that publishes it onto the bus — wiring point for sub-agent comm plugins (e.g. BlackboardCommPlugin).

//! Default `SpawnCommunicationRegistry` implementation + its opt-in
//! Extension wrapper.
//!
//! `SpawnCommRegistryPlugin` provides an empty registry on the bus so
//! `AgentSpawnTool` can look up comm capabilities at spawn time. Other
//! extensions (e.g. `BlackboardCommPlugin`) register their
//! capabilities via `Extension::configure()` by pulling the registry out
//! of the bus and calling `register(...)`. Without this extension,
//! `SpawnInput.comms: []` still works — only non-empty `comms` will
//! error at spawn time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use alva_kernel_abi::{SpawnCommunication, SpawnCommunicationRegistry};

use alva_agent_core::extension::{Plugin, Registrar};

/// In-process registry backed by a `Mutex<HashMap>`.
///
/// `register()` overwrites any previous entry for the same kind — last
/// registration wins. This mirrors the "default replacement" contract used
/// for `Extension::name()` elsewhere.
pub struct DefaultSpawnCommRegistry {
    inner: Mutex<HashMap<String, Arc<dyn SpawnCommunication>>>,
}

impl DefaultSpawnCommRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for DefaultSpawnCommRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SpawnCommunicationRegistry for DefaultSpawnCommRegistry {
    fn register(&self, ch: Arc<dyn SpawnCommunication>) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(ch.kind().to_string(), ch);
    }

    fn get(&self, kind: &str) -> Option<Arc<dyn SpawnCommunication>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.get(kind).cloned()
    }

    fn list(&self) -> Vec<Arc<dyn SpawnCommunication>> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.values().cloned().collect()
    }
}

/// Opt-in Extension that publishes an empty `DefaultSpawnCommRegistry` on
/// the bus under `dyn SpawnCommunicationRegistry`.
///
/// Other extensions (e.g. `BlackboardCommPlugin`) consume this
/// registry during their own `configure()` to register their
/// `SpawnCommunication` plugins. Without this extension installed, the
/// `AgentSpawnTool` treats any non-empty `comms` spec as an error.
pub struct SpawnCommRegistryPlugin {
    registry: Arc<DefaultSpawnCommRegistry>,
}

impl SpawnCommRegistryPlugin {
    /// Create with a fresh empty registry.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(DefaultSpawnCommRegistry::new()),
        }
    }
}

impl Default for SpawnCommRegistryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SpawnCommRegistryPlugin {
    fn name(&self) -> &str {
        "spawn-comm-registry"
    }

    fn description(&self) -> &str {
        "Provides an empty SpawnCommunicationRegistry to the bus; other \
         extensions (e.g. BlackboardCommPlugin) register communication \
         plugins into it."
    }

    async fn register(&self, r: &Registrar) {
        r.provide::<dyn SpawnCommunicationRegistry>(self.registry.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::{SpawnCommContext, SpawnCommError, SpawnCommHandle};
    use async_trait::async_trait;
    use serde_json::Value;

    struct StubComm(&'static str);

    #[async_trait]
    impl SpawnCommunication for StubComm {
        fn kind(&self) -> &str { self.0 }
        fn description(&self) -> &str { "stub" }
        async fn attach(
            &self,
            _ctx: &SpawnCommContext<'_>,
            _config: Value,
        ) -> Result<SpawnCommHandle, SpawnCommError> {
            Ok(SpawnCommHandle::empty())
        }
    }

    #[test]
    fn register_and_lookup() {
        let reg = DefaultSpawnCommRegistry::new();
        reg.register(Arc::new(StubComm("alpha")));
        reg.register(Arc::new(StubComm("beta")));
        assert!(reg.get("alpha").is_some());
        assert!(reg.get("beta").is_some());
        assert!(reg.get("gamma").is_none());
        assert_eq!(reg.list().len(), 2);
    }

    #[test]
    fn register_overwrites_same_kind() {
        let reg = DefaultSpawnCommRegistry::new();
        reg.register(Arc::new(StubComm("alpha")));
        reg.register(Arc::new(StubComm("alpha")));
        assert_eq!(reg.list().len(), 1);
    }
}
