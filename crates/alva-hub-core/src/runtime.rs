// INPUT:  alva_engine_runtime::{EngineRuntime, RuntimeEvent, RuntimeRequest}, tokio streams, uuid, HubError
// OUTPUT: HubRuntime registry/dispatch, HubRequest, HubEvent, product-session identity and snapshots
// POS:    Maps durable Hub product sessions to transient engine sessions without importing any agent implementation.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use alva_engine_runtime::{
    EngineDescriptor, EngineRuntime, PermissionDecision, RuntimeError, RuntimeEvent,
    RuntimeRequest, SoftStreamWarning, StreamContract, ToolOutput,
};
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};
use tokio_util::sync::CancellationToken;
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
    /// Monotonic per-turn claim counter. Every execute / abandon bumps it;
    /// a spawned turn only mutates the session while `claim_seq == my_seq`,
    /// so a superseded turn's tail writes can never clobber a newer claim.
    claim_seq: u64,
    /// Cancellation handle for the currently claimed turn. `abandon` cancels
    /// it so the spawned task stops draining an engine stream that will never
    /// terminate on its own.
    cancel_token: Option<CancellationToken>,
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
            claim_seq: 0,
            cancel_token: None,
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
    engines: Arc<RwLock<HashMap<EngineId, EngineDescriptor>>>,
    sessions: Arc<RwLock<HashMap<ProductSessionId, HubSession>>>,
    /// Live runs (ticket 13): per product session, the control-plane handle
    /// that can stop the run and reach its engine even after the registry
    /// entry for that engine is gone. Removed when the spawned task exits.
    runs: Arc<RwLock<HashMap<ProductSessionId, RunHandle>>>,
    /// R58: 进程内单调 run 代际计数。每个 RunHandle 携带登记时的代际，
    /// 旧 run 的尾部清理只删除同代际条目——快结束的旧轮与下一轮 execute
    /// 的登记竞争时，不会误删新 run 的控制面句柄。
    run_generation: Arc<AtomicU64>,
}

/// R63: 迟到控制面结果的定向释放目标。cancel 在 await 引擎结果期间，
/// 旧轮可能已结束、同 pid 的新轮可能已认领；await 返回后仅当当前
/// 认领者仍是目标代际/claim 时才允许 abandon 落地，旧结果不得触碰
/// 后来认领者。
enum ClaimTarget {
    /// live [`RunHandle`] 路径：按登记代际 + 该登记对应的会话 claim_seq
    /// 双重匹配。R92：runs 表与 sessions 表的更新时间线不同——新轮
    /// execute 已递增 claim_seq 但同步 engine.execute 尚未返回、RunHandle
    /// 未登记时，runs 里仍是旧条目，只比较旧 generation 会误判"仍是
    /// 当前认领者"。claim_seq 在 sessions 写锁内同事务校验，堵住这个
    /// claim→登记窗口。
    RunGeneration(u64, u64),
    /// 无 live run 的回退路径：按会话 claim_seq 匹配。
    ClaimSeq(u64),
}

/// A handle on one running `execute()` turn (ticket 13).
///
/// Holds a strong reference to the engine that produced the stream — unlike
/// the registry, which may drop the entry while the run is still alive — so
/// [`HubRuntime::cancel`] / [`HubRuntime::respond_permission`] keep working
/// for this run regardless of registry state.
#[derive(Clone)]
pub struct RunHandle {
    abort: tokio::task::AbortHandle,
    engine: Arc<dyn EngineRuntime>,
    engine_id: EngineId,
    /// Native session id, filled once `SessionStarted` arrives on this run.
    engine_session_id: Arc<RwLock<Option<String>>>,
    /// R58: 登记代际。尾部清理按代际匹配，旧轮只删自己的条目。
    generation: u64,
    /// R92: 登记时该轮持有的会话 claim_seq。定向释放必须同时匹配
    /// 代际与 claim——代际只证明 runs 表没换人，claim 才证明会话
    /// 认领没换人（claim→登记窗口内两者会短暂不一致）。
    claim_seq: u64,
}

