// INPUT:  alva_engine_runtime::{EngineRuntime, RuntimeEvent, RuntimeRequest}, tokio streams, uuid, HubError
// OUTPUT: HubRuntime registry/dispatch, HubRequest, HubEvent, product-session identity and snapshots
// POS:    Maps durable Hub product sessions to transient engine sessions without importing any agent implementation.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeEvent, RuntimeRequest, StreamContract,
};
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};
use tracing::{debug, warn};

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

    /// Reconstruct a product session id from a previously printed token.
    ///
    /// Hub does not validate UUID shape here — the registry is the authority
    /// for whether the id still exists in this process.
    pub fn from_existing(id: impl Into<String>) -> Self {
        Self(id.into())
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
    /// Engine this product session used before the latest handoff, if any.
    pub previous_engine_id: Option<EngineId>,
    /// Native session id of the previous engine (for audit / optional context).
    pub previous_engine_session_id: Option<String>,
    /// True until the first execute after [`HubRuntime::switch_engine`].
    pub pending_handoff: bool,
    /// Hard stream-contract violations observed on the **latest** execute stream.
    ///
    /// Hub still forwards violating events (adapters stay debuggable); this
    /// counter is the product-visible signal for inspector / CLI / tests.
    pub stream_hard_violations: u32,
    /// Soft hygiene warnings (e.g. unclosed tools) from the latest finish check.
    pub stream_soft_warnings: u32,
    /// Last hard violation description (human-readable, for debug UIs).
    pub last_stream_violation: Option<String>,
}

#[derive(Debug, Clone)]
struct HubSession {
    engine_id: EngineId,
    engine_session_id: Option<String>,
    previous_engine_id: Option<EngineId>,
    previous_engine_session_id: Option<String>,
    /// Cleared on the first execute after a handoff.
    pending_handoff: bool,
    /// Product-supplied handoff context retained when engine startup or the
    /// first post-switch stream fails, so a retry can reuse the same preamble.
    pending_handoff_context: Option<String>,
    status: HubSessionStatus,
    stream_hard_violations: u32,
    stream_soft_warnings: u32,
    last_stream_violation: Option<String>,
}

impl HubSession {
    fn new(engine_id: EngineId) -> Self {
        Self {
            engine_id,
            engine_session_id: None,
            previous_engine_id: None,
            previous_engine_session_id: None,
            pending_handoff: false,
            pending_handoff_context: None,
            status: HubSessionStatus::Starting,
            stream_hard_violations: 0,
            stream_soft_warnings: 0,
            last_stream_violation: None,
        }
    }

    fn reset_stream_diagnostics(&mut self) {
        self.stream_hard_violations = 0;
        self.stream_soft_warnings = 0;
        self.last_stream_violation = None;
    }

    fn snapshot(&self, product_session_id: ProductSessionId) -> HubSessionSnapshot {
        HubSessionSnapshot {
            product_session_id,
            engine_id: self.engine_id.clone(),
            engine_session_id: self.engine_session_id.clone(),
            status: self.status,
            previous_engine_id: self.previous_engine_id.clone(),
            previous_engine_session_id: self.previous_engine_session_id.clone(),
            pending_handoff: self.pending_handoff,
            stream_hard_violations: self.stream_hard_violations,
            stream_soft_warnings: self.stream_soft_warnings,
            last_stream_violation: self.last_stream_violation.clone(),
        }
    }
}

/// A product request routed to one registered engine.
#[derive(Debug, Clone)]
pub struct HubRequest {
    pub engine_id: EngineId,
    pub product_session_id: Option<ProductSessionId>,
    pub request: RuntimeRequest,
    /// Optional preamble injected once on the first turn after
    /// [`HubRuntime::switch_engine`] (when `pending_handoff` is set).
    /// Products decide which policy-approved context, if any, may cross engines.
    pub handoff_context: Option<String>,
}

impl HubRequest {
    /// Begin a new Hub product session using `engine_id`.
    pub fn new(engine_id: impl Into<EngineId>, request: RuntimeRequest) -> Self {
        Self {
            engine_id: engine_id.into(),
            product_session_id: None,
            request,
            handoff_context: None,
        }
    }

    /// Continue a Hub product session. Hub fills the engine's native resume ID
    /// from its session map instead of exposing it as product API.
    pub fn resume(mut self, product_session_id: ProductSessionId) -> Self {
        self.product_session_id = Some(product_session_id);
        self
    }

