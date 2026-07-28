// INPUT:  alva_engine_runtime::{EngineRuntime, RuntimeEvent, RuntimeRequest}, tokio streams, uuid, HubError
// OUTPUT: HubRuntime registry/dispatch, HubRequest, HubEvent, product-session identity and snapshots
// POS:    Maps durable Hub product sessions to transient engine sessions without importing any agent implementation.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use alva_engine_runtime::{EngineRuntime, PermissionDecision, RuntimeEvent, RuntimeRequest};
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};

use crate::HubError;

/// Stable identifier selected by the product for an engine registration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EngineId(String);

impl EngineId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for EngineId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for EngineId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl std::fmt::Display for EngineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Hub-owned session identity. It is intentionally distinct from an engine's
/// native session ID, so a product session may later hand off between engines.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProductSessionId(String);

impl ProductSessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProductSessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProductSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Lifecycle state tracked independently of the engine's own session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubSessionStatus {
    Starting,
    Running,
    Completed,
    Failed,
}

/// Immutable product-session state returned to product storage or UI layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubSessionSnapshot {
    pub product_session_id: ProductSessionId,
    pub engine_id: EngineId,
    pub engine_session_id: Option<String>,
    pub status: HubSessionStatus,
}

#[derive(Debug, Clone)]
struct HubSession {
    engine_id: EngineId,
    engine_session_id: Option<String>,
    status: HubSessionStatus,
}

impl HubSession {
    fn snapshot(&self, product_session_id: ProductSessionId) -> HubSessionSnapshot {
        HubSessionSnapshot {
            product_session_id,
            engine_id: self.engine_id.clone(),
            engine_session_id: self.engine_session_id.clone(),
            status: self.status,
        }
    }
}

/// A product request routed to one registered engine.
#[derive(Debug, Clone)]
pub struct HubRequest {
    pub engine_id: EngineId,
    pub product_session_id: Option<ProductSessionId>,
    pub request: RuntimeRequest,
}

impl HubRequest {
    /// Begin a new Hub product session using `engine_id`.
    pub fn new(engine_id: impl Into<EngineId>, request: RuntimeRequest) -> Self {
        Self {
            engine_id: engine_id.into(),
            product_session_id: None,
            request,
        }
    }

    /// Continue a Hub product session. Hub fills the engine's native resume ID
    /// from its session map instead of exposing it as product API.
    pub fn resume(mut self, product_session_id: ProductSessionId) -> Self {
        self.product_session_id = Some(product_session_id);
        self
    }
}

/// An engine event with its Hub routing identity attached.
#[derive(Debug, Clone)]
pub struct HubEvent {
    pub product_session_id: ProductSessionId,
    pub engine_id: EngineId,
    pub event: RuntimeEvent,
}

pub type HubEventStream = Pin<Box<dyn Stream<Item = HubEvent> + Send>>;

/// Registry and dispatcher for Alva Hub.
///
/// It owns product-session routing only. Agent loops, model APIs, and engine
/// implementation details remain behind `dyn EngineRuntime`.
#[derive(Clone, Default)]
pub struct HubRuntime {
    engines: Arc<RwLock<HashMap<EngineId, Arc<dyn EngineRuntime>>>>,
    sessions: Arc<RwLock<HashMap<ProductSessionId, HubSession>>>,
}

