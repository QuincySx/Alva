//! Integration tests for the plugin-replacement contract on
//! `BaseAgentBuilder`.
//!
//! `BaseAgentBuilder` no longer exposes `with_memory` / `memory_service` /
//! `security_middleware` setters. Memory and security ship as default
//! plugins (`MemoryPlugin`, `SecurityPlugin` from
//! `alva-agent-extension-builtin`) and the only customization mechanism is
//! to register your own plugin with the same `name()` — the builder
//! detects the duplicate and skips its default. These tests pin that
//! contract.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;

use alva_agent_core::extension::{Plugin, Registrar};
use alva_agent_memory::{InMemoryBackend, MemoryService, NoopEmbeddingProvider};
use alva_agent_security::SecurityGuard;
use alva_app_core::base_agent::BaseAgent;
use alva_test::fixtures::make_assistant_message;
use alva_test::mock_provider::MockLanguageModel;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Test 1: default MemoryPlugin is wired and publishes MemoryService on bus
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_memory_plugin_is_wired() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let model: Arc<dyn alva_kernel_abi::LanguageModel> =
        Arc::new(MockLanguageModel::new().with_response(make_assistant_message("ok")));

    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .build(model)
        .await
        .expect("build should succeed");

    let svc = agent
        .bus()
        .get::<MemoryService>()
        .expect("default MemoryPlugin should publish MemoryService on the bus");

    // Functional sanity check: the default service is empty and writeable.
    svc.store_entry("default-key", "default content", "test")
        .await
        .expect("store_entry on default service");
    let results = svc.search("default", 10).await.expect("search");
    assert!(
        results.iter().any(|e| e.path == "default-key"),
        "default MemoryService should be writable and queryable"
    );

    let snapshot = agent.assembly_snapshot();
    assert!(
        snapshot.plugin_names.iter().any(|name| name == "memory"),
        "default MemoryPlugin should be visible in assembly snapshot: {:?}",
        snapshot.plugin_names
    );
}

// ---------------------------------------------------------------------------
// Test 2: a user-registered "memory" plugin replaces the default
// ---------------------------------------------------------------------------

/// Counts how many times `register()` is called and seeds a marker entry
/// in its own MemoryService, so the test can detect the override took.
struct CustomMemoryPlugin {
    service: Arc<MemoryService>,
    activations: Arc<AtomicUsize>,
}

impl CustomMemoryPlugin {
    fn new(activations: Arc<AtomicUsize>) -> Self {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Box::new(NoopEmbeddingProvider::new());
        Self {
            service: Arc::new(MemoryService::with_backend(backend, embedder)),
            activations,
        }
    }
}

#[async_trait]
impl Plugin for CustomMemoryPlugin {
    fn name(&self) -> &str {
        "memory"
    }

    async fn register(&self, r: &Registrar) {
        self.activations.fetch_add(1, Ordering::SeqCst);
        // Seed a sentinel BEFORE publishing so the assertion below can
        // distinguish our service from a freshly-built default.
        self.service
            .store_entry("custom-sentinel", "from custom ext", "test")
            .await
            .expect("seed sentinel");
        r.provide::<MemoryService>(self.service.clone());
    }
}

#[tokio::test]
async fn custom_memory_plugin_replaces_default() {
    let activations = Arc::new(AtomicUsize::new(0));
    let tmp = tempfile::tempdir().expect("tempdir");
    let model: Arc<dyn alva_kernel_abi::LanguageModel> =
        Arc::new(MockLanguageModel::new().with_response(make_assistant_message("ok")));

    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .plugin(Box::new(CustomMemoryPlugin::new(activations.clone())))
        .build(model)
        .await
        .expect("build should succeed");

    assert_eq!(
        activations.load(Ordering::SeqCst),
        1,
        "custom MemoryPlugin's register() should have run exactly once"
    );

    let svc = agent
        .bus()
        .get::<MemoryService>()
        .expect("custom MemoryPlugin should publish MemoryService on the bus");
    let results = svc.search("sentinel", 10).await.expect("search");
    assert!(
        results.iter().any(|e| e.path == "custom-sentinel"),
        "expected the custom sentinel entry — instead got: {:?}",
        results.iter().map(|e| &e.path).collect::<Vec<_>>()
    );

    let snapshot = agent.assembly_snapshot();
    assert_eq!(
        snapshot
            .plugin_names
            .iter()
            .filter(|name| name.as_str() == "memory")
            .count(),
        1,
        "custom memory plugin should replace, not duplicate, the default: {:?}",
        snapshot.plugin_names
    );
}

