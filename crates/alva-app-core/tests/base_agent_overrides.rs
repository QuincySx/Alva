//! Integration tests for `BaseAgentBuilder`'s override hooks.
//!
//! Verifies that:
//! 1. `.memory_service(custom)` actually wires the caller-supplied
//!    `MemoryService` into the resulting `BaseAgent`, instead of
//!    constructing the default `InMemoryBackend`-backed one.
//! 2. `.security_middleware(custom)` actually replaces the default
//!    `SecurityMiddleware` in the middleware stack — proven by the
//!    custom middleware's `on_agent_start` hook firing during a prompt.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;

use alva_agent_memory::{InMemoryBackend, MemoryService, NoopEmbeddingProvider};
use alva_app_core::base_agent::BaseAgent;
use alva_app_core::AgentEvent;
use alva_kernel_core::middleware::Middleware;
use alva_kernel_core::shared::MiddlewareError;
use alva_kernel_core::state::AgentState;
use alva_test::fixtures::make_assistant_message;
use alva_test::mock_provider::MockLanguageModel;

// ---------------------------------------------------------------------------
// Test 1: memory_service override
// ---------------------------------------------------------------------------

#[tokio::test]
async fn memory_service_override_is_used() {
    // Build a custom MemoryService backed by the pure in-memory backend, and
    // seed it with a known sentinel entry.
    let backend = InMemoryBackend::new();
    let embedder = Box::new(NoopEmbeddingProvider::new());
    let custom = MemoryService::with_backend(Arc::new(backend), embedder);

    custom
        .store_entry(
            "override-sentinel-key",
            "sentinel content from custom memory service",
            "test",
        )
        .await
        .expect("seed sentinel entry");

    // Build the agent with the custom MemoryService injected.
    let tmp = tempfile::tempdir().expect("tempdir");
    let model: Arc<dyn alva_kernel_abi::LanguageModel> =
        Arc::new(MockLanguageModel::new().with_response(make_assistant_message("ok")));

    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .memory_service(custom)
        .build(model)
        .await
        .expect("build should succeed");

    // The agent should expose *our* MemoryService — confirm by querying for
    // the sentinel content. A freshly-constructed default InMemoryBackend would
    // have no entries, so a non-empty result proves we got the override.
    let memory = agent.memory().expect("memory should be wired");
    let results = memory
        .search("sentinel", 10)
        .await
        .expect("search should succeed");

    assert!(
        !results.is_empty(),
        "expected the seeded sentinel entry, got empty results — \
         this means the override was ignored and a fresh service was built"
    );
    assert!(
        results
            .iter()
            .any(|e| e.path == "override-sentinel-key"),
        "expected to find the sentinel entry by path; results: {:?}",
        results.iter().map(|e| &e.path).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Test 2: security_middleware override
// ---------------------------------------------------------------------------

/// A minimal `Middleware` that counts how often `on_agent_start` is invoked.
/// Used as a stand-in for a custom security middleware so we can assert the
/// builder actually wired it into the stack.
#[derive(Default)]
struct CountingMiddleware {
    starts: Arc<AtomicUsize>,
}

#[async_trait]
impl Middleware for CountingMiddleware {
    async fn on_agent_start(&self, _state: &mut AgentState) -> Result<(), MiddlewareError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn security_middleware_override_is_used() {
    let starts = Arc::new(AtomicUsize::new(0));
    let custom_mw = Arc::new(CountingMiddleware {
        starts: starts.clone(),
    });

    let tmp = tempfile::tempdir().expect("tempdir");
    let model: Arc<dyn alva_kernel_abi::LanguageModel> = Arc::new(
        MockLanguageModel::new().with_response(make_assistant_message("hello from mock")),
    );

    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .security_middleware(custom_mw.clone())
        .build(model)
        .await
        .expect("build should succeed");

    // Drive a single prompt through the agent loop. This should trigger
    // `on_agent_start` on every middleware in the stack — including ours.
    let mut rx = agent.prompt_text("hi");
    while let Some(event) = rx.recv().await {
        if matches!(event, AgentEvent::AgentEnd { .. }) {
            break;
        }
    }

    let count = starts.load(Ordering::SeqCst);
    assert!(
        count >= 1,
        "expected the custom security middleware to be called at least once \
         (on_agent_start), got {count}. This means the override was not wired \
         into the middleware stack."
    );
}