impl HubRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one product-visible engine.
    ///
    /// Engine replacement is fail-closed because existing product sessions
    /// carry native session IDs owned by the originally registered instance.
    pub fn register_engine(
        &self,
        id: impl Into<EngineId>,
        engine: Arc<dyn EngineRuntime>,
    ) -> Result<(), HubError> {
        let id = id.into();
        let mut engines = self
            .engines
            .write()
            .expect("Hub engine registry lock poisoned");
        if engines.contains_key(&id) {
            return Err(HubError::EngineAlreadyRegistered(id.to_string()));
        }
        engines.insert(id, engine);
        Ok(())
    }

    pub fn engine_ids(&self) -> Vec<EngineId> {
        let mut ids = self
            .engines
            .read()
            .expect("Hub engine registry lock poisoned")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        ids
    }

    pub fn session(&self, id: &ProductSessionId) -> Option<HubSessionSnapshot> {
        self.sessions
            .read()
            .expect("Hub session registry lock poisoned")
            .get(id)
            .map(|session| session.snapshot(id.clone()))
    }

    pub fn sessions(&self) -> Vec<HubSessionSnapshot> {
        let mut sessions = self
            .sessions
            .read()
            .expect("Hub session registry lock poisoned")
            .iter()
            .map(|(id, session)| session.snapshot(id.clone()))
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| {
            left.product_session_id
                .as_str()
                .cmp(right.product_session_id.as_str())
        });
        sessions
    }

    /// Start an engine and project its events onto a Hub-owned product session.
    pub fn execute(&self, request: HubRequest) -> Result<HubEventStream, HubError> {
        let engine = self.engine(&request.engine_id)?;
        let product_session_id = request.product_session_id.unwrap_or_default();
        let mut runtime_request = request.request;

        // Claim the product session under one write lock. Without this
        // transition a pair of concurrent resume calls can both observe
        // `Completed` and enter the same native session.
        let previous_status = {
            let mut sessions = self
                .sessions
                .write()
                .expect("Hub session registry lock poisoned");
            if let Some(existing) = sessions.get_mut(&product_session_id) {
                if existing.engine_id != request.engine_id {
                    return Err(HubError::EngineMismatch {
                        session_id: product_session_id.to_string(),
                        requested: request.engine_id.to_string(),
                        actual: existing.engine_id.to_string(),
                    });
                }
                if !engine.capabilities().resume {
                    return Err(HubError::ResumeUnsupported(request.engine_id.to_string()));
                }
                if matches!(
                    existing.status,
                    HubSessionStatus::Starting | HubSessionStatus::Running
                ) {
                    return Err(HubError::SessionBusy(product_session_id.to_string()));
                }
                runtime_request.resume_session =
                    Some(existing.engine_session_id.clone().ok_or_else(|| {
                        HubError::SessionNotReady(product_session_id.to_string())
                    })?);
                let status = existing.status;
                existing.status = HubSessionStatus::Starting;
                Some(status)
            } else {
                sessions.insert(
                    product_session_id.clone(),
                    HubSession {
                        engine_id: request.engine_id.clone(),
                        engine_session_id: None,
                        status: HubSessionStatus::Starting,
                    },
                );
                None
            }
        };

        let engine_stream = match engine.execute(runtime_request) {
            Ok(stream) => stream,
            Err(error) => {
                let mut sessions = self
                    .sessions
                    .write()
                    .expect("Hub session registry lock poisoned");
                if let Some(status) = previous_status {
                    if let Some(session) = sessions.get_mut(&product_session_id) {
                        session.status = status;
                    }
                } else {
                    sessions.remove(&product_session_id);
                }
                return Err(error.into());
            }
        };
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let sessions = self.sessions.clone();
        let engine_id = request.engine_id;
        let output_session_id = product_session_id.clone();

        tokio::spawn(async move {
            let mut engine_stream = engine_stream;
            let mut output_open = true;
            let mut saw_completed = false;
            while let Some(event) = engine_stream.next().await {
                saw_completed |= matches!(event, RuntimeEvent::Completed { .. });
                update_session(&sessions, &output_session_id, &event);
                if output_open
                    && event_tx
                        .send(HubEvent {
                            product_session_id: output_session_id.clone(),
                            engine_id: engine_id.clone(),
                            event,
                        })
                        .is_err()
                {
                    // Keep draining the engine stream even when the product
                    // consumer drops it; lifecycle projection must still
                    // reach Completed so the session does not remain busy.
                    output_open = false;
                }
            }
            if !saw_completed {
                let engine_session_id =
                    mark_session_failed(&sessions, &output_session_id).unwrap_or_default();
                if output_open {
                    let _ = event_tx.send(HubEvent {
                        product_session_id: output_session_id.clone(),
                        engine_id: engine_id.clone(),
                        event: RuntimeEvent::Error {
                            message: "engine stream ended without terminal Completed event".into(),
                            recoverable: false,
                        },
                    });
                    let _ = event_tx.send(HubEvent {
                        product_session_id: output_session_id,
                        engine_id,
                        event: RuntimeEvent::Completed {
                            session_id: engine_session_id,
                            result: None,
                            usage: None,
                        },
                    });
                }
            }
        });

        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    /// Route a product-level cancellation request to the active engine session.
    pub async fn cancel(&self, product_session_id: &ProductSessionId) -> Result<(), HubError> {
        let session = self
            .session(product_session_id)
            .ok_or_else(|| HubError::SessionNotFound(product_session_id.to_string()))?;
        let engine = self.engine(&session.engine_id)?;
        if !engine.capabilities().cancel {
            return Err(HubError::CancelUnsupported(session.engine_id.to_string()));
        }
        let engine_session_id = session
            .engine_session_id
            .ok_or_else(|| HubError::SessionNotReady(product_session_id.to_string()))?;
        engine.cancel(&engine_session_id).await?;
        Ok(())
    }

    /// Route a permission decision from Hub UI/CLI to the engine session that
    /// emitted the corresponding request.
    pub async fn respond_permission(
        &self,
        product_session_id: &ProductSessionId,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), HubError> {
        let session = self
            .session(product_session_id)
            .ok_or_else(|| HubError::SessionNotFound(product_session_id.to_string()))?;
        let engine = self.engine(&session.engine_id)?;
        if !engine.capabilities().permission_callback {
            return Err(HubError::PermissionUnsupported(
                session.engine_id.to_string(),
            ));
        }
        let engine_session_id = session
            .engine_session_id
            .ok_or_else(|| HubError::SessionNotReady(product_session_id.to_string()))?;
        engine
            .respond_permission(&engine_session_id, request_id, decision)
            .await?;
        Ok(())
    }

    fn engine(&self, id: &EngineId) -> Result<Arc<dyn EngineRuntime>, HubError> {
        self.engines
            .read()
            .expect("Hub engine registry lock poisoned")
            .get(id)
            .cloned()
            .ok_or_else(|| HubError::EngineNotFound(id.to_string()))
    }
}