    /// Attach a one-shot handoff preamble for the next post-switch execute.
    pub fn with_handoff_context(mut self, context: impl Into<String>) -> Self {
        self.handoff_context = Some(context.into());
        self
    }
}

/// Message Hub puts on the terminal pair it synthesizes when an engine stream
/// ends without `Completed`.
///
/// Deliberately worded to avoid every phrase in the product's transport
/// keyword table: this is an adapter contract violation, not an operational
/// continuity failure, and must not spend another engine's account on a
/// fallback attempt. `alva-hub-desktop` pins that property against this
/// constant, so changing the wording will fail that test rather than silently
/// re-enable paid fallback.
pub const SYNTHETIC_TERMINAL_MESSAGE: &str =
    "engine produced no terminal event (adapter contract violation)";

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

    /// Seed a product session so the next [`Self::execute`] can resume it.
    ///
    /// Used by products that rehydrate from disk after process restart.
    /// The engine must still understand `engine_session_id` (e.g. Claude
    /// `--resume`); in-memory engines like Alva only work if that native
    /// session still lives in the same adapter process.
    ///
    /// Fails if the product id is already registered to a different engine or
    /// a different engine session id.
    pub fn restore_session(
        &self,
        product_session_id: ProductSessionId,
        engine_id: impl Into<EngineId>,
        engine_session_id: impl Into<String>,
        status: HubSessionStatus,
    ) -> Result<(), HubError> {
        let engine_id = engine_id.into();
        let engine_session_id = engine_session_id.into();
        if engine_session_id.trim().is_empty() {
            return Err(HubError::SessionNotReady(product_session_id.to_string()));
        }
        // Engine must be registered so resume routing is possible.
        let _ = self.engine(&engine_id)?;

        let mut sessions = self
            .sessions
            .write()
            .expect("Hub session registry lock poisoned");
        if let Some(existing) = sessions.get(&product_session_id) {
            if existing.engine_id != engine_id {
                return Err(HubError::EngineMismatch {
                    session_id: product_session_id.to_string(),
                    requested: engine_id.to_string(),
                    actual: existing.engine_id.to_string(),
                });
            }
            if let Some(prev) = &existing.engine_session_id {
                if prev != &engine_session_id {
                    return Err(HubError::SessionConflict {
                        session_id: product_session_id.to_string(),
                        detail: format!(
                            "engine session mismatch (have {prev}, restore {engine_session_id})"
                        ),
                    });
                }
            }
            // Idempotent restore of the same mapping.
            let entry = sessions.get_mut(&product_session_id).expect("just checked");
            entry.engine_session_id = Some(engine_session_id);
            if !matches!(
                entry.status,
                HubSessionStatus::Starting | HubSessionStatus::Running
            ) {
                entry.status = status;
            }
            return Ok(());
        }

        let mut session = HubSession::new(engine_id);
        session.engine_session_id = Some(engine_session_id);
        session.status = status;
        sessions.insert(product_session_id, session);
        Ok(())
    }

    /// Hand off a product session to another registered engine.
    ///
    /// Keeps the same [`ProductSessionId`]. The previous engine mapping is
    /// stored on the snapshot for audit; the next [`Self::execute`] starts a
    /// **fresh** native session on `new_engine_id` (no native resume on the
    /// new engine). Context is not automatically migrated — products may
    /// inject a summary into the next prompt if desired.
    ///
    /// No-op (success) when the session already targets `new_engine_id`.
    pub fn switch_engine(
        &self,
        product_session_id: &ProductSessionId,
        new_engine_id: impl Into<EngineId>,
    ) -> Result<HubSessionSnapshot, HubError> {
        let new_engine_id = new_engine_id.into();
        let _ = self.engine(&new_engine_id)?;

        let mut sessions = self
            .sessions
            .write()
            .expect("Hub session registry lock poisoned");
        let session = sessions
            .get_mut(product_session_id)
            .ok_or_else(|| HubError::SessionNotFound(product_session_id.to_string()))?;

        if matches!(
            session.status,
            HubSessionStatus::Starting | HubSessionStatus::Running
        ) {
            return Err(HubError::SessionBusy(product_session_id.to_string()));
        }

        if session.engine_id == new_engine_id {
            return Ok(session.snapshot(product_session_id.clone()));
        }

        session.previous_engine_id = Some(session.engine_id.clone());
        session.previous_engine_session_id = session.engine_session_id.clone();
        session.engine_id = new_engine_id;
        session.engine_session_id = None;
        session.pending_handoff = true;
        session.pending_handoff_context = None;
        // Ready for a new native session on the target engine.
        session.status = HubSessionStatus::Completed;
        session.reset_stream_diagnostics();
        Ok(session.snapshot(product_session_id.clone()))
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
        let handoff_context = request.handoff_context;

        // Claim the product session under one write lock. Without this
        // transition a pair of concurrent resume calls can both observe
        // `Completed` and enter the same native session.
        let (previous_session, handoff_context_used) = {
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
                if matches!(
                    existing.status,
                    HubSessionStatus::Starting | HubSessionStatus::Running
                ) {
                    return Err(HubError::SessionBusy(product_session_id.to_string()));
                }
                // Same product session, same engine:
                // - with native id → resume when the engine supports it
                // - without native id → handoff / first turn after switch_engine
                match &existing.engine_session_id {
                    Some(engine_sid) => {
                        if !engine.capabilities().resume {
                            return Err(HubError::ResumeUnsupported(request.engine_id.to_string()));
                        }
                        runtime_request.resume_session = Some(engine_sid.clone());
                    }
                    None => {
                        runtime_request.resume_session = None;
                    }
                }

                let rollback = existing.clone();
                let mut handoff_context_used = None;
                // One-shot handoff preamble for the first post-switch turn.
                if existing.pending_handoff {
                    let prev = existing
                        .previous_engine_id
                        .as_ref()
                        .map(|e| e.as_str())
                        .unwrap_or("unknown");
                    let banner = handoff_context
                        .or_else(|| existing.pending_handoff_context.take())
                        .unwrap_or_else(|| {
                            default_handoff_banner(prev, request.engine_id.as_str())
                        });
                    runtime_request.prompt =
                        format!("{banner}\n\n## User message\n\n{}", runtime_request.prompt);
                    handoff_context_used = Some(banner);
                    existing.pending_handoff = false;
                }

                existing.status = HubSessionStatus::Starting;
                // New execute turn: diagnostics describe the stream about to run.
                existing.reset_stream_diagnostics();
                (Some(rollback), handoff_context_used)
            } else {
                sessions.insert(
                    product_session_id.clone(),
                    HubSession::new(request.engine_id.clone()),
                );
                (None, None)
            }
        };

        let engine_stream = match engine.execute(runtime_request) {
            Ok(stream) => stream,
            Err(error) => {
                let mut sessions = self
                    .sessions
                    .write()
                    .expect("Hub session registry lock poisoned");
                if let Some(mut previous) = previous_session {
                    previous.pending_handoff_context = handoff_context_used;
                    sessions.insert(product_session_id.clone(), previous);
                } else {
                    sessions.remove(&product_session_id);
                }
                return Err(error.into());
            }
        };
        let failed_handoff_rollback = previous_session.zip(handoff_context_used);
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let sessions = self.sessions.clone();
        let engine_id = request.engine_id;
        let output_session_id = product_session_id.clone();

        tokio::spawn(async move {
            let mut engine_stream = engine_stream;
            let mut output_open = true;
            let mut saw_completed = false;
            let mut saw_fatal_error = false;
            // Multi-engine stream contract (cindy lessons): observe every
            // engine event. Hard violations are logged; Hub still forwards
            // events so adapters stay debuggable, and missing Completed is
            // still synthesized below.
            let mut contract = StreamContract::new();
            let mut hard_violations = 0u32;
            while let Some(event) = engine_stream.next().await {
                if let Err(err) = contract.observe(&event) {
                    hard_violations = hard_violations.saturating_add(1);
                    record_hard_violation(&sessions, &output_session_id, &err.to_string());
                    warn!(
                        product_session = %output_session_id,
                        engine = %engine_id,
                        %err,
                        "engine stream violated multi-engine stream contract"
                    );
                    // debug builds also surface on stderr so hub CLI / tests
                    // catch adapter regressions without a tracing subscriber.
                    if cfg!(debug_assertions) {
                        eprintln!(
                            "alva-hub-core: stream contract violation (session={output_session_id}, engine={engine_id}): {err}"
                        );
                    }
                }
                saw_completed |= matches!(&event, RuntimeEvent::Completed { .. });
                saw_fatal_error |= matches!(
                    &event,
                    RuntimeEvent::Error {
                        recoverable: false,
                        ..
                    }
                );
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
            match contract.finish() {
                Ok(()) => {
                    let warnings = contract.soft_warnings();
                    let soft_count = warnings.len() as u32;
                    if soft_count > 0 {
                        record_soft_warnings(&sessions, &output_session_id, soft_count);
                    }
                    for warning in warnings {
                        debug!(
                            product_session = %output_session_id,
                            engine = %engine_id,
                            %warning,
                            "engine stream soft contract warning"
                        );
                    }
                }
                Err(err) => {
                    // MissingCompleted is expected when adapters die mid-stream;
                    // synthesis below repairs the product edge.
                    if hard_violations == 0 {
                        hard_violations = 1;
                        record_hard_violation(&sessions, &output_session_id, &err.to_string());
                    }
                    warn!(
                        product_session = %output_session_id,
                        engine = %engine_id,
                        %err,
                        "engine stream failed stream-contract finish check"
                    );
                }
            }
            if hard_violations > 0 {
                debug!(
                    product_session = %output_session_id,
                    engine = %engine_id,
                    hard_violations,
                    "engine stream finished with contract hard violations"
                );
            }
            if !saw_completed {
                saw_fatal_error = true;
                let engine_session_id =
                    mark_session_failed(&sessions, &output_session_id).unwrap_or_else(|| {
                        // No native session id exists when the engine never sent
                        // SessionStarted. The empty string is the honest value —
                        // downstream guards on `!is_empty()` — but it must not be
                        // silent, or a missing id looks like a downstream bug.
                        tracing::warn!(
                            product_session_id = %output_session_id,
                            "synthesizing terminal pair with no native engine session id"
                        );
                        String::new()
                    });
                if output_open {
                    let _ = event_tx.send(HubEvent {
                        product_session_id: output_session_id.clone(),
                        engine_id: engine_id.clone(),
                        event: RuntimeEvent::Error {
                            message: SYNTHETIC_TERMINAL_MESSAGE.into(),
                            recoverable: false,
                        },
                    });
                    let _ = event_tx.send(HubEvent {
                        product_session_id: output_session_id.clone(),
                        engine_id,
                        event: RuntimeEvent::Completed {
                            session_id: engine_session_id,
                            result: None,
                            usage: None,
                        },
                    });
                }
            }
            if saw_fatal_error {
                if let Some((previous, context)) = failed_handoff_rollback {
                    restore_failed_handoff(&sessions, &output_session_id, previous, context);
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
            if session.status != HubSessionStatus::Failed {
                session.status = HubSessionStatus::Running;
            }
        }
        RuntimeEvent::Error {
            recoverable: false, ..
        } => session.status = HubSessionStatus::Failed,
        RuntimeEvent::Completed { .. } if session.status != HubSessionStatus::Failed => {
            session.status = HubSessionStatus::Completed;
        }
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

fn record_hard_violation(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    detail: &str,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let Some(session) = sessions.get_mut(product_session_id) else {
        return;
    };
    session.stream_hard_violations = session.stream_hard_violations.saturating_add(1);
    session.last_stream_violation = Some(detail.to_string());
}

fn record_soft_warnings(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    count: u32,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let Some(session) = sessions.get_mut(product_session_id) else {
        return;
    };
    session.stream_soft_warnings = session.stream_soft_warnings.saturating_add(count);
}

fn restore_failed_handoff(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    mut previous: HubSession,
    context: String,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    if let Some(failed) = sessions.get(product_session_id) {
        previous.stream_hard_violations = failed.stream_hard_violations;
        previous.stream_soft_warnings = failed.stream_soft_warnings;
        previous.last_stream_violation = failed.last_stream_violation.clone();
    }
    previous.pending_handoff_context = Some(context);
    sessions.insert(product_session_id.clone(), previous);
}

/// Minimal handoff banner when the product does not supply richer context.
fn default_handoff_banner(from_engine: &str, to_engine: &str) -> String {
    format!(
        "\
## Alva Hub engine handoff

You are continuing **product-session** work after a handoff from engine \
`{from_engine}` to engine `{to_engine}`.

The previous engine's full conversation is **not** automatically available \
here. Use only the handoff metadata and the new user message. \
If critical context is missing, say what you need."
    )
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

    fn session_registry() -> (
        ProductSessionId,
        RwLock<HashMap<ProductSessionId, HubSession>>,
    ) {
        let product_session_id = ProductSessionId::new();
        let sessions = RwLock::new(HashMap::from([(
            product_session_id.clone(),
            HubSession::new(EngineId::from("test")),
        )]));
        (product_session_id, sessions)
    }

    #[test]
    fn nonrecoverable_error_keeps_session_failed_after_completed() {
        let (product_session_id, sessions) = session_registry();
        update_session(
            &sessions,
            &product_session_id,
            &RuntimeEvent::Error {
                message: "engine failed".into(),
                recoverable: false,
            },
        );
        update_session(
            &sessions,
            &product_session_id,
            &RuntimeEvent::Completed {
                session_id: String::new(),
                result: None,
                usage: None,
            },
        );

        assert_eq!(
            sessions.read().unwrap()[&product_session_id].status,
            HubSessionStatus::Failed
        );
    }

    #[test]
    fn recoverable_error_allows_normal_completion() {
        let (product_session_id, sessions) = session_registry();
        update_session(
            &sessions,
            &product_session_id,
            &RuntimeEvent::Error {
                message: "temporary failure".into(),
                recoverable: true,
            },
        );
        update_session(
            &sessions,
            &product_session_id,
            &RuntimeEvent::SessionStarted {
                session_id: "native-session".into(),
                model: None,
                tools: vec![],
            },
        );
        update_session(
            &sessions,
            &product_session_id,
            &RuntimeEvent::Completed {
                session_id: "native-session".into(),
                result: Some("done".into()),
                usage: None,
            },
        );

        let session = &sessions.read().unwrap()[&product_session_id];
        assert_eq!(session.status, HubSessionStatus::Completed);
        assert_eq!(session.engine_session_id.as_deref(), Some("native-session"));
    }

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

    struct ErrorBeforeStartEngine;

    #[async_trait]
    impl EngineRuntime for ErrorBeforeStartEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::Error {
                    message: "bridge spawn failed".into(),
                    recoverable: false,
                },
                RuntimeEvent::Completed {
                    session_id: String::new(),
                    result: None,
                    usage: None,
                },
            ])))
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

    struct FailOnceEngine {
        calls: AtomicUsize,
        requests: Mutex<Vec<RuntimeRequest>>,
    }

    struct FatalStreamOnceEngine {
        calls: AtomicUsize,
        requests: Mutex<Vec<RuntimeRequest>>,
    }

    #[async_trait]
    impl EngineRuntime for FatalStreamOnceEngine {
        fn execute(
            &self,
            request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.requests.lock().unwrap().push(request);
            let events = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                vec![
                    RuntimeEvent::Error {
                        message: "target failed after startup".into(),
                        recoverable: false,
                    },
                    RuntimeEvent::Completed {
                        session_id: String::new(),
                        result: None,
                        usage: None,
                    },
                ]
            } else {
                vec![
                    RuntimeEvent::SessionStarted {
                        session_id: "stream-retry-engine".into(),
                        model: None,
                        tools: vec![],
                    },
                    RuntimeEvent::Completed {
                        session_id: "stream-retry-engine".into(),
                        result: Some("done".into()),
                        usage: None,
                    },
                ]
            };
            Ok(Box::pin(stream::iter(events)))
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

    #[async_trait]
    impl EngineRuntime for FailOnceEngine {
        fn execute(
            &self,
            request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.requests.lock().unwrap().push(request);
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(RuntimeError::ProcessError("transient spawn failure".into()));
            }
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "retry-engine".into(),
                    model: None,
                    tools: vec![],
                },
                RuntimeEvent::Completed {
                    session_id: "retry-engine".into(),
                    result: Some("done".into()),
                    usage: None,
                },
            ])))
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
    async fn restore_session_enables_resume_after_rehydrate() {
        let hub = HubRuntime::new();
        let engine = Arc::new(FakeEngine::new());
        hub.register_engine("claude", engine.clone()).unwrap();

        let product_id = ProductSessionId::from_existing("prod-restored");
        hub.restore_session(
            product_id.clone(),
            "claude",
            "engine-from-disk",
            HubSessionStatus::Completed,
        )
        .unwrap();

        let snap = hub.session(&product_id).unwrap();
        assert_eq!(snap.engine_id.as_str(), "claude");
        assert_eq!(snap.engine_session_id.as_deref(), Some("engine-from-disk"));
        assert_eq!(snap.status, HubSessionStatus::Completed);

        let _: Vec<_> = hub
            .execute(HubRequest::new("claude", RuntimeRequest::new("again")).resume(product_id))
            .unwrap()
            .collect()
            .await;

        let requests = engine.requests.lock().unwrap();
        assert_eq!(
            requests[0].resume_session.as_deref(),
            Some("engine-from-disk")
        );
    }

    #[tokio::test]
    async fn restore_session_rejects_engine_mismatch() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        hub.register_engine("claude", Arc::new(FakeEngine::new()))
            .unwrap();
        let product_id = ProductSessionId::from_existing("prod-x");
        hub.restore_session(
            product_id.clone(),
            "alva",
            "e1",
            HubSessionStatus::Completed,
        )
        .unwrap();
        let err = hub
            .restore_session(product_id, "claude", "e2", HubSessionStatus::Completed)
            .unwrap_err();
        assert!(matches!(err, HubError::EngineMismatch { .. }));
    }

    #[tokio::test]
    async fn switch_engine_starts_fresh_native_session_on_target() {
        let hub = HubRuntime::new();
        let alva = Arc::new(FakeEngine::new());
        let claude = Arc::new(FakeEngine::new());
        hub.register_engine("alva", alva.clone()).unwrap();
        hub.register_engine("claude", claude.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("on alva")))
            .unwrap()
            .collect()
            .await;
        let product_id = first[0].product_session_id.clone();
        let snap = hub.session(&product_id).unwrap();
        assert_eq!(snap.engine_id.as_str(), "alva");
        assert!(snap.engine_session_id.is_some());

        let switched = hub.switch_engine(&product_id, "claude").unwrap();
        assert_eq!(switched.engine_id.as_str(), "claude");
        assert!(switched.engine_session_id.is_none());
        assert_eq!(
            switched.previous_engine_id.as_ref().map(|e| e.as_str()),
            Some("alva")
        );
        assert_eq!(
            switched.previous_engine_session_id.as_deref(),
            Some("engine-42")
        );

        let _: Vec<_> = hub
            .execute(HubRequest::new("claude", RuntimeRequest::new("on claude")).resume(product_id))
            .unwrap()
            .collect()
            .await;

        // Fresh native session on claude — no resume_session filled; handoff
        // banner is prepended to the user prompt.
        {
            let claude_reqs = claude.requests.lock().unwrap();
            assert_eq!(claude_reqs.len(), 1);
            assert!(claude_reqs[0].resume_session.is_none());
            assert!(
                claude_reqs[0].prompt.contains("engine handoff")
                    && claude_reqs[0].prompt.contains("on claude"),
                "expected handoff banner + user prompt, got: {}",
                claude_reqs[0].prompt
            );
        }
        // Second turn on same engine should not re-inject the banner.
        // Drop the requests lock before execute — FakeEngine locks the same mutex.
        let product_id = first[0].product_session_id.clone();
        let _: Vec<_> = hub
            .execute(
                HubRequest::new("claude", RuntimeRequest::new("again"))
                    .resume(product_id)
                    .with_handoff_context("SHOULD_NOT_APPEAR"),
            )
            .unwrap()
            .collect()
            .await;
        let claude_reqs = claude.requests.lock().unwrap();
        assert_eq!(claude_reqs.len(), 2);
        assert!(!claude_reqs[1].prompt.contains("SHOULD_NOT_APPEAR"));
        assert!(!claude_reqs[1].prompt.contains("engine handoff"));
    }

    #[tokio::test]
    async fn handoff_uses_product_supplied_context_once() {
        let hub = HubRuntime::new();
        let alva = Arc::new(FakeEngine::new());
        let claude = Arc::new(FakeEngine::new());
        hub.register_engine("alva", alva).unwrap();
        hub.register_engine("claude", claude.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("start")))
            .unwrap()
            .collect()
            .await;
        let product_id = first[0].product_session_id.clone();
        hub.switch_engine(&product_id, "claude").unwrap();
        let _: Vec<_> = hub
            .execute(
                HubRequest::new("claude", RuntimeRequest::new("go"))
                    .resume(product_id)
                    .with_handoff_context("CUSTOM_HANDOFF_BLOCK"),
            )
            .unwrap()
            .collect()
            .await;
        let reqs = claude.requests.lock().unwrap();
        assert!(reqs[0].prompt.contains("CUSTOM_HANDOFF_BLOCK"));
        assert!(reqs[0].prompt.contains("## User message"));
        assert!(reqs[0].prompt.contains("go"));
    }

    #[tokio::test]
    async fn failed_engine_start_preserves_handoff_context_for_retry() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        let retry_engine = Arc::new(FailOnceEngine {
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        });
        hub.register_engine("retry", retry_engine.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("start")))
            .unwrap()
            .collect()
            .await;
        let product_id = first[0].product_session_id.clone();
        hub.switch_engine(&product_id, "retry").unwrap();

        let error = match hub.execute(
            HubRequest::new("retry", RuntimeRequest::new("first attempt"))
                .resume(product_id.clone())
                .with_handoff_context("CUSTOM_HANDOFF_BLOCK"),
        ) {
            Err(error) => error,
            Ok(_) => panic!("first engine start should fail"),
        };
        assert!(matches!(
            error,
            HubError::Runtime(RuntimeError::ProcessError(_))
        ));
        let after_error = hub.session(&product_id).unwrap();
        assert_eq!(after_error.status, HubSessionStatus::Completed);
        assert!(after_error.pending_handoff);

        let _: Vec<_> = hub
            .execute(
                HubRequest::new("retry", RuntimeRequest::new("retry attempt"))
                    .resume(product_id.clone()),
            )
            .unwrap()
            .collect()
            .await;

        let requests = retry_engine.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].prompt.contains("CUSTOM_HANDOFF_BLOCK"));
        assert!(requests[1].prompt.contains("retry attempt"));
        assert!(!hub.session(&product_id).unwrap().pending_handoff);
    }

    #[tokio::test]
    async fn fatal_first_handoff_stream_preserves_context_for_retry() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        let retry_engine = Arc::new(FatalStreamOnceEngine {
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        });
        hub.register_engine("retry", retry_engine.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("start")))
            .unwrap()
            .collect()
            .await;
        let product_id = first[0].product_session_id.clone();
        hub.switch_engine(&product_id, "retry").unwrap();

        let failed: Vec<_> = hub
            .execute(
                HubRequest::new("retry", RuntimeRequest::new("first attempt"))
                    .resume(product_id.clone())
                    .with_handoff_context("SAFE_HANDOFF_BLOCK"),
            )
            .unwrap()
            .collect()
            .await;
        assert!(matches!(
            failed[0].event,
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        let after_failure = hub.session(&product_id).unwrap();
        assert!(after_failure.pending_handoff);
        assert_eq!(after_failure.engine_id.as_str(), "retry");
        assert!(after_failure.engine_session_id.is_none());

        let _: Vec<_> = hub
            .execute(
                HubRequest::new("retry", RuntimeRequest::new("second attempt"))
                    .resume(product_id.clone()),
            )
            .unwrap()
            .collect()
            .await;
        let requests = retry_engine.requests.lock().unwrap();
        assert!(requests[0].prompt.contains("SAFE_HANDOFF_BLOCK"));
        assert!(requests[1].prompt.contains("SAFE_HANDOFF_BLOCK"));
        assert!(!hub.session(&product_id).unwrap().pending_handoff);
    }

    #[tokio::test]
    async fn switch_engine_same_target_is_noop() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        let product_id = ProductSessionId::from_existing("p-same");
        hub.restore_session(
            product_id.clone(),
            "alva",
            "e1",
            HubSessionStatus::Completed,
        )
        .unwrap();
        let snap = hub.switch_engine(&product_id, "alva").unwrap();
        assert_eq!(snap.engine_session_id.as_deref(), Some("e1"));
        assert!(snap.previous_engine_id.is_none());
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
    async fn error_before_session_started_then_completed_stays_failed() {
        let hub = HubRuntime::new();
        hub.register_engine("spawn-error", Arc::new(ErrorBeforeStartEngine))
            .unwrap();

        let events: Vec<_> = hub
            .execute(HubRequest::new("spawn-error", RuntimeRequest::new("hello")))
            .unwrap()
            .collect()
            .await;

        assert!(matches!(
            events.as_slice(),
            [
                HubEvent {
                    event: RuntimeEvent::Error {
                        recoverable: false,
                        ..
                    },
                    ..
                },
                HubEvent {
                    event: RuntimeEvent::Completed { .. },
                    ..
                },
            ]
        ));
        let session = hub.session(&events[0].product_session_id).unwrap();
        assert_eq!(session.status, HubSessionStatus::Failed);
        assert!(session.engine_session_id.is_none());
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

    /// Emits a post-Completed event — hard stream-contract violation.
    /// Hub must still forward events (observe + warn, not drop).
    struct ContractBreakingEngine;

    #[async_trait]
    impl EngineRuntime for ContractBreakingEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "break-1".into(),
                    model: None,
                    tools: vec![],
                },
                RuntimeEvent::Completed {
                    session_id: "break-1".into(),
                    result: Some("ok".into()),
                    usage: None,
                },
                RuntimeEvent::Error {
                    message: "late event after completed".into(),
                    recoverable: true,
                },
            ])))
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
                streaming: false,
                tool_control: false,
                permission_callback: false,
                resume: false,
                cancel: false,
            }
        }
    }

    #[tokio::test]
    async fn stream_contract_violations_are_forwarded_not_dropped() {
        let hub = HubRuntime::new();
        hub.register_engine("broken-contract", Arc::new(ContractBreakingEngine))
            .unwrap();

        let events: Vec<_> = hub
            .execute(HubRequest::new(
                "broken-contract",
                RuntimeRequest::new("probe"),
            ))
            .unwrap()
            .collect()
            .await;

        assert_eq!(
            events.len(),
            3,
            "Hub observes contract but does not drop events"
        );
        assert!(matches!(events[1].event, RuntimeEvent::Completed { .. }));
        assert!(matches!(
            events[2].event,
            RuntimeEvent::Error {
                recoverable: true,
                ..
            }
        ));
        // Product session still completed from the Completed event (before late error).
        let session = hub.session(&events[0].product_session_id).unwrap();
        assert_eq!(session.status, HubSessionStatus::Completed);
        assert!(
            session.stream_hard_violations >= 1,
            "snapshot must expose hard violations for inspector: {session:?}"
        );
        assert!(
            session
                .last_stream_violation
                .as_deref()
                .is_some_and(|s| s.contains("after Completed")
                    || s.contains("EventAfterCompleted")
                    || s.contains("event after")),
            "last violation detail missing: {:?}",
            session.last_stream_violation
        );
    }

    /// Leaves a tool open so soft warnings are recorded on finish.
    struct SoftWarningEngine;

    #[async_trait]
    impl EngineRuntime for SoftWarningEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "soft-1".into(),
                    model: None,
                    tools: vec![],
                },
                RuntimeEvent::ToolStart {
                    id: "t-open".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
                },
                RuntimeEvent::Completed {
                    session_id: "soft-1".into(),
                    result: None,
                    usage: None,
                },
            ])))
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
                streaming: false,
                tool_control: true,
                permission_callback: false,
                resume: false,
                cancel: false,
            }
        }
    }

    #[tokio::test]
    async fn stream_soft_warnings_are_recorded_on_snapshot() {
        let hub = HubRuntime::new();
        hub.register_engine("soft", Arc::new(SoftWarningEngine))
            .unwrap();

        let events: Vec<_> = hub
            .execute(HubRequest::new("soft", RuntimeRequest::new("probe")))
            .unwrap()
            .collect()
            .await;

        let session = hub.session(&events[0].product_session_id).unwrap();
        assert_eq!(session.status, HubSessionStatus::Completed);
        assert_eq!(session.stream_hard_violations, 0);
        assert!(
            session.stream_soft_warnings >= 1,
            "unclosed ToolStart must soft-warn: {session:?}"
        );
    }

    #[tokio::test]
    async fn clean_stream_has_zero_diagnostics() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        let events: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("hello")))
            .unwrap()
            .collect()
            .await;
        let session = hub.session(&events[0].product_session_id).unwrap();
        assert_eq!(session.stream_hard_violations, 0);
        assert_eq!(session.stream_soft_warnings, 0);
        assert!(session.last_stream_violation.is_none());
    }

    #[tokio::test]
    async fn switch_engine_to_unregistered_engine_fails() {
        let hub = HubRuntime::new();
        let alva = Arc::new(FakeEngine::new());
        hub.register_engine("alva", alva).unwrap();
        let first: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("on alva")))
            .unwrap()
            .collect()
            .await;
        let product_id = first[0].product_session_id.clone();

        let error = hub
            .switch_engine(&product_id, "codex")
            .expect_err("switching to an unregistered engine must fail");
        assert!(
            error.to_string().contains("codex"),
            "error names the missing engine: {error}"
        );
    }

    #[tokio::test]
    async fn switch_engine_refuses_a_running_session() {
        let hub = HubRuntime::new();
        let engine = Arc::new(FakeEngine::new());
        hub.register_engine("alva", engine).unwrap();
        let product_id = ProductSessionId::new();
        hub.restore_session(
            product_id.clone(),
            "alva",
            "native-running",
            HubSessionStatus::Running,
        )
        .unwrap();

        let error = hub
            .switch_engine(&product_id, "alva")
            .expect_err("a running session must not be switched");
        assert!(
            error.to_string().contains("already running"),
            "busy error names the state: {error}"
        );
    }

    #[tokio::test]
    async fn register_engine_rejects_duplicate_ids() {
        let hub = HubRuntime::new();
        let engine = Arc::new(FakeEngine::new());
        hub.register_engine("alva", engine).unwrap();
        let error = hub
            .register_engine("alva", Arc::new(FakeEngine::new()))
            .expect_err("a duplicate engine id must be rejected");
        assert!(
            error.to_string().contains("already registered"),
            "duplicate error names the id: {error}"
        );
    }
}