// ---------------------------------------------------------------------------
// Test 3: default SecurityPlugin is wired and publishes SecurityGuard on bus
// ---------------------------------------------------------------------------

#[tokio::test]
async fn default_security_plugin_is_wired() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let model: Arc<dyn alva_kernel_abi::LanguageModel> =
        Arc::new(MockLanguageModel::new().with_response(make_assistant_message("ok")));

    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .build(model)
        .await
        .expect("build should succeed");

    let guard = agent
        .bus()
        .get::<Mutex<SecurityGuard>>()
        .expect("default SecurityPlugin should publish SecurityGuard on the bus");
    // Just hold the lock briefly to make sure it's a real, usable handle.
    let _g = guard.lock().await;

    let snapshot = agent.assembly_snapshot();
    assert!(
        snapshot.plugin_names.iter().any(|name| name == "security"),
        "default SecurityPlugin should be visible in assembly snapshot: {:?}",
        snapshot.plugin_names
    );
}

// ---------------------------------------------------------------------------
// Test 4: a user-registered "security" plugin replaces the default
// ---------------------------------------------------------------------------

/// A custom "security" plugin that does NOT register a SecurityGuard on
/// the bus (it stores a marker instead). If the override mechanism works,
/// the bus will not have a SecurityGuard published — proving the default
/// SecurityPlugin was skipped.
struct CustomSecurityPlugin {
    activations: Arc<AtomicUsize>,
    marker: Arc<StdMutex<bool>>,
}

impl CustomSecurityPlugin {
    fn new(activations: Arc<AtomicUsize>, marker: Arc<StdMutex<bool>>) -> Self {
        Self {
            activations,
            marker,
        }
    }
}

#[async_trait]
impl Plugin for CustomSecurityPlugin {
    fn name(&self) -> &str {
        "security"
    }

    async fn register(&self, _r: &Registrar) {
        self.activations.fetch_add(1, Ordering::SeqCst);
        *self.marker.lock().unwrap() = true;
    }
}

#[tokio::test]
async fn custom_security_plugin_replaces_default() {
    let activations = Arc::new(AtomicUsize::new(0));
    let marker = Arc::new(StdMutex::new(false));
    let tmp = tempfile::tempdir().expect("tempdir");
    let model: Arc<dyn alva_kernel_abi::LanguageModel> =
        Arc::new(MockLanguageModel::new().with_response(make_assistant_message("ok")));

    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .plugin(Box::new(CustomSecurityPlugin::new(
            activations.clone(),
            marker.clone(),
        )))
        .build(model)
        .await
        .expect("build should succeed");

    assert_eq!(
        activations.load(Ordering::SeqCst),
        1,
        "custom SecurityPlugin's register() should have run exactly once"
    );
    assert!(
        *marker.lock().unwrap(),
        "custom SecurityPlugin marker should be set"
    );
    assert!(
        agent.bus().get::<Mutex<SecurityGuard>>().is_none(),
        "the default SecurityPlugin must NOT have been wired \
         when the user registered their own 'security' plugin"
    );

    let snapshot = agent.assembly_snapshot();
    assert_eq!(
        snapshot
            .plugin_names
            .iter()
            .filter(|name| name.as_str() == "security")
            .count(),
        1,
        "custom security plugin should replace, not duplicate, the default: {:?}",
        snapshot.plugin_names
    );
}