fn update_session(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    event: &RuntimeEvent,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let Some(session) = sessions.get_mut(product_session_id) else {
        return;
    };
    match event {
        RuntimeEvent::SessionStarted { session_id, .. } => {
            session.engine_session_id = Some(session_id.clone());
            session.status = HubSessionStatus::Running;
        }
        RuntimeEvent::Completed { .. } => session.status = HubSessionStatus::Completed,
        _ => {}
    }
}

fn mark_session_failed(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
) -> Option<String> {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let session = sessions.get_mut(product_session_id)?;
    session.status = HubSessionStatus::Failed;
    session.engine_session_id.clone()
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use alva_engine_runtime::{
        EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
        RuntimeRequest,
    };
    use async_trait::async_trait;
    use futures::{stream, Stream};
    use tokio_stream::StreamExt;

    use super::*;

    struct PendingEngine;

    #[async_trait]
    impl EngineRuntime for PendingEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Ok(Box::pin(stream::pending()))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn capabilities(&self) -> RuntimeCapabilities {
            RuntimeCapabilities {
                streaming: true,
                tool_control: false,
                permission_callback: false,
                resume: true,
                cancel: true,
            }
        }
    }

    struct BrokenEngine {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl EngineRuntime for BrokenEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::pin(stream::iter(vec![RuntimeEvent::SessionStarted {
                session_id: "broken-engine".into(),
                model: None,
                tools: vec![],
            }])))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn capabilities(&self) -> RuntimeCapabilities {
            RuntimeCapabilities {
                streaming: true,
                tool_control: false,
                permission_callback: false,
                resume: true,
                cancel: true,
            }
        }
    }

    struct FakeEngine {
        requests: Mutex<Vec<RuntimeRequest>>,
        cancelled: Mutex<Vec<String>>,
    }

    impl FakeEngine {
        fn new() -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                cancelled: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl EngineRuntime for FakeEngine {
        fn execute(
            &self,
            request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.requests.lock().unwrap().push(request);
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "engine-42".into(),
                    model: None,
                    tools: vec![],
                },
                RuntimeEvent::Completed {
                    session_id: "engine-42".into(),
                    result: Some("done".into()),
                    usage: None,
                },
            ])))
        }

        async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
            self.cancelled.lock().unwrap().push(session_id.into());
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn capabilities(&self) -> RuntimeCapabilities {
            RuntimeCapabilities {
                streaming: true,
                tool_control: false,
                permission_callback: false,
                resume: true,
                cancel: true,
            }
        }
    }

    #[tokio::test]
    async fn executes_and_records_distinct_product_and_engine_session_ids() {
        let hub = HubRuntime::new();
        let engine = Arc::new(FakeEngine::new());
        hub.register_engine("alva", engine).unwrap();

        let events: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("hello")))
            .unwrap()
            .collect()
            .await;

        assert_eq!(events.len(), 2);
        let product_id = events[0].product_session_id.clone();
        assert_ne!(product_id.as_str(), "engine-42");
        let session = hub.session(&product_id).unwrap();
        assert_eq!(session.engine_session_id.as_deref(), Some("engine-42"));
        assert_eq!(session.status, HubSessionStatus::Completed);
    }

    #[tokio::test]
    async fn resume_uses_the_engine_session_id_not_the_product_session_id() {
        let hub = HubRuntime::new();
        let engine = Arc::new(FakeEngine::new());
        hub.register_engine("alva", engine.clone()).unwrap();
        let first: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("first")))
            .unwrap()
            .collect()
            .await;
        let product_id = first[0].product_session_id.clone();

        let _: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("second")).resume(product_id))
            .unwrap()
            .collect()
            .await;

        let requests = engine.requests.lock().unwrap();
        assert_eq!(requests[1].resume_session.as_deref(), Some("engine-42"));
    }

    #[tokio::test]
    async fn concurrent_product_session_execute_is_rejected_while_starting() {
        let hub = HubRuntime::new();
        hub.register_engine("pending", Arc::new(PendingEngine))
            .unwrap();
        let product_id = ProductSessionId::new();

        let _first = hub
            .execute(
                HubRequest::new("pending", RuntimeRequest::new("first")).resume(product_id.clone()),
            )
            .unwrap();
        let second = hub
            .execute(HubRequest::new("pending", RuntimeRequest::new("second")).resume(product_id));

        assert!(matches!(second, Err(HubError::SessionBusy(_))));
    }

    #[tokio::test]
    async fn missing_engine_terminal_is_synthesized_and_session_is_failed() {
        let hub = HubRuntime::new();
        hub.register_engine(
            "broken",
            Arc::new(BrokenEngine {
                calls: AtomicUsize::new(0),
            }),
        )
        .unwrap();

        let events: Vec<_> = hub
            .execute(HubRequest::new(
                "broken",
                RuntimeRequest::new("missing terminal"),
            ))
            .unwrap()
            .collect()
            .await;

        assert_eq!(events.len(), 3);
        assert!(matches!(
            events[1].event,
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        assert!(matches!(
            events[2].event,
            RuntimeEvent::Completed { result: None, .. }
        ));
        let session = hub.session(&events[0].product_session_id).unwrap();
        assert_eq!(session.status, HubSessionStatus::Failed);
    }

    #[tokio::test]
    async fn permission_response_checks_engine_capability() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        let events: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("hello")))
            .unwrap()
            .collect()
            .await;

        let error = hub
            .respond_permission(
                &events[0].product_session_id,
                "approval-1",
                PermissionDecision::Deny {
                    message: "no".into(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, HubError::PermissionUnsupported(_)));
    }

    #[test]
    fn duplicate_engine_registration_is_rejected() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();

        let error = hub
            .register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap_err();
        assert!(matches!(error, HubError::EngineAlreadyRegistered(_)));
    }
}