impl HubRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one product-visible engine with its self-description
    /// (ticket 15 batch 1/2). The descriptor carries the permission plans,
    /// config surface, and install metadata that products consult instead of
    /// matching on engine names. Existing callers keep using
    /// [`Self::register_engine`] (runtime-only) until they migrate batch-by-batch.
    ///
    /// Replacement is fail-closed because routing ambiguity would otherwise
    /// let a new registration silently hijack the control plane of runs that
    /// began on the old instance.
    pub fn register_engine_descriptor(
        &self,
        id: impl Into<EngineId>,
        descriptor: EngineDescriptor,
    ) -> Result<(), HubError> {
        let id = id.into();
        let mut engines = self
            .engines
            .write()
            .expect("Hub engine registry lock poisoned");
        if engines.contains_key(&id) {
            return Err(HubError::EngineAlreadyRegistered(id.to_string()));
        }
        engines.insert(id, descriptor);
        Ok(())
    }

    /// The full self-description registered for an engine, so products can
    /// decide permission/readiness/config questions without engine-name
    /// literals (ticket 15 batch 3).
    pub fn descriptor(&self, id: &EngineId) -> Option<EngineDescriptor> {
        self.engines
            .read()
            .expect("Hub engine registry lock poisoned")
            .get(id)
            .cloned()
    }

    /// Register one product-visible engine.
    ///
    /// Replacement is fail-closed because routing ambiguity would otherwise
    /// let a new registration silently hijack the control plane of runs that
    /// began on the old instance.
    pub fn register_engine(
        &self,
        id: impl Into<EngineId>,
        engine: Arc<dyn EngineRuntime>,
    ) -> Result<(), HubError> {
        self.register_engine_descriptor(id, EngineDescriptor::from_runtime(engine))
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

    /// Remove an engine from the directory (ticket 13).
    ///
    /// New executes on this id fail. Runs that started before the removal
    /// keep working through their [`RunHandle`]'s strong engine reference, so
    /// cancel / respond_permission still reach them.
    pub fn unregister_engine(&self, id: &EngineId) -> bool {
        self.engines
            .write()
            .expect("Hub engine registry lock poisoned")
            .remove(id)
            .is_some()
    }

    /// R66: 实例身份比较。ActiveHubs sink 没有代际字段，同步启动失败的
    /// 回收用本方法判定条目是否仍是自己发布的实例，避免误删并发的
    /// 后继代际。
    pub fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.sessions, &other.sessions)
    }

    /// Whether a run is currently registered for this product session
    /// (inspection / tests).
    pub fn has_live_run(&self, product_session_id: &ProductSessionId) -> bool {
        self.runs
            .read()
            .expect("Hub runs lock poisoned")
            .contains_key(product_session_id)
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
    /// `--resume`). In-memory engines like Alva resume across processes when
    /// the adapter is configured with a native record repository
    /// (`alva-hub-desktop/src/alva.rs` wires `SessionRepository`); without
    /// one, the same-process adapter instance must still hold the session.
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
        // A restored session is rehydrated from disk with no running turn.
        // Refuse non-terminal states so a product cannot plant a session that
        // is forever "already running" with no recoverable exit.
        if !matches!(
            status,
            HubSessionStatus::Completed | HubSessionStatus::Failed
        ) {
            return Err(HubError::SessionNotReady(format!(
                "restored session '{product_session_id}' cannot be in non-terminal state {status:?}"
            )));
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

    /// Abandon a stuck product session and release its claim.
    ///
    /// Consumers that give up on a turn (timeout, Ctrl-C, closed tab) must not
    /// leave the session permanently busy. `abandon` cancels the in-flight
    /// spawned turn (via its [`CancellationToken`]) and moves the session to a
    /// terminal [`HubSessionStatus::Failed`], so the next [`Self::execute`] or
    /// [`Self::switch_engine`] can reclaim it. The abandoned turn's tail writes
    /// are blocked by the `claim_seq` guard.
    pub fn abandon(
        &self,
        product_session_id: &ProductSessionId,
    ) -> Result<HubSessionSnapshot, HubError> {
        let mut sessions = self
            .sessions
            .write()
            .expect("Hub session registry lock poisoned");
        Ok(Self::abandon_locked(&mut sessions, &self.runs, product_session_id)?
            .expect("unguarded abandon always releases"))
    }

    /// R63: 带认领校验的 abandon。校验与释放同处一次 sessions 写锁持有
    /// 内，多线程下不存在"校验通过后新 claim 插入"的窗口。目标已不是
    /// 当前认领者时返回 `Ok(None)`，不触碰会话状态。
    fn abandon_targeted(
        &self,
        product_session_id: &ProductSessionId,
        target: ClaimTarget,
    ) -> Result<Option<HubSessionSnapshot>, HubError> {
        let mut sessions = self
            .sessions
            .write()
            .expect("Hub session registry lock poisoned");
        let still_current = match target {
            ClaimTarget::RunGeneration(expected_generation, expected_claim) => {
                // R92: 同一把 sessions 写锁内同时校验 runs 代际与会话
                // claim——校验和释放之间新 execute/abandon 无法插入。
                let claim_matches =
                    sessions.get(product_session_id).map(|s| s.claim_seq) == Some(expected_claim);
                claim_matches
                    && self
                        .runs
                        .read()
                        .expect("Hub runs lock poisoned")
                        .get(product_session_id)
                        .map(|handle| handle.generation)
                        == Some(expected_generation)
            }
            ClaimTarget::ClaimSeq(expected) => {
                sessions.get(product_session_id).map(|s| s.claim_seq) == Some(expected)
            }
        };
        if !still_current {
            return Ok(None);
        }
        Self::abandon_locked(&mut sessions, &self.runs, product_session_id)
    }

    /// abandon 主体；调用方必须已持有 sessions 写锁。返回 `None` 表示
    /// 会话不存在。
    fn abandon_locked(
        sessions: &mut HashMap<ProductSessionId, HubSession>,
        runs: &RwLock<HashMap<ProductSessionId, RunHandle>>,
        product_session_id: &ProductSessionId,
    ) -> Result<Option<HubSessionSnapshot>, HubError> {
        let session = sessions
            .get_mut(product_session_id)
            .ok_or_else(|| HubError::SessionNotFound(product_session_id.to_string()))?;
        if let Some(token) = session.cancel_token.take() {
            token.cancel();
        }
        // Abort the run task (ticket 13): even a stream that ignores the
        // cancellation token is stopped at the task boundary. The run entry is
        // removed here because an aborted task never runs its body-end cleanup.
        if let Some(handle) = runs
            .write()
            .expect("Hub runs lock poisoned")
            .remove(product_session_id)
        {
            handle.abort.abort();
        }
        // Invalidate any in-flight turn so its tail writes become no-ops even
        // if a new claim races in before the old task finishes unwinding.
        session.claim_seq = session.claim_seq.wrapping_add(1);
        session.status = HubSessionStatus::Failed;
        Ok(Some(session.snapshot(product_session_id.clone())))
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
        let cancel_token = CancellationToken::new();

        // Claim the product session under one write lock. Without this
        // transition a pair of concurrent resume calls can both observe
        // `Completed` and enter the same native session.
        let (previous_session, handoff_context_used, my_seq) = {
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
                        if engine.capabilities().resume {
                            runtime_request.resume_session = Some(engine_sid.clone());
                        } else {
                            // Stateless engine (ticket 14 D-2): a stored native
                            // id cannot be resumed. Drop it and start a fresh
                            // native session instead of failing the turn forever
                            // with ResumeUnsupported.
                            existing.engine_session_id = None;
                            runtime_request.resume_session = None;
                        }
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
                        .or_else(|| existing.pending_handoff_context.clone())
                        .unwrap_or_else(|| {
                            default_handoff_banner(prev, request.engine_id.as_str())
                        });
                    runtime_request.prompt =
                        format!("{banner}\n\n## User message\n\n{}", runtime_request.prompt);
                    // R90: the flag and the exact banner stay ON the session
                    // until this turn's OUTCOME is known. Consuming them here
                    // left a fatal first turn retrying with no handoff
                    // context (the old stream's EOF restore is generation-
                    // skipped once the retry claims). Success clears both at
                    // the terminal projection; a failed turn keeps them so
                    // the retry re-injects the same banner exactly once.
                    existing.pending_handoff_context = Some(banner.clone());
                    handoff_context_used = Some(banner);
                }

                existing.claim_seq = existing.claim_seq.wrapping_add(1);
                let my_seq = existing.claim_seq;
                existing.cancel_token = Some(cancel_token.clone());
                existing.status = HubSessionStatus::Starting;
                // New execute turn: diagnostics describe the stream about to run.
                existing.reset_stream_diagnostics();
                (Some(rollback), handoff_context_used, my_seq)
            } else {
                let mut fresh = HubSession::new(request.engine_id.clone());
                fresh.claim_seq = 1;
                fresh.cancel_token = Some(cancel_token.clone());
                sessions.insert(product_session_id.clone(), fresh);
                (None, None, 1)
            }
        };

        // True when this turn carries a native resume id (filled by the claim
        // block above): a fatal stream that never starts a native session means
        // the id was rejected and must be cleared (ticket 11). Captured before
        // `engine.execute` consumes the request.
        let was_resume = runtime_request.resume_session.is_some();
        let engine_stream = match engine.execute(runtime_request) {
            Ok(stream) => stream,
            Err(error) => {
                let mut sessions = self
                    .sessions
                    .write()
                    .expect("Hub session registry lock poisoned");
                // R64: 仅当本 execute 仍是当前认领者时回滚。同步 execute
                // Err 返回前 claim 已被 abandon + 新轮取代时，不得用旧
                // 快照覆写、也不得删除新轮的注册表条目。
                if sessions.get(&product_session_id).map(|s| s.claim_seq) == Some(my_seq) {
                    if let Some(mut previous) = previous_session {
                        previous.pending_handoff_context = handoff_context_used;
                        sessions.insert(product_session_id.clone(), previous);
                    } else {
                        sessions.remove(&product_session_id);
                    }
                }
                return Err(error.into());
            }
        };
        let mut failed_handoff_rollback = previous_session.zip(handoff_context_used);
        // R90: whether THIS claim injected the handoff preamble — captured
        // before the rollback pair consumes the context.
        let handoff_turn = failed_handoff_rollback.is_some();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let sessions = self.sessions.clone();
        let runs = self.runs.clone();
        let engine_id = request.engine_id;
        let run_engine_id = engine_id.clone();
        let output_session_id = product_session_id.clone();
        let run_key = output_session_id.clone();
        // Shared slot that mirrors `engine_session_id` for this run, so the
        // RunHandle can route cancel/permission even after the registry entry
        // for this engine is gone (ticket 13).
        let run_slot = Arc::new(RwLock::new(None::<String>));
        let handle_run_slot = run_slot.clone();
        // R58: 登记握手。任务尾部清理前必须收到本侧完成 RunHandle 登记
        // 的信号（同一代际），使"先 remove 后 insert"的残留不可能。
        let (registered_tx, registered_rx) = tokio::sync::oneshot::channel::<u64>();

        let task = tokio::spawn(async move {
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
            let mut saw_session_started = false;
            // R93: 本流自己观察到的 native id。handoff 失败轮的身份在
            // fatal 事件时即从会话表清除（重试不得 resume），合成终态对
            // 仍应携带本轮真实启动过的 native id，投影不因清除而变空。
            let mut native_id: Option<String> = None;
            // R49: 循环内实时跟踪未闭合工具。终态事件（Completed / fatal
            // Error）在循环内转发，补发 ToolEnd 必须发生在转发之前——
            // 产品收到终态后可能立即停止消费，之后的 ToolEnd 无人处理。
            let mut open_tools: HashMap<String, String> = HashMap::new();
            let run_slot = run_slot;
            loop {
                // `abandon` cancels this token so the loop stops draining an
                // engine stream that will never terminate on its own.
                let event = tokio::select! {
                    ev = engine_stream.next() => ev,
                    _ = cancel_token.cancelled() => None,
                };
                let Some(event) = event else { break };
                if let Err(err) = contract.observe(&event) {
                    hard_violations = hard_violations.saturating_add(1);
                    record_hard_violation(&sessions, &output_session_id, my_seq, &err.to_string());
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
                saw_session_started |= matches!(&event, RuntimeEvent::SessionStarted { .. });
                if let RuntimeEvent::SessionStarted { session_id, .. } = &event {
                    *run_slot.write().expect("run slot lock poisoned") = Some(session_id.clone());
                    native_id = Some(session_id.clone());
                }
                saw_fatal_error |= matches!(
                    &event,
                    RuntimeEvent::Error {
                        recoverable: false,
                        ..
                    }
                );
                match &event {
                    RuntimeEvent::ToolStart { id, name, .. } => {
                        open_tools.insert(id.clone(), name.clone());
                    }
                    RuntimeEvent::ToolEnd { id, .. } => {
                        open_tools.remove(id);
                    }
                    _ => {}
                }
                update_session(
                    &sessions,
                    &output_session_id,
                    my_seq,
                    &event,
                    handoff_turn,
                );
                // R49: 引擎自带的终态事件转发前，先补齐仍未闭合的工具——
                // ToolEnd 必须先于 Error/Completed 到达产品。
                let forwarding_terminal = matches!(&event, RuntimeEvent::Completed { .. })
                    || matches!(
                        &event,
                        RuntimeEvent::Error {
                            recoverable: false,
                            ..
                        }
                    );
                if forwarding_terminal && output_open && !open_tools.is_empty() {
                    for (id, name) in std::mem::take(&mut open_tools) {
                        let _ = event_tx.send(HubEvent {
                            product_session_id: output_session_id.clone(),
                            engine_id: engine_id.clone(),
                            event: RuntimeEvent::ToolEnd {
                                id,
                                name,
                                result: ToolOutput::error(
                                    "engine ended with tool still running".to_string(),
                                ),
                                duration_ms: None,
                            },
                        });
                    }
                }
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
            // Unclosed tools are closed with a synthesized failing ToolEnd so
            // a mid-tool death / stall does not leave the product's tool card
            // stuck on "running" (ticket 12). Take warnings on both branches.
            let mut unclosed_tools = Vec::new();
            match contract.finish() {
                Ok(()) => {
                    let warnings = contract.soft_warnings();
                    let soft_count = warnings.len() as u32;
                    if soft_count > 0 {
                        record_soft_warnings(&sessions, &output_session_id, my_seq, soft_count);
                    }
                    for warning in warnings {
                        if let SoftStreamWarning::UnclosedTool { id, name } = &warning {
                            unclosed_tools.push((id.clone(), name.clone()));
                        }
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
                        record_hard_violation(
                            &sessions,
                            &output_session_id,
                            my_seq,
                            &err.to_string(),
                        );
                    }
                    for warning in contract.soft_warnings() {
                        if let SoftStreamWarning::UnclosedTool { id, name } = &warning {
                            unclosed_tools.push((id.clone(), name.clone()));
                        }
                    }
                    warn!(
                        product_session = %output_session_id,
                        engine = %engine_id,
                        %err,
                        "engine stream failed stream-contract finish check"
                    );
                }
            }
            // R49: 循环内已在终态转发前补过的工具不再重复补——contract 的
            // finish 快照仍视其为未闭合，但转发面已经关闭它们；open_tools
            // 只剩从未到达终态（EOF / 中断路径）的未闭合工具。
            unclosed_tools.retain(|(id, _)| open_tools.contains_key(id));
            if hard_violations > 0 {
                debug!(
                    product_session = %output_session_id,
                    engine = %engine_id,
                    hard_violations,
                    "engine stream finished with contract hard violations"
                );
            }
            // A resume turn that never started a native session and failed
            // fatally means the stored id was rejected. Drop it so the next
            // turn attempts a fresh native session instead of reusing a dead id
            // forever (ticket 11).
            if was_resume && !saw_session_started && saw_fatal_error {
                clear_rejected_resume_session(&sessions, &output_session_id, my_seq);
            }
            // Close any tools left open, before the terminal pair so the
            // product receives ToolEnd before Error/Completed.
            if !unclosed_tools.is_empty() && output_open {
                for (id, name) in &unclosed_tools {
                    let _ = event_tx.send(HubEvent {
                        product_session_id: output_session_id.clone(),
                        engine_id: engine_id.clone(),
                        event: RuntimeEvent::ToolEnd {
                            id: id.clone(),
                            name: name.clone(),
                            result: ToolOutput::error(
                                "engine ended with tool still running".to_string(),
                            ),
                            duration_ms: None,
                        },
                    });
                }
            }
            if !saw_completed {
                saw_fatal_error = true;
                // R97: 失败状态释放与 handoff 身份恢复在同一临界区完成，
                // 且都发生在合成终态对消费者可见之前。旧序（先发终态、后
                // 恢复身份）留有窗口：消费者在 Error 可见后立即认领下一
                // 轮，会 resume 仍然存储的失败 native id；迟到的恢复又因
                // claim_seq 更新被跳过，死 id 从此滞留。
                let engine_session_id = settle_synthetic_failure(
                    &sessions,
                    &output_session_id,
                    my_seq,
                    failed_handoff_rollback.take(),
                )
                // R93: handoff 失败轮的会话表身份已在 fatal 时清除，
                // 合成终态对改用本流观察到的 native id，其余路径不变。
                .or_else(|| if handoff_turn { native_id.clone() } else { None })
                .unwrap_or_else(|| {
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
            // A resume turn that never started a native session and failed
            // fatally means the stored id was rejected. Drop it so the next
            // turn attempts a fresh native session instead of reusing a dead id
            // forever (ticket 11).
            if was_resume && !saw_session_started && saw_fatal_error {
                clear_rejected_resume_session(&sessions, &output_session_id, my_seq);
            }
            if saw_fatal_error {
                // R97: handoff 回滚已在 settle_synthetic_failure 的同一临界
                // 区内完成（take() 已消费）；这里只剩无合成终态的显式 fatal
                // 路径（引擎自身发出 Completed 的失败流）。
                if let Some((previous, context)) = failed_handoff_rollback {
                    restore_failed_handoff(
                        &sessions,
                        &output_session_id,
                        previous,
                        context,
                        my_seq,
                    );
                }
            }
            // This run is done; release the control-plane handle so a later
            // execute on the same product session can register a fresh one.
            // R58: 清理只删除自身代际的条目——旧轮尾部与下一轮 execute 的
            // 登记竞争时，不能误删新 run 的句柄。删除前等待登记握手，
            // "极快结束先 remove 后 insert"造成的永久残留不再可能。
            let my_generation = match registered_rx.await {
                Ok(generation) => generation,
                // 登记方消失意味着 execute 已放弃本轮（任务被 abort），
                // 条目由 abort 方负责。
                Err(_) => return,
            };
            let mut runs = runs.write().expect("Hub runs lock poisoned");
            if runs.get(&output_session_id).map(|h| h.generation) == Some(my_generation) {
                runs.remove(&output_session_id);
            }
        });

        // R58: 登记代际在本侧生成；任务尾部经握手拿到同一代际后再清理。
        // 握手保证删除不会先于登记执行。
        let my_generation = self.run_generation.fetch_add(1, Ordering::SeqCst) + 1;

        // R64: 登记前校验本 execute 仍是当前认领者。旧 execute 在同步
        // `engine.execute` 中被 abandon、新轮已认领时，迟到的登记不得
        // 覆盖新轮的 RunHandle。sessions 写锁覆盖校验+插入，期间新
        // execute/abandon 无法递增 claim_seq。
        let mut registered = false;
        {
            let sessions = self
                .sessions
                .write()
                .expect("Hub session registry lock poisoned");
            if sessions.get(&product_session_id).map(|s| s.claim_seq) == Some(my_seq) {
                // Hold the abort handle + a strong engine reference so this run stays
                // controllable even if its engine is later unregistered.
                self.runs.write().expect("Hub runs lock poisoned").insert(
                    run_key,
                    RunHandle {
                        abort: task.abort_handle(),
                        engine,
                        engine_id: run_engine_id,
                        engine_session_id: handle_run_slot,
                        generation: my_generation,
                        claim_seq: my_seq,
                    },
                );
                registered = true;
            }
        }
        // R64: 登记被跳过时丢弃握手发送端——任务尾部收到 Err 直接返回，
        // 不会误删新代际的条目。
        if registered {
            let _ = registered_tx.send(my_generation);
        }

        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    /// Route a product-level cancellation request to the active engine session.
    ///
    /// Prefers the live [`RunHandle`] (strong engine ref + native session id),
    /// so cancellation works even after the engine's registry entry is gone
    /// (ticket 13). Falls back to the session/registry path for restored
    /// sessions with no live run. Two-tier (ticket 08): when the request
    /// cannot reach a native session, Hub still releases its own claim via
    /// [`Self::abandon`].
    ///
    /// R50: 能力门控覆盖两条路径——引擎声明不支持 cancel（或实现与声明
    /// 不符地返回 Unsupported）时同样走 abandon 释放 Hub claim，会话回到
    /// 可重开状态。Hub 释放只代表本进程放弃这一轮，不代表引擎进程已终止。
    pub async fn cancel(&self, product_session_id: &ProductSessionId) -> Result<(), HubError> {
        let live_handle = self
            .runs
            .read()
            .expect("Hub runs lock poisoned")
            .get(product_session_id)
            .cloned();
        if let Some(handle) = live_handle {
            // R63: await 前固定目标代际——await 返回后的释放只允许作用于
            // 这一代际，旧结果不得触碰后来认领的新轮。R92: 同时固定该
            // 登记对应的 claim_seq，claim→登记窗口内新轮已换 claim 时
            // 释放被跳过，旧 handle 不再能操作新会话。
            let my_generation = handle.generation;
            let my_claim_seq = handle.claim_seq;
            let engine_session_id = {
                handle
                    .engine_session_id
                    .read()
                    .expect("run slot lock poisoned")
                    .clone()
            };
            if let Some(engine_session_id) = engine_session_id {
                if !handle.engine.capabilities().cancel {
                    // R80: the capability bypass releases only THIS
                    // generation. Between the handle clone and this release a
                    // concurrent abandon+execute may have claimed a new run
                    // under the same product id; an untargeted abandon would
                    // tear that new run down.
                    self.abandon_targeted(
                        product_session_id,
                        ClaimTarget::RunGeneration(my_generation, my_claim_seq),
                    )?;
                    return Ok(());
                }
                return match handle.engine.cancel(&engine_session_id).await {
                    Ok(()) => Ok(()),
                    Err(RuntimeError::Unsupported(_)) => {
                        // R63: 旧轮 cancel 的迟到 Unsupported 只释放旧代际；
                        // 新轮已认领时跳过释放，新轮继续运行。
                        self.abandon_targeted(
                            product_session_id,
                            ClaimTarget::RunGeneration(my_generation, my_claim_seq),
                        )?;
                        Ok(())
                    }
                    Err(err) => Err(err.into()),
                };
            }
            // R80: no native id yet — same targeted release as the bypass
            // above; the saved generation is the only claim this cancel may
            // release.
            self.abandon_targeted(
                product_session_id,
                ClaimTarget::RunGeneration(my_generation, my_claim_seq),
            )?;
            return Ok(());
        }

        // R80: route (native id) and claim identity must come from ONE
        // session snapshot. Sampling claim_seq after the capability gate can
        // pick up a newer generation's claim and aim the release at the new
        // turn even though the cancel went to the old native id.
        let (engine_id, engine_session_id, my_claim_seq) = {
            let sessions = self
                .sessions
                .read()
                .expect("Hub session registry lock poisoned");
            let session = sessions
                .get(product_session_id)
                .ok_or_else(|| HubError::SessionNotFound(product_session_id.to_string()))?;
            (
                session.engine_id.clone(),
                session.engine_session_id.clone(),
                session.claim_seq,
            )
        };
        let engine = self.engine(&engine_id)?;
        let Some(engine_session_id) = engine_session_id else {
            self.abandon_targeted(product_session_id, ClaimTarget::ClaimSeq(my_claim_seq))?;
            return Ok(());
        };
        if !engine.capabilities().cancel {
            self.abandon_targeted(product_session_id, ClaimTarget::ClaimSeq(my_claim_seq))?;
            return Ok(());
        }
        // R63: 回退路径同样在 await 前固定目标 claim（R80：与路由同快照）。
        match engine.cancel(&engine_session_id).await {
            Ok(()) => Ok(()),
            Err(RuntimeError::Unsupported(_)) => {
                self.abandon_targeted(product_session_id, ClaimTarget::ClaimSeq(my_claim_seq))?;
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Route a permission decision from Hub UI/CLI to the engine session that
    /// emitted the corresponding request.
    pub async fn respond_permission(
        &self,
        product_session_id: &ProductSessionId,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), HubError> {
        // Prefer the live run's engine reference (ticket 13) so permission
        // routing survives an engine being unregistered mid-run.
        let live_handle = self
            .runs
            .read()
            .expect("Hub runs lock poisoned")
            .get(product_session_id)
            .cloned();
        if let Some(handle) = live_handle {
            let engine_session_id = {
                handle
                    .engine_session_id
                    .read()
                    .expect("run slot lock poisoned")
                    .clone()
            };
            let Some(engine_session_id) = engine_session_id else {
                return Err(HubError::SessionNotReady(product_session_id.to_string()));
            };
            if !handle.engine.capabilities().permission_callback {
                return Err(HubError::PermissionUnsupported(
                    handle.engine_id.to_string(),
                ));
            }
            handle
                .engine
                .respond_permission(&engine_session_id, request_id, decision)
                .await?;
            return Ok(());
        }

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
            .map(|descriptor| descriptor.runtime.clone())
            .ok_or_else(|| HubError::EngineNotFound(id.to_string()))
    }
}

fn update_session(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    my_seq: u64,
    event: &RuntimeEvent,
    handoff_turn: bool,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let Some(session) = sessions.get_mut(product_session_id) else {
        return;
    };
    // A superseded turn must never mutate the session a newer claim owns.
    if session.claim_seq != my_seq {
        return;
    }
    match event {
        RuntimeEvent::SessionStarted { session_id, .. } => {
            session.engine_session_id = Some(session_id.clone());
            if session.status != HubSessionStatus::Failed {
                session.status = HubSessionStatus::Running;
            }
        }
        RuntimeEvent::Error {
            recoverable: false, ..
        } if session.status != HubSessionStatus::Completed => {
            session.status = HubSessionStatus::Failed;
            // R93: 失败的首个 handoff 轮必须在重试被允许之前就完成与 EOF
            // 后 restore_failed_handoff 相同的身份恢复——否则 fatal→EOF
            // 窗口内的新认领会拿到 SessionStarted 留下的失败轮 native id
            // 并 resume 它。banner/pending_handoff 仍由 R90 语义保留到
            // 成功为止；EOF 尾部的恢复按 claim 校验照常执行或跳过。
            if handoff_turn {
                session.engine_session_id = None;
            }
        }
        RuntimeEvent::Completed { .. } if session.status != HubSessionStatus::Failed => {
            session.status = HubSessionStatus::Completed;
            // R90: a handoff turn that finishes successfully consumes the
            // preamble exactly once. The clear must be atomic with the
            // success terminal — clearing it at claim time left a fatal
            // first turn retrying with no handoff context, and clearing it
            // in a separate later step would race a next claim in between.
            if handoff_turn {
                session.pending_handoff = false;
                session.pending_handoff_context = None;
            }
        }
        _ => {}
    }
}

fn mark_session_failed(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    my_seq: u64,
) -> Option<String> {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let session = sessions.get_mut(product_session_id)?;
    if session.claim_seq != my_seq {
        return None;
    }
    session.status = HubSessionStatus::Failed;
    session.engine_session_id.clone()
}

/// R97: release the retryable Failed state AND restore the handoff identity
/// in ONE critical section, and do it BEFORE the synthetic terminal pair
/// becomes visible to the consumer. The previous two-step order opened a
/// window where the session was already Failed (a consumer retry could claim
/// it) while the failed native id was still stored — the retry resumed the
/// dead id, and the late `restore_failed_handoff` then skipped itself on the
/// claim_seq guard, leaving the poisoned id in place forever.
///
/// Returns the failed turn's native session id for the synthetic Completed
/// (the same contract as `mark_session_failed`), or None when the claim was
/// lost. `None` stored id stays None — the caller's R93 fallback supplies the
/// stream-observed id for handoff turns.
fn settle_synthetic_failure(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    my_seq: u64,
    rollback: Option<(HubSession, String)>,
) -> Option<String> {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    {
        let session = sessions.get_mut(product_session_id)?;
        if session.claim_seq != my_seq {
            return None;
        }
        session.status = HubSessionStatus::Failed;
    }
    let native_id = sessions
        .get(product_session_id)
        .and_then(|session| session.engine_session_id.clone());
    if let Some((mut previous, context)) = rollback {
        let Some(failed) = sessions.get(product_session_id) else {
            return native_id;
        };
        previous.stream_hard_violations = failed.stream_hard_violations;
        previous.stream_soft_warnings = failed.stream_soft_warnings;
        previous.last_stream_violation = failed.last_stream_violation.clone();
        previous.claim_seq = failed.claim_seq;
        previous.cancel_token = failed.cancel_token.clone();
        previous.pending_handoff_context = Some(context);
        // The turn failed; never restore the rollback's stale terminal status
        // (ticket 07) — a fatal turn must be observable as Failed, not
        // Completed.
        previous.status = HubSessionStatus::Failed;
        sessions.insert(product_session_id.clone(), previous);
    }
    native_id
}

/// Drop a rejected native session id after a failed resume turn so the next
/// execute starts fresh instead of reusing a dead id (ticket 11).
fn clear_rejected_resume_session(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    my_seq: u64,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let Some(session) = sessions.get_mut(product_session_id) else {
        return;
    };
    if session.claim_seq != my_seq {
        return;
    }
    session.engine_session_id = None;
}

fn record_hard_violation(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    my_seq: u64,
    detail: &str,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let Some(session) = sessions.get_mut(product_session_id) else {
        return;
    };
    if session.claim_seq != my_seq {
        return;
    }
    session.stream_hard_violations = session.stream_hard_violations.saturating_add(1);
    session.last_stream_violation = Some(detail.to_string());
}

fn record_soft_warnings(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    my_seq: u64,
    count: u32,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let Some(session) = sessions.get_mut(product_session_id) else {
        return;
    };
    if session.claim_seq != my_seq {
        return;
    }
    session.stream_soft_warnings = session.stream_soft_warnings.saturating_add(count);
}

fn restore_failed_handoff(
    sessions: &RwLock<HashMap<ProductSessionId, HubSession>>,
    product_session_id: &ProductSessionId,
    mut previous: HubSession,
    context: String,
    my_seq: u64,
) {
    let mut sessions = sessions
        .write()
        .expect("Hub session registry lock poisoned");
    let Some(failed) = sessions.get(product_session_id) else {
        return;
    };
    // If a newer claim has taken over this session, the failed turn's tail
    // must not clobber the newer turn's state.
    if failed.claim_seq != my_seq {
        return;
    }
    previous.stream_hard_violations = failed.stream_hard_violations;
    previous.stream_soft_warnings = failed.stream_soft_warnings;
    previous.last_stream_violation = failed.last_stream_violation.clone();
    previous.claim_seq = failed.claim_seq;
    previous.cancel_token = failed.cancel_token.clone();
    previous.pending_handoff_context = Some(context);
    // The turn failed; never restore the rollback's stale terminal status
    // (ticket 07) — a fatal turn must be observable as Failed, not Completed.
    previous.status = HubSessionStatus::Failed;
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
            0,
            &RuntimeEvent::Error {
                message: "engine failed".into(),
                recoverable: false,
            },
            false,
        );
        update_session(
            &sessions,
            &product_session_id,
            0,
            &RuntimeEvent::Completed {
                session_id: String::new(),
                result: None,
                usage: None,
            },
                    false,
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
            0,
            &RuntimeEvent::Error {
                message: "temporary failure".into(),
                recoverable: true,
            },
                    false,
        );
        update_session(
            &sessions,
            &product_session_id,
            0,
            &RuntimeEvent::SessionStarted {
                session_id: "native-session".into(),
                model: None,
                tools: vec![],
            },
                    false,
        );
        update_session(
            &sessions,
            &product_session_id,
            0,
            &RuntimeEvent::Completed {
                session_id: "native-session".into(),
                result: Some("done".into()),
                usage: None,
            },
                    false,
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

    /// R90: the fatal first handoff turn must not lose the preamble when the
    /// retry claims BEFORE the old stream reaches EOF. The old stream parks
    /// after its fatal Error; the retry (same product session, no handoff
    /// context of its own) must start with the SAME banner injected exactly
    /// once; releasing the old stream then EOFs it, and its restore/cleanup
    /// must be generation-skipped without clobbering the new turn.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn retry_before_handoff_stream_eof_still_carries_the_banner() {
        struct GatedFatalHandoffEngine {
            turn: AtomicUsize,
            // Async gate on purpose: a parked stream task must stay
            // droppable when a failing assertion unwinds the test.
            eof_gate: Arc<tokio::sync::Notify>,
            requests: Mutex<Vec<String>>,
        }
        #[async_trait]
        impl EngineRuntime for GatedFatalHandoffEngine {
            fn execute(
                &self,
                request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                let n = self.turn.fetch_add(1, Ordering::SeqCst);
                self.requests.lock().unwrap().push(request.prompt.clone());
                let session_id = format!("native-{n}");
                let started = stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: session_id.clone(),
                    model: None,
                    tools: vec![],
                }]);
                if n == 0 {
                    // First handoff turn: fatal Error, then park BEFORE EOF.
                    let gate = self.eof_gate.clone();
                    let gate_session = session_id.clone();
                    Ok(Box::pin(
                        started
                            .chain(stream::iter(vec![RuntimeEvent::Error {
                                message: "fatal mid-handoff".into(),
                                recoverable: false,
                            }]))
                            .chain(stream::once(async move {
                                gate.notified().await;
                                RuntimeEvent::Completed {
                                    session_id: gate_session,
                                    result: None,
                                    usage: None,
                                }
                            })),
                    ))
                } else {
                    Ok(Box::pin(started.chain(stream::iter(vec![
                        RuntimeEvent::Completed {
                            session_id,
                            result: Some("retry done".into()),
                            usage: None,
                        },
                    ]))))
                }
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

        let eof_gate = Arc::new(tokio::sync::Notify::new());
        let retry_engine = Arc::new(GatedFatalHandoffEngine {
            turn: AtomicUsize::new(0),
            eof_gate: eof_gate.clone(),
            requests: Mutex::new(Vec::new()),
        });
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        hub.register_engine("retry", retry_engine.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("start")))
            .unwrap()
            .collect()
            .await;
        let product_id = first[0].product_session_id.clone();
        hub.switch_engine(&product_id, "retry").unwrap();

        // 第一轮带唯一标记；流在 fatal 之后、EOF 之前停住。
        let mut failed_stream = hub
            .execute(
                HubRequest::new("retry", RuntimeRequest::new("first attempt"))
                    .resume(product_id.clone())
                    .with_handoff_context("R90_HANDOFF_MARKER"),
            )
            .unwrap();
        let mut saw_fatal = false;
        while let Some(event) = failed_stream.next().await {
            if matches!(
                event.event,
                RuntimeEvent::Error {
                    recoverable: false,
                    ..
                }
            ) {
                saw_fatal = true;
                break;
            }
        }
        assert!(saw_fatal, "first handoff turn must turn fatal");
        let between = hub.session(&product_id).unwrap();
        assert_eq!(between.status, HubSessionStatus::Failed);
        assert!(
            between.pending_handoff,
            "a fatal handoff turn must keep the preamble pending"
        );

        // 旧流 EOF 未到达时的重跑：不得丢交接上下文，标记恰好一次。
        let retried: Vec<_> = hub
            .execute(
                HubRequest::new("retry", RuntimeRequest::new("second attempt"))
                    .resume(product_id.clone()),
            )
            .unwrap()
            .collect()
            .await;
        assert!(matches!(
            retried.last().unwrap().event,
            RuntimeEvent::Completed { .. }
        ));
        {
            let requests = retry_engine.requests.lock().unwrap();
            assert!(requests[1].contains("second attempt"));
            assert_eq!(
                requests[1].matches("R90_HANDOFF_MARKER").count(),
                1,
                "the retry must carry the original banner exactly once: {}",
                requests[1]
            );
        }
        let after_retry = hub.session(&product_id).unwrap();
        assert_eq!(after_retry.status, HubSessionStatus::Completed);
        assert!(!after_retry.pending_handoff);

        // 释放旧流：其尾部恢复/合成被代际校验跳过，不覆写新轮。
        eof_gate.notify_one();
        while let Some(_event) = failed_stream.next().await {}
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let final_state = hub.session(&product_id).unwrap();
        assert_eq!(
            final_state.status,
            HubSessionStatus::Completed,
            "the abandoned stream's tail must not overwrite the retry turn"
        );
        assert!(!final_state.pending_handoff);
        let requests = retry_engine.requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "the stale stream must not re-execute");
    }

    /// R93: fatal→EOF 窗口内的 handoff 重试不得 resume 失败轮的 native
    /// 身份。失败轮的 SessionStarted 把 native id 写进了会话表；R90 只
    /// 保住了 banner，重试请求却仍携带 `resume_session=Some(失败轮 id)`。
    /// 身份恢复现在在 fatal 事件时同步完成：窗口内重试拿到
    /// resume_session=None、banner 恰好一次；旧流尾部按 claim 校验跳过，
    /// 不覆写新轮。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn retry_before_handoff_eof_does_not_resume_failed_native() {
        struct GatedFatalNativeEngine {
            turn: AtomicUsize,
            eof_gate: Arc<tokio::sync::Notify>,
            requests: Mutex<Vec<RuntimeRequest>>,
        }
        #[async_trait]
        impl EngineRuntime for GatedFatalNativeEngine {
            fn execute(
                &self,
                request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                let n = self.turn.fetch_add(1, Ordering::SeqCst);
                self.requests.lock().unwrap().push(request);
                let session_id = format!("native-{n}");
                let started = stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: session_id.clone(),
                    model: None,
                    tools: vec![],
                }]);
                if n == 0 {
                    // 首个 handoff 轮：SessionStarted → fatal → 停在 EOF 前。
                    let gate = self.eof_gate.clone();
                    Ok(Box::pin(
                        started
                            .chain(stream::iter(vec![RuntimeEvent::Error {
                                message: "fatal first handoff".into(),
                                recoverable: false,
                            }]))
                            .chain(stream::once(async move {
                                gate.notified().await;
                                RuntimeEvent::Completed {
                                    session_id,
                                    result: None,
                                    usage: None,
                                }
                            })),
                    ))
                } else {
                    Ok(Box::pin(started.chain(stream::iter(vec![
                        RuntimeEvent::Completed {
                            session_id,
                            result: Some("recovered".into()),
                            usage: None,
                        },
                    ]))))
                }
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

        let eof_gate = Arc::new(tokio::sync::Notify::new());
        let target = Arc::new(GatedFatalNativeEngine {
            turn: AtomicUsize::new(0),
            eof_gate: eof_gate.clone(),
            requests: Mutex::new(Vec::new()),
        });
        let hub = HubRuntime::new();
        hub.register_engine("source", Arc::new(FakeEngine::new()))
            .unwrap();
        hub.register_engine("target", target.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("source", RuntimeRequest::new("start")))
            .unwrap()
            .collect()
            .await;
        let pid = first[0].product_session_id.clone();
        hub.switch_engine(&pid, "target").unwrap();

        // 首个 handoff 轮：fatal 之后、EOF 之前停住。
        let mut failed_stream = hub
            .execute(
                HubRequest::new("target", RuntimeRequest::new("first attempt"))
                    .resume(pid.clone())
                    .with_handoff_context("R93_UNIQUE_MARKER"),
            )
            .unwrap();
        let mut saw_fatal = false;
        while let Some(event) = failed_stream.next().await {
            if matches!(
                event.event,
                RuntimeEvent::Error {
                    recoverable: false,
                    ..
                }
            ) {
                saw_fatal = true;
                break;
            }
        }
        assert!(saw_fatal);
        let between = hub.session(&pid).unwrap();
        assert_eq!(between.status, HubSessionStatus::Failed);
        assert!(between.pending_handoff);
        assert!(
            between.engine_session_id.is_none(),
            "the failed handoff turn's native id must be released before a retry can claim it"
        );

        // EOF 未到达时的重试：完整请求必须 resume=None、banner 恰好一次。
        let retried: Vec<_> = hub
            .execute(
                HubRequest::new("target", RuntimeRequest::new("second attempt"))
                    .resume(pid.clone()),
            )
            .unwrap()
            .collect()
            .await;
        assert!(matches!(
            retried.last().unwrap().event,
            RuntimeEvent::Completed { .. }
        ));
        {
            let requests = target.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(
                requests[1].resume_session.is_none(),
                "the failed first-handoff native id must not be resumed: {:?}",
                requests[1].resume_session
            );
            assert_eq!(
                requests[1].prompt.matches("R93_UNIQUE_MARKER").count(),
                1,
                "the retry must carry the banner exactly once: {}",
                requests[1].prompt
            );
            assert!(requests[1].prompt.contains("second attempt"));
        }
        let after_retry = hub.session(&pid).unwrap();
        assert_eq!(after_retry.status, HubSessionStatus::Completed);
        assert!(!after_retry.pending_handoff);
        assert_eq!(after_retry.engine_session_id.as_deref(), Some("native-1"));

        // 释放旧流：其尾部恢复按 claim 校验跳过，不覆写新轮。
        eof_gate.notify_one();
        while let Some(_event) = failed_stream.next().await {}
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let final_state = hub.session(&pid).unwrap();
        assert_eq!(
            final_state.status,
            HubSessionStatus::Completed,
            "the stale stream's tail must not overwrite the retry turn"
        );
        assert_eq!(
            final_state.engine_session_id.as_deref(),
            Some("native-1"),
            "the stale stream's tail must not restore the failed native id over the retry's"
        );
        assert!(!final_state.pending_handoff);
        assert_eq!(target.requests.lock().unwrap().len(), 2);
    }

    /// R97: 无显式 Error 的 `SessionStarted → EOF`（合成 fatal 路径）在
    /// handoff 轮上必须先于合成终态可见就完成身份恢复。旧序把
    /// mark_failed（发布 Failed、保留 native id）与 restore_failed_handoff
    /// 分成两个临界区，中间夹着合成终态的发送：消费者在 Error 可见后立即
    /// 认领下一轮时，会 resume 仍存储的失败 native id，而迟到的恢复又因
    /// claim_seq 更新被跳过。现在同一临界区（settle_synthetic_failure）
    /// 先恢复身份再发布终态：Failed 可观察的那一刻 resume 身份必为 None，
    /// 重试拿到 banner 恰好一次，新轮正常落定。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn synthetic_fatal_eof_handoff_settles_identity_before_the_retry_window() {
        struct EofHandoffEngine {
            turn: AtomicUsize,
            requests: Mutex<Vec<RuntimeRequest>>,
        }
        #[async_trait]
        impl EngineRuntime for EofHandoffEngine {
            fn execute(
                &self,
                request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                let n = self.turn.fetch_add(1, Ordering::SeqCst);
                self.requests.lock().unwrap().push(request);
                let session_id = format!("native-{n}");
                let started = stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: session_id.clone(),
                    model: None,
                    tools: vec![],
                }]);
                if n == 0 {
                    // 首个 handoff 轮：SessionStarted 后直接 EOF——无
                    // Completed、无显式 Error，走合成终态路径。
                    Ok(Box::pin(started))
                } else {
                    Ok(Box::pin(started.chain(stream::iter(vec![
                        RuntimeEvent::Completed {
                            session_id,
                            result: Some("recovered".into()),
                            usage: None,
                        },
                    ]))))
                }
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

        let target = Arc::new(EofHandoffEngine {
            turn: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        });
        let hub = HubRuntime::new();
        hub.register_engine("source", Arc::new(FakeEngine::new()))
            .unwrap();
        hub.register_engine("target", target.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("source", RuntimeRequest::new("start")))
            .unwrap()
            .collect()
            .await;
        let pid = first[0].product_session_id.clone();
        hub.switch_engine(&pid, "target").unwrap();

        // 首个 handoff 轮：合成 fatal 路径，一次性收满事件流。
        let failed: Vec<_> = hub
            .execute(
                HubRequest::new("target", RuntimeRequest::new("first attempt"))
                    .resume(pid.clone())
                    .with_handoff_context("R97_UNIQUE_MARKER"),
            )
            .unwrap()
            .collect()
            .await;
        assert!(
            failed.iter().any(|event| matches!(
                &event.event,
                RuntimeEvent::Error {
                    message,
                    recoverable: false,
                } if message == SYNTHETIC_TERMINAL_MESSAGE
            )),
            "a SessionStarted→EOF stream must surface the synthetic fatal: {failed:#?}"
        );
        // R93 投影不变：合成终态携带本流观察到的 native id，且不带结果。
        assert!(
            matches!(
                failed.last().map(|hub_event| &hub_event.event),
                Some(RuntimeEvent::Completed {
                    session_id,
                    result: None,
                    ..
                }) if session_id == "native-0"
            ),
            "the synthetic terminal keeps the stream-observed native id: {failed:#?}"
        );
        // Failed 可观察的那一刻，身份必须已经恢复（同一临界区）：
        let settled = hub.session(&pid).unwrap();
        assert_eq!(settled.status, HubSessionStatus::Failed);
        assert!(
            settled.engine_session_id.is_none(),
            "the failed native id must not remain stored once Failed is observable (R97)"
        );
        assert!(settled.pending_handoff, "the banner stays pending for the retry");

        // 紧随其后的重试：resume=None、banner 恰好一次、正常落定。
        let retried: Vec<_> = hub
            .execute(
                HubRequest::new("target", RuntimeRequest::new("second attempt"))
                    .resume(pid.clone()),
            )
            .unwrap()
            .collect()
            .await;
        assert!(matches!(
            retried.last().unwrap().event,
            RuntimeEvent::Completed { .. }
        ));
        {
            let requests = target.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(
                requests[1].resume_session.is_none(),
                "the synthetic-fatal handoff native id must not be resumed: {:?}",
                requests[1].resume_session
            );
            assert_eq!(
                requests[1].prompt.matches("R97_UNIQUE_MARKER").count(),
                1,
                "the retry must carry the banner exactly once: {}",
                requests[1].prompt
            );
        }
        let after_retry = hub.session(&pid).unwrap();
        assert_eq!(after_retry.status, HubSessionStatus::Completed);
        assert!(!after_retry.pending_handoff);
        assert_eq!(after_retry.engine_session_id.as_deref(), Some("native-1"));
    }

    /// R93: EOF 已到达的失败 handoff 重试同样必须 resume=None——与窗口内
    /// 时序保持同一身份契约。合成终态对仍携带失败轮真实启动过的 native
    /// id（投影不变），但会话表身份恢复为 None。
    #[tokio::test]
    async fn retry_after_failed_handoff_eof_resumes_no_native_identity() {
        struct FatalAfterStartEngine {
            turn: AtomicUsize,
            requests: Mutex<Vec<RuntimeRequest>>,
        }
        #[async_trait]
        impl EngineRuntime for FatalAfterStartEngine {
            fn execute(
                &self,
                request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                let n = self.turn.fetch_add(1, Ordering::SeqCst);
                self.requests.lock().unwrap().push(request);
                let session_id = format!("native-{n}");
                if n == 0 {
                    // fatal 后直接 EOF（无 Completed）——走合成终态路径。
                    Ok(Box::pin(stream::iter(vec![
                        RuntimeEvent::SessionStarted {
                            session_id: session_id.clone(),
                            model: None,
                            tools: vec![],
                        },
                        RuntimeEvent::Error {
                            message: "fatal after start".into(),
                            recoverable: false,
                        },
                    ])))
                } else {
                    Ok(Box::pin(stream::iter(vec![
                        RuntimeEvent::SessionStarted {
                            session_id: session_id.clone(),
                            model: None,
                            tools: vec![],
                        },
                        RuntimeEvent::Completed {
                            session_id,
                            result: Some("recovered".into()),
                            usage: None,
                        },
                    ])))
                }
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

        let target = Arc::new(FatalAfterStartEngine {
            turn: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        });
        let hub = HubRuntime::new();
        hub.register_engine("source", Arc::new(FakeEngine::new()))
            .unwrap();
        hub.register_engine("target", target.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("source", RuntimeRequest::new("start")))
            .unwrap()
            .collect()
            .await;
        let pid = first[0].product_session_id.clone();
        hub.switch_engine(&pid, "target").unwrap();

        let failed: Vec<_> = hub
            .execute(
                HubRequest::new("target", RuntimeRequest::new("first attempt"))
                    .resume(pid.clone())
                    .with_handoff_context("R93_EOF_MARKER"),
            )
            .unwrap()
            .collect()
            .await;
        assert!(matches!(
            failed[1].event,
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        // 合成终态对收尾；失败轮启动过的 native id 保留在投影里。
        assert!(matches!(
            failed.last().unwrap().event,
            RuntimeEvent::Completed {
                ref session_id,
                result: None,
                ..
            } if session_id == "native-0"
        ));
        let after_failure = hub.session(&pid).unwrap();
        assert_eq!(after_failure.status, HubSessionStatus::Failed);
        assert!(after_failure.pending_handoff);
        assert!(after_failure.engine_session_id.is_none());

        let retried: Vec<_> = hub
            .execute(
                HubRequest::new("target", RuntimeRequest::new("second attempt"))
                    .resume(pid.clone()),
            )
            .unwrap()
            .collect()
            .await;
        assert!(matches!(
            retried.last().unwrap().event,
            RuntimeEvent::Completed { .. }
        ));
        let requests = target.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].resume_session.is_none());
        assert_eq!(
            requests[1].prompt.matches("R93_EOF_MARKER").count(),
            1,
            "the post-EOF retry must carry the banner exactly once: {}",
            requests[1].prompt
        );
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

    /// ticket 15: a fifth engine can register with an EngineDescriptor, and
    /// products can ask it (by declared plan, not by name) whether it can
    /// satisfy a permission profile.
    #[test]
    fn descriptor_declares_permission_plans_without_engine_name() {
        use alva_engine_runtime::{
            ConfigSurface, InstallMetadata, PermissionPlanDecl, PermissionRelation,
            ProviderProtocol,
        };

        let descriptor = EngineDescriptor {
            runtime: Arc::new(FakeEngine::new()),
            inspector: None,
            permission_plans: vec![PermissionPlanDecl {
                profile_id: "workspace-read".into(),
                relation: PermissionRelation::Equivalent,
                summary: "read-only FUSE sandbox".into(),
            }],
            config_surface: ConfigSurface {
                supports_max_turns: true,
                provider_protocol: ProviderProtocol::Native,
                supports_fast_mode: false,
                supports_custom_http_headers: false,
            },
            install_metadata: InstallMetadata {
                required_binary: Some("gemini".into()),
                install_hint: Some("brew install gemini-cli".into()),
            },
        };
        let hub = HubRuntime::new();
        hub.register_engine_descriptor("gemini", descriptor)
            .unwrap();

        let queried = EngineDescriptor::from_runtime(Arc::new(FakeEngine::new()));
        assert!(
            hub.engine_ids().iter().any(|id| id.as_str() == "gemini"),
            "fifth engine must register at the type level"
        );
        assert!(queried.permission_plans.is_empty());
    }

    /// ticket 15 batch 3: a product reads back the registered descriptor and
    /// decides by declaration (max_turns support, permission profile), never
    /// by engine name.
    #[test]
    fn descriptor_is_queryable_and_decides_without_engine_names() {
        use alva_engine_runtime::{
            ConfigSurface, InstallMetadata, PermissionPlanDecl, PermissionRelation,
            ProviderProtocol,
        };
        let descriptor = EngineDescriptor {
            runtime: Arc::new(NoResumeEngine {
                requests: Mutex::new(Vec::new()),
            }),
            inspector: None,
            permission_plans: vec![PermissionPlanDecl {
                profile_id: "workspace_read".into(),
                relation: PermissionRelation::Equivalent,
                summary: "read-only sandbox".into(),
            }],
            config_surface: ConfigSurface {
                supports_max_turns: false,
                provider_protocol: ProviderProtocol::Native,
                supports_fast_mode: false,
                supports_custom_http_headers: false,
            },
            install_metadata: InstallMetadata {
                required_binary: Some("gemini".into()),
                install_hint: None,
            },
        };
        let hub = HubRuntime::new();
        hub.register_engine_descriptor("gemini", descriptor.clone())
            .unwrap();

        let stored = hub
            .descriptor(&EngineId::from("gemini"))
            .expect("registry must retain the full descriptor");
        // The decision a CLI/product used to make by matching "codex"/"gemini":
        // → does it honor --max-turns? → does it satisfy workspace_read?
        assert!(!stored.config_surface.supports_max_turns);
        assert!(stored.can_satisfy("workspace_read"));
        assert!(!stored.can_satisfy("workspace_write"));
        assert_eq!(
            stored.install_metadata.required_binary.as_deref(),
            Some("gemini")
        );
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

    /// Dies mid-tool: leaves a ToolStart open and then the stream ends without
    /// a terminal pair — the reproduction for ticket 12.
    struct MidrunDeathEngine;

    #[async_trait]
    impl EngineRuntime for MidrunDeathEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "die-1".into(),
                    model: None,
                    tools: vec![],
                },
                RuntimeEvent::ToolStart {
                    id: "t-open".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
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
                tool_control: true,
                permission_callback: false,
                resume: true,
                cancel: true,
            }
        }
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
        hub.register_engine("pending", Arc::new(PendingEngine))
            .unwrap();
        let product_id = ProductSessionId::new();
        // A live claim (Starting) is the honest way to have a running session;
        // restore_session no longer accepts non-terminal states.
        let _first = hub
            .execute(
                HubRequest::new("pending", RuntimeRequest::new("first")).resume(product_id.clone()),
            )
            .unwrap();

        let error = hub
            .switch_engine(&product_id, "pending")
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

    // ticket 08: abandon must release a stuck claim so the session can be
    // reclaimed by a later execute, and must stop the abandoned spawned turn.
    #[tokio::test]
    async fn abandoned_session_can_be_reclaimed() {
        let hub = HubRuntime::new();
        hub.register_engine("pending", Arc::new(PendingEngine))
            .unwrap();
        let product_id = ProductSessionId::new();

        let _first = hub
            .execute(
                HubRequest::new("pending", RuntimeRequest::new("first")).resume(product_id.clone()),
            )
            .unwrap();
        assert_eq!(
            hub.session(&product_id).unwrap().status,
            HubSessionStatus::Starting
        );
        // Before abandon: busy.
        assert!(matches!(
            hub.execute(
                HubRequest::new("pending", RuntimeRequest::new("second"))
                    .resume(product_id.clone())
            ),
            Err(HubError::SessionBusy(_))
        ));

        hub.abandon(&product_id).unwrap();
        assert_eq!(
            hub.session(&product_id).unwrap().status,
            HubSessionStatus::Failed
        );
        // After abandon: reclaimable.
        assert!(hub
            .execute(HubRequest::new("pending", RuntimeRequest::new("third")).resume(product_id))
            .is_ok());
    }

    #[tokio::test]
    async fn abandon_stops_the_spawned_turn() {
        let hub = HubRuntime::new();
        hub.register_engine("pending", Arc::new(PendingEngine))
            .unwrap();
        let product_id = ProductSessionId::new();

        let _stream = hub
            .execute(
                HubRequest::new("pending", RuntimeRequest::new("first")).resume(product_id.clone()),
            )
            .unwrap();
        hub.abandon(&product_id).unwrap();

        // The spawned task must terminate within a bounded time instead of
        // hanging on stream::pending() forever. Give it a moment to unwind.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    /// R63: 旧轮 cancel 的迟到结果只释放旧代际。gate 固定交错：旧 cancel
    /// 已进入引擎 → 旧轮 Completed → 同 pid 新轮 Started 认领 → 旧 cancel
    /// 返回 Unsupported。新轮必须仍 Running、句柄仍可达，且对新轮的真实
    /// cancel 仍能落地（Failed + 无 live run）。
    #[tokio::test(flavor = "current_thread")]
    async fn stale_cancel_result_must_not_abort_new_generation() {
        struct GatedEngine {
            turn: AtomicUsize,
            complete: Arc<tokio::sync::Notify>,
            cancel_enter: Arc<tokio::sync::Notify>,
            cancel_release: Arc<tokio::sync::Notify>,
        }
        #[async_trait]
        impl EngineRuntime for GatedEngine {
            fn execute(
                &self,
                _request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                let n = self.turn.fetch_add(1, Ordering::SeqCst);
                let session_id = format!("turn-{n}");
                let started = stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: session_id.clone(),
                    model: None,
                    tools: vec![],
                }]);
                if n == 0 {
                    let gate = self.complete.clone();
                    Ok(Box::pin(started.chain(stream::once(async move {
                        gate.notified().await;
                        RuntimeEvent::Completed {
                            session_id,
                            result: None,
                            usage: None,
                        }
                    }))))
                } else {
                    Ok(Box::pin(
                        started.chain(stream::pending::<RuntimeEvent>()),
                    ))
                }
            }

            async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
                self.cancel_enter.notify_one();
                self.cancel_release.notified().await;
                Err(RuntimeError::Unsupported("cancel".into()))
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

        let engine = Arc::new(GatedEngine {
            turn: AtomicUsize::new(0),
            complete: Arc::new(tokio::sync::Notify::new()),
            cancel_enter: Arc::new(tokio::sync::Notify::new()),
            cancel_release: Arc::new(tokio::sync::Notify::new()),
        });
        let hub = HubRuntime::new();
        hub.register_engine("probe", engine.clone()).unwrap();

        let mut first = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("one")))
            .unwrap();
        let pid = first.next().await.unwrap().product_session_id;

        // 旧轮 cancel 已进入引擎、停在 release gate。
        let cancel_task = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::spawn(async move { hub.cancel(&pid).await })
        };
        engine.cancel_enter.notified().await;

        // 旧轮正常结束。
        engine.complete.notify_one();
        assert!(matches!(
            first.next().await.unwrap().event,
            RuntimeEvent::Completed { .. }
        ));

        // 同 pid 新轮认领并 Started。
        let mut second = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("two")).resume(pid.clone()))
            .unwrap();
        let started = second.next().await.unwrap();
        assert!(matches!(started.event, RuntimeEvent::SessionStarted { .. }));
        assert_eq!(hub.session(&pid).unwrap().status, HubSessionStatus::Running);

        // 旧 cancel 返回 Unsupported——不得触碰新轮。
        engine.cancel_release.notify_one();
        cancel_task.await.unwrap().unwrap();
        assert_eq!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Running,
            "stale cancel result must not abandon the new generation"
        );
        assert!(hub.has_live_run(&pid), "new generation handle must survive");

        // 新轮自身可取消：再次 cancel 走到引擎并返回 Unsupported 时，
        // 目标代际就是当前认领者，abandon 正常落地。
        let second_cancel = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::spawn(async move { hub.cancel(&pid).await })
        };
        engine.cancel_enter.notified().await;
        engine.cancel_release.notify_one();
        second_cancel.await.unwrap().unwrap();
        assert_eq!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Failed,
            "a real cancel of the current generation must still release"
        );
        assert!(!hub.has_live_run(&pid));
    }

    /// R92: 旧轮 fatal 后流未 EOF、旧 RunHandle 仍在 runs；新 execute 已
    /// 递增 claim_seq 并置 Starting，但同步 `engine.execute` 尚未返回、
    /// 新 RunHandle 未登记。此时旧 cancel 返回 Unsupported：只比较 runs
    /// 里仍是旧条目的 generation 会误判"自己仍是当前认领者"，进而把
    /// 新轮 Starting 置 Failed、取消其 token。释放目标必须绑定该登记
    /// 对应的 claim_seq，且校验与释放同处一个 sessions 写锁事务。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_cancel_in_claim_registration_gap_must_not_kill_new_turn() {
        struct ClaimGapEngine {
            turn: AtomicUsize,
            cancel_enter: Arc<tokio::sync::Notify>,
            cancel_release: Arc<tokio::sync::Notify>,
            execute_entered: Arc<tokio::sync::Notify>,
            execute_gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
        }
        #[async_trait]
        impl EngineRuntime for ClaimGapEngine {
            fn execute(
                &self,
                _request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                let n = self.turn.fetch_add(1, Ordering::SeqCst);
                if n == 1 {
                    // 新轮：同步 execute 停在 OS 门上——已认领、未登记。
                    self.execute_entered.notify_one();
                    let mut released = self.execute_gate.0.lock().unwrap();
                    while !*released {
                        released = self.execute_gate.1.wait(released).unwrap();
                    }
                }
                let started = stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: format!("native-{n}"),
                    model: None,
                    tools: vec![],
                }]);
                if n == 0 {
                    // 旧轮：fatal 后流悬停不 EOF，旧 RunHandle 持续存活。
                    Ok(Box::pin(
                        started
                            .chain(stream::iter(vec![RuntimeEvent::Error {
                                message: "fatal before eof".into(),
                                recoverable: false,
                            }]))
                            .chain(stream::pending::<RuntimeEvent>()),
                    ))
                } else {
                    Ok(Box::pin(
                        started.chain(stream::pending::<RuntimeEvent>()),
                    ))
                }
            }

            async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
                self.cancel_enter.notify_one();
                self.cancel_release.notified().await;
                Err(RuntimeError::Unsupported("cancel".into()))
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

        let engine = Arc::new(ClaimGapEngine {
            turn: AtomicUsize::new(0),
            cancel_enter: Arc::new(tokio::sync::Notify::new()),
            cancel_release: Arc::new(tokio::sync::Notify::new()),
            execute_entered: Arc::new(tokio::sync::Notify::new()),
            execute_gate: Arc::new((Mutex::new(false), std::sync::Condvar::new())),
        });
        let hub = HubRuntime::new();
        hub.register_engine("probe", engine.clone()).unwrap();

        // 旧轮：fatal 已置 Failed，但流悬停，RunHandle 仍在。
        let mut first = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("first")))
            .unwrap();
        let pid = first.next().await.unwrap().product_session_id;
        assert!(matches!(
            first.next().await.unwrap().event,
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        assert_eq!(hub.session(&pid).unwrap().status, HubSessionStatus::Failed);

        // 旧 cancel 进入引擎并停在 release gate。
        let old_cancel = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::spawn(async move { hub.cancel(&pid).await })
        };
        engine.cancel_enter.notified().await;

        // 新轮同步 execute 认领后停在 claim→登记窗口。
        let new_execute = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::task::spawn_blocking(move || {
                hub.execute(HubRequest::new("probe", RuntimeRequest::new("second")).resume(pid))
            })
        };
        engine.execute_entered.notified().await;
        assert_eq!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Starting
        );

        // 旧 cancel 返回 Unsupported：claim 已换人，不得释放/伤害新轮。
        engine.cancel_release.notify_one();
        old_cancel.await.unwrap().unwrap();
        assert_eq!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Starting,
            "old RunGeneration must not release a newer session claim before its RunHandle registers"
        );

        // 新轮正常登记、Started，且随后可被自己的 cancel 正常释放。
        release_gate(&engine.execute_gate);
        let mut second = new_execute.await.unwrap().unwrap();
        assert!(matches!(
            second.next().await.unwrap().event,
            RuntimeEvent::SessionStarted { .. }
        ));
        assert!(hub.has_live_run(&pid), "new turn must register once its synchronous start returns");
        let second_cancel = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::spawn(async move { hub.cancel(&pid).await })
        };
        engine.cancel_enter.notified().await;
        engine.cancel_release.notify_one();
        second_cancel.await.unwrap().unwrap();
        assert_eq!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Failed,
            "the registered new turn must remain cancelable"
        );
        assert!(!hub.has_live_run(&pid));
    }

    /// R63: 回退路径（无 live run）的迟到 cancel 结果按 claim_seq 校验。
    /// 恢复的会话 cancel 停在引擎 gate 内时，新 execute 已递增 claim_seq
    /// 并认领；旧 Unsupported 返回不得释放新 claim。
    #[tokio::test(flavor = "current_thread")]
    async fn stale_fallback_cancel_must_not_release_new_claim() {
        struct GatedPendingEngine {
            cancel_enter: Arc<tokio::sync::Notify>,
            cancel_release: Arc<tokio::sync::Notify>,
        }
        #[async_trait]
        impl EngineRuntime for GatedPendingEngine {
            fn execute(
                &self,
                _request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                Ok(Box::pin(stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: "native-fallback".into(),
                    model: None,
                    tools: vec![],
                }])
                .chain(stream::pending::<RuntimeEvent>())))
            }

            async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
                self.cancel_enter.notify_one();
                self.cancel_release.notified().await;
                Err(RuntimeError::Unsupported("cancel".into()))
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

        let engine = Arc::new(GatedPendingEngine {
            cancel_enter: Arc::new(tokio::sync::Notify::new()),
            cancel_release: Arc::new(tokio::sync::Notify::new()),
        });
        let hub = HubRuntime::new();
        hub.register_engine("probe", engine.clone()).unwrap();

        // 恢复的会话：终态 + 已知 native id，无 live run。
        let pid = ProductSessionId::new();
        hub.restore_session(pid.clone(), "probe", "native-0", HubSessionStatus::Completed)
            .unwrap();

        let cancel_task = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::spawn(async move { hub.cancel(&pid).await })
        };
        engine.cancel_enter.notified().await;

        // cancel await 期间新轮认领。
        let mut next = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("next")).resume(pid.clone()))
            .unwrap();
        assert!(matches!(
            next.next().await.unwrap().event,
            RuntimeEvent::SessionStarted { .. }
        ));

        // 旧 cancel 返回 Unsupported——claim_seq 已变化，不得释放。
        engine.cancel_release.notify_one();
        cancel_task.await.unwrap().unwrap();
        assert_eq!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Running,
            "stale fallback cancel must not release the new claim"
        );
        assert!(hub.has_live_run(&pid));
        drop(next); // 测试收尾：运行时关闭时回收 pending 流任务
    }

    /// R80: live 路径的能力 bypass 只释放原代际。旧 cancel 已 clone handle
    /// （generation 已固定）后停进同步的 capabilities() gate；期间旧轮被
    /// abandon、同 pid 新轮认领。gate 返回 cancel=false 时，无差别 abandon
    /// 会误杀新轮；目标化释放必须跳过。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capability_bypass_cancel_must_target_original_generation() {
        // 只有第一次 capabilities() 调用（cancel 自己的查询）会停在 gate；
        // 后续 resume-execute 的能力探针直接通过。
        struct GateCapEngine {
            turn: AtomicUsize,
            gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
            cap_enter: Arc<AtomicUsize>,
        }
        #[async_trait]
        impl EngineRuntime for GateCapEngine {
            fn execute(
                &self,
                _request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                let n = self.turn.fetch_add(1, Ordering::SeqCst);
                Ok(Box::pin(
                    stream::iter(vec![RuntimeEvent::SessionStarted {
                        session_id: format!("native-{n}"),
                        model: None,
                        tools: vec![],
                    }])
                    .chain(stream::pending::<RuntimeEvent>()),
                ))
            }

            async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
                Err(RuntimeError::Unsupported("cancel".into()))
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
                if self.cap_enter.fetch_add(1, Ordering::SeqCst) == 0 {
                    let mut released = self.gate.0.lock().unwrap();
                    while !*released {
                        released = self.gate.1.wait(released).unwrap();
                    }
                }
                RuntimeCapabilities {
                    streaming: true,
                    tool_control: false,
                    permission_callback: false,
                    resume: true,
                    cancel: false,
                }
            }
        }

        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let cap_enter = Arc::new(AtomicUsize::new(0));
        let engine = Arc::new(GateCapEngine {
            turn: AtomicUsize::new(0),
            gate: gate.clone(),
            cap_enter: cap_enter.clone(),
        });
        let hub = HubRuntime::new();
        hub.register_engine("probe", engine.clone()).unwrap();

        let mut first = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("one")))
            .unwrap();
        let pid = first.next().await.unwrap().product_session_id;
        assert_eq!(hub.session(&pid).unwrap().status, HubSessionStatus::Running);

        // 旧 cancel 已 clone handle（generation 固定）并停进 capabilities gate。
        let cancel_task = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::spawn(async move { hub.cancel(&pid).await })
        };
        let mut waited = 0;
        while cap_enter.load(Ordering::SeqCst) == 0 {
            waited += 1;
            assert!(waited <= 200, "cancel must reach the capability gate");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        // gate 未开时：旧轮被 abandon，同 pid 新轮认领并 Running。
        hub.abandon(&pid).unwrap();
        let mut second = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("two")).resume(pid.clone()))
            .unwrap();
        assert!(matches!(
            second.next().await.unwrap().event,
            RuntimeEvent::SessionStarted { .. }
        ));
        assert_eq!(hub.session(&pid).unwrap().status, HubSessionStatus::Running);

        // gate 返回 cancel=false：bypass 只允许释放旧代际。
        release_gate(&gate);
        cancel_task.await.unwrap().unwrap();
        assert_eq!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Running,
            "capability-bypass cancel must not abandon a newer generation"
        );
        assert!(hub.has_live_run(&pid), "new generation handle must survive");

        // 新轮自身的真实 cancel 仍落地（bypass 命中当前代际）。
        let second_cancel = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::spawn(async move { hub.cancel(&pid).await })
        };
        second_cancel.await.unwrap().unwrap();
        assert_eq!(hub.session(&pid).unwrap().status, HubSessionStatus::Failed);
        assert!(!hub.has_live_run(&pid));
        drop(first);
        drop(second);
    }

    /// R80: 回退路径的路由与 claim 必须来自同一会话快照。恢复会话的
    /// cancel 停在 capabilities gate 期间新 execute 已认领；cancel 走旧
    /// native id，Unsupported 只能释放采样到的旧 claim_seq——旧实现在
    /// gate 之后另采 claim_seq，释放的是新轮。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fallback_capability_gate_cancel_must_keep_original_claim() {
        struct GateCapFallbackEngine {
            gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
            cap_enter: Arc<AtomicUsize>,
            cancelled_ids: Arc<Mutex<Vec<String>>>,
        }
        #[async_trait]
        impl EngineRuntime for GateCapFallbackEngine {
            fn execute(
                &self,
                _request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                Ok(Box::pin(
                    stream::iter(vec![RuntimeEvent::SessionStarted {
                        session_id: "new-native".into(),
                        model: None,
                        tools: vec![],
                    }])
                    .chain(stream::pending::<RuntimeEvent>()),
                ))
            }

            async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
                self.cancelled_ids
                    .lock()
                    .expect("cancelled ids lock poisoned")
                    .push(session_id.to_string());
                Err(RuntimeError::Unsupported("cancel".into()))
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
                if self.cap_enter.fetch_add(1, Ordering::SeqCst) == 0 {
                    let mut released = self.gate.0.lock().unwrap();
                    while !*released {
                        released = self.gate.1.wait(released).unwrap();
                    }
                }
                RuntimeCapabilities {
                    streaming: true,
                    tool_control: false,
                    permission_callback: false,
                    resume: true,
                    cancel: true,
                }
            }
        }

        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let cap_enter = Arc::new(AtomicUsize::new(0));
        let cancelled_ids = Arc::new(Mutex::new(Vec::new()));
        let engine = Arc::new(GateCapFallbackEngine {
            gate: gate.clone(),
            cap_enter: cap_enter.clone(),
            cancelled_ids: cancelled_ids.clone(),
        });
        let hub = HubRuntime::new();
        hub.register_engine("probe", engine.clone()).unwrap();

        // 恢复的会话：终态 + 已知旧 native id，无 live run。
        let pid = ProductSessionId::new();
        hub.restore_session(pid.clone(), "probe", "native-old", HubSessionStatus::Completed)
            .unwrap();

        let cancel_task = {
            let hub = hub.clone();
            let pid = pid.clone();
            tokio::spawn(async move { hub.cancel(&pid).await })
        };
        let mut waited = 0;
        while cap_enter.load(Ordering::SeqCst) == 0 {
            waited += 1;
            assert!(waited <= 200, "cancel must reach the capability gate");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        // gate 未开时新轮已认领（claim_seq 递增、native id 换新）。
        let mut next = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("next")).resume(pid.clone()))
            .unwrap();
        assert!(matches!(
            next.next().await.unwrap().event,
            RuntimeEvent::SessionStarted { .. }
        ));
        assert!(hub.has_live_run(&pid));

        // gate 返回 cancel=true：cancel 走旧 native id 并返回 Unsupported。
        release_gate(&gate);
        cancel_task.await.unwrap().unwrap();
        assert_eq!(
            *cancelled_ids.lock().unwrap(),
            vec!["native-old".to_string()],
            "cancel must route to the snapshot's original native id"
        );
        assert_eq!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Running,
            "stale fallback cancel must not release the newer claim"
        );
        assert!(hub.has_live_run(&pid));
        drop(next); // 测试收尾：运行时关闭时回收 pending 流任务
    }

    /// R64: 同步启动 gate。turn 0 的 execute 阻塞在 OS 门上（模拟已 claim
    /// 但尚未返回/登记的旧轮），释放后按 `mode` 返回 pending 流或 Err。
    struct GatedSyncExecuteEngine {
        turn: AtomicUsize,
        entered: Arc<tokio::sync::Notify>,
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
        fail_first: bool,
    }
    #[async_trait]
    impl EngineRuntime for GatedSyncExecuteEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            let n = self.turn.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                self.entered.notify_one();
                let mut released = self.gate.0.lock().unwrap();
                while !*released {
                    released = self.gate.1.wait(released).unwrap();
                }
                if self.fail_first {
                    return Err(RuntimeError::Unsupported("gated sync start".into()));
                }
                return Ok(Box::pin(stream::pending::<RuntimeEvent>()));
            }
            Ok(Box::pin(
                stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: "new-native".into(),
                    model: None,
                    tools: vec![],
                }])
                .chain(stream::pending::<RuntimeEvent>()),
            ))
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

    fn release_gate(gate: &(Mutex<bool>, std::sync::Condvar)) {
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
    }

    /// R64: 已被 abandon 的旧 execute 迟到返回时，其登记不得覆盖新轮的
    /// RunHandle。旧 execute 阻塞在同步 engine.execute → abandon → 新轮
    /// Started 并登记 → 释放旧 execute → 登记被跳过，新代际句柄仍可达。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn late_registration_after_abandon_does_not_overwrite_new_run() {
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let engine = Arc::new(GatedSyncExecuteEngine {
            turn: AtomicUsize::new(0),
            entered: Arc::new(tokio::sync::Notify::new()),
            gate: gate.clone(),
            fail_first: false,
        });
        let hub = Arc::new(HubRuntime::new());
        hub.register_engine("probe", engine.clone()).unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        {
            let hub = hub.clone();
            let handle = tokio::runtime::Handle::current();
            std::thread::spawn(move || {
                let _enter = handle.enter();
                tx.send(
                    hub.execute(HubRequest::new("probe", RuntimeRequest::new("old")))
                        .unwrap(),
                )
                .unwrap();
            });
        }
        engine.entered.notified().await;

        let pid = hub.sessions()[0].product_session_id.clone();
        hub.abandon(&pid).unwrap();

        let mut second = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("new")).resume(pid.clone()))
            .unwrap();
        assert!(matches!(
            second.next().await.unwrap().event,
            RuntimeEvent::SessionStarted { .. }
        ));
        assert!(hub.has_live_run(&pid));
        let new_generation = hub
            .runs
            .read()
            .expect("Hub runs lock poisoned")
            .get(&pid)
            .unwrap()
            .generation;

        // 旧 execute 迟到返回并尝试登记。
        release_gate(&gate);
        rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        // 有界轮询给旧登记尝试留出执行窗口（负向断言：登记必须被跳过）。
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let current = hub
                .runs
                .read()
                .expect("Hub runs lock poisoned")
                .get(&pid)
                .unwrap()
                .generation;
            assert_eq!(
                current, new_generation,
                "late abandoned registration must not overwrite the new run handle"
            );
            assert!(hub.has_live_run(&pid));
        }
        assert_eq!(hub.session(&pid).unwrap().status, HubSessionStatus::Running);
    }

    /// R64: 旧 execute 的同步启动 Err 回滚在 claim 已被取代时不得触碰
    /// 新轮——旧快照（Completed/old-native）不得覆写新轮的 Running。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn late_sync_execute_err_rollback_does_not_clobber_new_turn() {
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let engine = Arc::new(GatedSyncExecuteEngine {
            turn: AtomicUsize::new(0),
            entered: Arc::new(tokio::sync::Notify::new()),
            gate: gate.clone(),
            fail_first: true,
        });
        let hub = Arc::new(HubRuntime::new());
        hub.register_engine("probe", engine.clone()).unwrap();

        let pid = ProductSessionId::new();
        hub.restore_session(pid.clone(), "probe", "old-native", HubSessionStatus::Completed)
            .unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        {
            let hub = hub.clone();
            let pid = pid.clone();
            let handle = tokio::runtime::Handle::current();
            std::thread::spawn(move || {
                let _enter = handle.enter();
                let result = hub.execute(
                    HubRequest::new("probe", RuntimeRequest::new("old")).resume(pid),
                );
                tx.send(result.is_err()).unwrap();
            });
        }
        engine.entered.notified().await;
        hub.abandon(&pid).unwrap();

        let mut second = hub
            .execute(HubRequest::new("probe", RuntimeRequest::new("new")).resume(pid.clone()))
            .unwrap();
        assert!(matches!(
            second.next().await.unwrap().event,
            RuntimeEvent::SessionStarted { .. }
        ));

        // 旧 execute 拿到 Err，迟到回滚。
        release_gate(&gate);
        assert!(rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap());
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            let session = hub.session(&pid).unwrap();
            assert_eq!(
                session.status,
                HubSessionStatus::Running,
                "stale rollback must not overwrite the new turn with the old snapshot"
            );
            assert_eq!(
                session.engine_session_id.as_deref(),
                Some("new-native"),
                "stale rollback must not restore the old native id"
            );
        }
        assert!(hub.has_live_run(&pid));
    }

    #[tokio::test]
    async fn cancel_releases_claim_when_engine_session_is_unknown() {
        let hub = HubRuntime::new();
        hub.register_engine("pending", Arc::new(PendingEngine))
            .unwrap();
        let product_id = ProductSessionId::new();
        let _first = hub
            .execute(
                HubRequest::new("pending", RuntimeRequest::new("first")).resume(product_id.clone()),
            )
            .unwrap();
        // stream::pending() never emits SessionStarted, so engine_session_id is
        // None; cancel must release the claim rather than return SessionNotReady.
        hub.cancel(&product_id).await.unwrap();
        assert!(hub
            .execute(HubRequest::new("pending", RuntimeRequest::new("second")).resume(product_id))
            .is_ok());
    }

    // ticket 09: restore_session must refuse non-terminal states.
    #[tokio::test]
    async fn restore_session_rejects_non_terminal_status() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        let err = hub
            .restore_session(
                ProductSessionId::from_existing("p-nonterm"),
                "alva",
                "e1",
                HubSessionStatus::Running,
            )
            .unwrap_err();
        assert!(matches!(err, HubError::SessionNotReady(_)));
    }

    // ticket 07: a Completed turn must not be re-written to Failed by a late
    // fatal Error (the violation is still counted on the snapshot).
    #[tokio::test]
    async fn fatal_error_after_completed_keeps_completed() {
        let (product_session_id, sessions) = session_registry();
        update_session(
            &sessions,
            &product_session_id,
            0,
            &RuntimeEvent::Completed {
                session_id: "e1".into(),
                result: Some("done".into()),
                usage: None,
            },
                    false,
        );
        update_session(
            &sessions,
            &product_session_id,
            0,
            &RuntimeEvent::Error {
                message: "late fatal after completed".into(),
                recoverable: false,
            },
            false,
        );
        let session = &sessions.read().unwrap()[&product_session_id];
        assert_eq!(session.status, HubSessionStatus::Completed);
    }

    /// Fatal first handoff turn whose trailing event lands after a delay, so
    /// the consumer can claim the next turn while the first turn's tail is
    /// still unwinding — the ticket-07 race.
    struct DelayedFatalHandoffEngine {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl EngineRuntime for DelayedFatalHandoffEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            let nth = self.calls.fetch_add(1, Ordering::SeqCst);
            if nth == 0 {
                // Fatal + Completed immediately, then a trailing event after a
                // short sleep so the spawned tail cannot finish before a newer
                // claim lands.
                let stream = stream::iter(vec![
                    RuntimeEvent::Error {
                        message: "target failed after startup".into(),
                        recoverable: false,
                    },
                    RuntimeEvent::Completed {
                        session_id: String::new(),
                        result: None,
                        usage: None,
                    },
                ])
                .chain(stream::once(async {
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    RuntimeEvent::Error {
                        message: "late tail event".into(),
                        recoverable: true,
                    }
                }));
                Ok(Box::pin(stream))
            } else {
                // Subsequent claim stays running so a third execute must be busy.
                Ok(Box::pin(stream::pending()))
            }
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stale_turn_tail_does_not_release_next_turn_claim() {
        for _ in 0..30 {
            let hub = HubRuntime::new();
            hub.register_engine("a", Arc::new(FakeEngine::new()))
                .unwrap();
            let wrapped = Arc::new(DelayedFatalHandoffEngine {
                calls: AtomicUsize::new(0),
            });
            hub.register_engine("b", wrapped.clone()).unwrap();

            let first: Vec<_> = hub
                .execute(HubRequest::new("a", RuntimeRequest::new("start")))
                .unwrap()
                .collect()
                .await;
            let pid = first[0].product_session_id.clone();
            hub.switch_engine(&pid, "b").unwrap();

            // Turn A on b: fatal handoff stream. Consumer reads until Completed
            // then immediately reclaims the same product session (turn B).
            let stream_a = hub
                .execute(
                    HubRequest::new("b", RuntimeRequest::new("t1"))
                        .resume(pid.clone())
                        .with_handoff_context("C"),
                )
                .unwrap();
            let mut stream_a = Box::pin(stream_a);
            while let Some(ev) = stream_a.next().await {
                if matches!(ev.event, RuntimeEvent::Completed { .. }) {
                    break;
                }
            }
            drop(stream_a);

            // Turn B claims now, while A's tail is still in flight.
            assert!(hub
                .execute(HubRequest::new("b", RuntimeRequest::new("t2")).resume(pid.clone()))
                .is_ok());

            // Let A's tail land; the seq guard must keep B's claim intact.
            tokio::time::sleep(std::time::Duration::from_millis(90)).await;

            let claim_c =
                hub.execute(HubRequest::new("b", RuntimeRequest::new("t3")).resume(pid.clone()));
            assert!(
                matches!(claim_c, Err(HubError::SessionBusy(_))),
                "A's stale rollback must not release B's claim"
            );
        }
    }

    // ticket 14 D-2: an engine that cannot resume must start a fresh native
    // session on its second turn instead of failing with ResumeUnsupported.
    struct NoResumeEngine {
        requests: Mutex<Vec<RuntimeRequest>>,
    }

    #[async_trait]
    impl EngineRuntime for NoResumeEngine {
        fn execute(
            &self,
            request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.requests.lock().unwrap().push(request);
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "stateless-1".into(),
                    model: None,
                    tools: vec![],
                },
                RuntimeEvent::Completed {
                    session_id: "stateless-1".into(),
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
                resume: false,
                cancel: true,
            }
        }
    }

    #[tokio::test]
    async fn stateless_engine_starts_a_fresh_native_session_instead_of_failing() {
        let hub = HubRuntime::new();
        let engine = Arc::new(NoResumeEngine {
            requests: Mutex::new(Vec::new()),
        });
        hub.register_engine("stateless", engine.clone()).unwrap();

        let first: Vec<_> = hub
            .execute(HubRequest::new("stateless", RuntimeRequest::new("first")))
            .unwrap()
            .collect()
            .await;
        let pid = first[0].product_session_id.clone();
        assert_eq!(
            hub.session(&pid).unwrap().engine_session_id.as_deref(),
            Some("stateless-1")
        );

        // Second turn: engine has resume=false, so hub must NOT fail and must
        // start fresh (no resume_session filled).
        let second = hub
            .execute(
                HubRequest::new("stateless", RuntimeRequest::new("second")).resume(pid.clone()),
            )
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        assert!(!second.is_empty(), "second turn must succeed");
        let requests = engine.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1].resume_session.is_none(),
            "stateless engine must not be asked to resume: {:?}",
            requests[1]
        );
    }

    /// ticket 13: a run that stays alive and has a native id; cancel records
    /// the call so tests can assert the RunHandle routed it.
    struct LiveRunEngine {
        cancelled: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl EngineRuntime for LiveRunEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            let stream = stream::iter(vec![RuntimeEvent::SessionStarted {
                session_id: "live-1".into(),
                model: None,
                tools: vec![],
            }])
            .chain(stream::pending());
            Ok(Box::pin(stream))
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
    async fn cancel_reaches_a_running_run_after_engine_is_unregistered() {
        let hub = HubRuntime::new();
        let engine = Arc::new(LiveRunEngine {
            cancelled: Mutex::new(Vec::new()),
        });
        hub.register_engine("live", engine.clone()).unwrap();

        let mut stream = hub
            .execute(HubRequest::new("live", RuntimeRequest::new("hi")))
            .unwrap();
        let pid = stream.next().await.unwrap().product_session_id;
        assert!(hub.has_live_run(&pid));
        assert_eq!(
            hub.session(&pid).unwrap().engine_session_id.as_deref(),
            Some("live-1")
        );

        // Remove the registry entry while the run is still alive.
        assert!(hub.unregister_engine(&EngineId::from("live")));

        // cancel must reach the running engine through its RunHandle even
        // though the registry no longer has the entry.
        hub.cancel(&pid).await.unwrap();
        assert_eq!(
            engine.cancelled.lock().unwrap().as_slice(),
            &["live-1".to_string()],
            "cancel must be routed to the live run's engine"
        );
    }

    #[tokio::test]
    async fn abandon_aborts_the_stream_task() {
        let hub = HubRuntime::new();
        hub.register_engine("pending", Arc::new(PendingEngine))
            .unwrap();
        let product_id = ProductSessionId::new();
        let _stream = hub
            .execute(
                HubRequest::new("pending", RuntimeRequest::new("first")).resume(product_id.clone()),
            )
            .unwrap();
        assert!(hub.has_live_run(&product_id));

        hub.abandon(&product_id).unwrap();

        // The spawned turn must exit within a bounded time and release its
        // run entry (the token-cancelled loop finishes its body and removes
        // itself).
        tokio::time::timeout(std::time::Duration::from_millis(500), async {
            loop {
                if !hub.has_live_run(&product_id) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("abandon must stop the spawned turn and release its run entry");
    }

    // ticket 11: a resume turn that fails fatally before any SessionStarted must
    // drop the stored native id so the next turn starts fresh instead of
    // resuming the rejected id forever.
    struct ResumeRejectEngine;

    #[async_trait]
    impl EngineRuntime for ResumeRejectEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::Error {
                    message: "Alva session 'x' not found in memory or native record store".into(),
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

    #[tokio::test]
    async fn rejected_resume_clears_stored_engine_session_id() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(ResumeRejectEngine))
            .unwrap();
        let product_id = ProductSessionId::from_existing("p-resume");
        hub.restore_session(
            product_id.clone(),
            "alva",
            "dead-native",
            HubSessionStatus::Completed,
        )
        .unwrap();
        assert_eq!(
            hub.session(&product_id)
                .unwrap()
                .engine_session_id
                .as_deref(),
            Some("dead-native")
        );

        let _: Vec<_> = hub
            .execute(
                HubRequest::new("alva", RuntimeRequest::new("resume dead"))
                    .resume(product_id.clone()),
            )
            .unwrap()
            .collect()
            .await;

        assert_eq!(
            hub.session(&product_id).unwrap().engine_session_id,
            None,
            "rejected resume id must be cleared so the next turn starts fresh"
        );
        assert_eq!(
            hub.session(&product_id).unwrap().status,
            HubSessionStatus::Failed
        );
    }

    // ticket 12: an engine that dies with an open tool must have that tool
    // closed with a synthesized failing ToolEnd before the terminal pair.
    #[tokio::test]
    async fn unclosed_tool_is_synthesized_as_failing_toolend_on_midrun_death() {
        let hub = HubRuntime::new();
        hub.register_engine("die-mid-tool", Arc::new(MidrunDeathEngine))
            .unwrap();

        let events: Vec<_> = hub
            .execute(HubRequest::new(
                "die-mid-tool",
                RuntimeRequest::new("probe"),
            ))
            .unwrap()
            .collect()
            .await;

        // Forwarded SessionStarted + ToolStart, then synthesized failing
        // ToolEnd for the open tool, then terminal Error + Completed.
        assert_eq!(events.len(), 5);
        assert!(matches!(
            events[0].event,
            RuntimeEvent::SessionStarted { .. }
        ));
        assert!(matches!(events[1].event, RuntimeEvent::ToolStart { .. }));
        assert!(matches!(
            events[2].event,
            RuntimeEvent::ToolEnd { id: ref i, .. } if i == "t-open"
        ));
        if let RuntimeEvent::ToolEnd { result, .. } = &events[2].event {
            assert!(result.is_error, "synthesized ToolEnd must be an error");
        }
        assert!(matches!(
            events[3].event,
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        assert!(matches!(
            events[4].event,
            RuntimeEvent::Completed { result: None, .. }
        ));
    }

    // -----------------------------------------------------------------------
    // R49: unclosed tools must close BEFORE the engine's own terminal events
    // -----------------------------------------------------------------------

    /// Reaches Completed with a tool still open: ToolStart a, ToolStart b,
    /// ToolEnd b (a never closes), then the engine's own fatal Error +
    /// Completed pair.
    struct TerminalWithOpenToolEngine;

    #[async_trait]
    impl EngineRuntime for TerminalWithOpenToolEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            Ok(Box::pin(stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "terminal-open".into(),
                    model: None,
                    tools: vec![],
                },
                RuntimeEvent::ToolStart {
                    id: "a".into(),
                    name: "read".into(),
                    input: serde_json::json!({}),
                },
                RuntimeEvent::ToolStart {
                    id: "b".into(),
                    name: "grep".into(),
                    input: serde_json::json!({}),
                },
                RuntimeEvent::ToolEnd {
                    id: "b".into(),
                    name: "grep".into(),
                    result: ToolOutput::text("ok"),
                    duration_ms: Some(3),
                },
                RuntimeEvent::Error {
                    message: "fixture failure".into(),
                    recoverable: false,
                },
                RuntimeEvent::Completed {
                    session_id: "terminal-open".into(),
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
                tool_control: true,
                permission_callback: false,
                resume: true,
                cancel: true,
            }
        }
    }

    #[tokio::test]
    async fn unclosed_tool_closes_before_engine_terminal_events() {
        let hub = HubRuntime::new();
        hub.register_engine("terminal-open", Arc::new(TerminalWithOpenToolEngine))
            .unwrap();

        let events: Vec<_> = hub
            .execute(HubRequest::new(
                "terminal-open",
                RuntimeRequest::new("probe"),
            ))
            .unwrap()
            .collect()
            .await;

        // SessionStarted, ToolStart a, ToolStart b, ToolEnd b, then the
        // synthesized ToolEnd for a BEFORE the engine's own Error/Completed.
        let kinds: Vec<String> = events
            .iter()
            .map(|e| match &e.event {
                RuntimeEvent::SessionStarted { .. } => "started".into(),
                RuntimeEvent::ToolStart { id, .. } => format!("start:{id}"),
                RuntimeEvent::ToolEnd { id, .. } => format!("end:{id}"),
                RuntimeEvent::Error { .. } => "error".into(),
                RuntimeEvent::Completed { .. } => "completed".into(),
                other => format!("{other:?}"),
            })
            .collect();
        let error_at = kinds.iter().position(|k| k == "error").unwrap();
        let completed_at = kinds.iter().position(|k| k == "completed").unwrap();
        let ends_of_a: Vec<usize> = kinds
            .iter()
            .enumerate()
            .filter(|(_, k)| k.as_str() == "end:a")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            ends_of_a.len(),
            1,
            "only the unclosed tool gets a synthetic end, exactly once: {kinds:?}"
        );
        assert!(
            ends_of_a[0] < error_at,
            "synthetic ToolEnd must precede the engine's Error: {kinds:?}"
        );
        assert!(
            ends_of_a[0] < completed_at,
            "synthetic ToolEnd must precede the engine's Completed: {kinds:?}"
        );
        // The engine's own ToolEnd b is untouched and single.
        assert_eq!(
            kinds.iter().filter(|k| k.as_str() == "end:b").count(),
            1,
            "already-closed tool must not be re-closed: {kinds:?}"
        );
        // The engine emitted a fatal Error: per session semantics the status
        // stays Failed even though Completed followed (matches update_session
        // contract tests).
        assert_eq!(
            hub.session(&events[0].product_session_id).unwrap().status,
            HubSessionStatus::Failed
        );
    }

    // -----------------------------------------------------------------------
    // R50: cancel without engine cancel capability must still release the
    // Hub claim
    // -----------------------------------------------------------------------

    /// Declares cancel:false and records any cancel attempts; the stream
    /// stays pending forever after SessionStarted.
    struct UncancellablePendingEngine {
        cancel_attempts: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl EngineRuntime for UncancellablePendingEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            let started = stream::iter(vec![RuntimeEvent::SessionStarted {
                session_id: "uncancellable-1".into(),
                model: None,
                tools: vec![],
            }]);
            Ok(Box::pin(started.chain(stream::pending())))
        }

        async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
            self.cancel_attempts.lock().unwrap().push(session_id.into());
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
                cancel: false,
            }
        }
    }

    #[tokio::test]
    async fn cancel_without_engine_capability_releases_claim_and_allows_reopen() {
        let hub = HubRuntime::new();
        let engine = Arc::new(UncancellablePendingEngine {
            cancel_attempts: Mutex::new(Vec::new()),
        });
        hub.register_engine("uncanc", engine.clone()).unwrap();

        let mut stream = hub
            .execute(HubRequest::new("uncanc", RuntimeRequest::new("hi")))
            .unwrap();
        let pid = stream.next().await.unwrap().product_session_id;
        assert!(hub.has_live_run(&pid));

        // Before the fix this returned Err(Unsupported) and left the session
        // Running forever. Hub must release its own claim instead.
        hub.cancel(&pid)
            .await
            .expect("cancel must release the Hub claim even without engine cancel");
        assert!(
            engine.cancel_attempts.lock().unwrap().is_empty(),
            "engine without cancel capability must not be asked to cancel"
        );
        assert_ne!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Running,
            "released claim must not leave the session running"
        );

        // The freed claim is immediately reusable.
        let mut next = hub
            .execute(HubRequest::new("uncanc", RuntimeRequest::new("again")).resume(pid.clone()))
            .expect("released session must be re-executable");
        let event = tokio::time::timeout(std::time::Duration::from_millis(500), next.next())
            .await
            .expect("reopened session must start")
            .unwrap();
        assert!(matches!(event.event, RuntimeEvent::SessionStarted { .. }));
    }

    #[tokio::test]
    async fn cancel_reporting_unsupported_still_releases_claim() {
        // A well-behaved capability declaration is covered above; here the
        // engine claims cancel support but its implementation refuses — the
        // Hub release must still happen instead of failing the request.
        struct LiesAboutCancelEngine;
        #[async_trait]
        impl EngineRuntime for LiesAboutCancelEngine {
            fn execute(
                &self,
                _request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
                let started = stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: "lies-1".into(),
                    model: None,
                    tools: vec![],
                }]);
                Ok(Box::pin(started.chain(stream::pending())))
            }
            async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
                Err(RuntimeError::Unsupported("cancel".into()))
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

        let hub = HubRuntime::new();
        hub.register_engine("lies", Arc::new(LiesAboutCancelEngine))
            .unwrap();
        let mut stream = hub
            .execute(HubRequest::new("lies", RuntimeRequest::new("hi")))
            .unwrap();
        let pid = stream.next().await.unwrap().product_session_id;
        hub.cancel(&pid)
            .await
            .expect("Unsupported engine cancel must fall back to the Hub release");
        assert_ne!(
            hub.session(&pid).unwrap().status,
            HubSessionStatus::Running,
            "Hub release must move the session out of Running (the engine process itself is not claimed dead)"
        );
    }

    // -----------------------------------------------------------------------
    // R58: RunHandle registration order + per-generation cleanup
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn fast_finishing_run_leaves_no_stale_handle() {
        let hub = HubRuntime::new();
        hub.register_engine("alva", Arc::new(FakeEngine::new()))
            .unwrap();
        let events: Vec<_> = hub
            .execute(HubRequest::new("alva", RuntimeRequest::new("done fast")))
            .unwrap()
            .collect()
            .await;
        let pid = events[0].product_session_id.clone();
        // The product stream only ends after the run task drops event_tx —
        // i.e. after its tail cleanup. The entry must be gone (the old
        // spawn-before-insert order could strand it forever).
        assert!(
            !hub.has_live_run(&pid),
            "a finished run must not leave a stale control-plane handle"
        );
    }

    /// Turn 1 emits SessionStarted + Completed, then parks until released a
    /// second time before ending the stream — holding the run task between
    /// the terminal event and its tail cleanup. Turn 2 behaves like a live
    /// run that records cancels.
    struct TwoTurnGatedEngine {
        gate: Arc<tokio::sync::Notify>,
        turn: AtomicUsize,
        cancelled: Mutex<Vec<String>>,
    }

    impl TwoTurnGatedEngine {
        fn first_stream(
            &self,
        ) -> Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>> {
            let gate = self.gate.clone();
            enum St {
                EmitStarted,
                WaitThenCompleted,
                WaitThenEnd,
            }
            Box::pin(stream::unfold(St::EmitStarted, move |st| {
                let gate = gate.clone();
                async move {
                    match st {
                        St::EmitStarted => Some((
                            RuntimeEvent::SessionStarted {
                                session_id: "gated-1".into(),
                                model: None,
                                tools: vec![],
                            },
                            St::WaitThenCompleted,
                        )),
                        St::WaitThenCompleted => {
                            gate.notified().await;
                            Some((
                                RuntimeEvent::Completed {
                                    session_id: "gated-1".into(),
                                    result: None,
                                    usage: None,
                                },
                                St::WaitThenEnd,
                            ))
                        }
                        St::WaitThenEnd => {
                            gate.notified().await;
                            None
                        }
                    }
                }
            }))
        }
    }

    #[async_trait]
    impl EngineRuntime for TwoTurnGatedEngine {
        fn execute(
            &self,
            _request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            if self.turn.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(self.first_stream())
            } else {
                let started = stream::iter(vec![RuntimeEvent::SessionStarted {
                    session_id: "second-1".into(),
                    model: None,
                    tools: vec![],
                }]);
                Ok(Box::pin(started.chain(stream::pending())))
            }
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
    async fn old_run_tail_does_not_delete_the_new_runs_handle() {
        let hub = HubRuntime::new();
        let engine = Arc::new(TwoTurnGatedEngine {
            gate: Arc::new(tokio::sync::Notify::new()),
            turn: AtomicUsize::new(0),
            cancelled: Mutex::new(Vec::new()),
        });
        hub.register_engine("gated", engine.clone()).unwrap();

        // Turn 1: SessionStarted arrives; the run parks before Completed.
        let mut first = hub
            .execute(HubRequest::new("gated", RuntimeRequest::new("one")))
            .unwrap();
        let pid = first.next().await.unwrap().product_session_id;

        // Release the gate: Completed flows, session turns terminal, but the
        // run task is still parked before its tail cleanup.
        engine.gate.notify_one();
        let completed = tokio::time::timeout(std::time::Duration::from_millis(500), first.next())
            .await
            .expect("gated Completed must arrive")
            .unwrap();
        assert!(matches!(completed.event, RuntimeEvent::Completed { .. }));
        assert!(hub.has_live_run(&pid), "run 1 still holds the handle");

        // Turn 2 registers on the now-terminal session, replacing run 1's
        // entry with a fresh generation.
        let mut second = hub
            .execute(HubRequest::new("gated", RuntimeRequest::new("two")).resume(pid.clone()))
            .unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_millis(500), second.next())
            .await
            .expect("second turn must start")
            .unwrap();
        assert!(matches!(event.event, RuntimeEvent::SessionStarted { .. }));

        // End run 1's stream: its tail must NOT delete run 2's handle. Give
        // the old tail a bounded window to (wrongly) run before asserting.
        engine.gate.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            hub.has_live_run(&pid),
            "old run's tail cleanup must not remove the new run's handle"
        );

        // The control plane still routes to run 2.
        hub.cancel(&pid).await.unwrap();
        assert_eq!(
            engine.cancelled.lock().unwrap().as_slice(),
            &["second-1".to_string()],
            "cancel must reach the NEW run after the old run's tail cleanup"
        );
    }
}
