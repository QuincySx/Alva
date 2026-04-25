// INPUT:  async_trait, serde, std::collections, tokio::sync::RwLock, std::sync::Arc,
//         alva_kernel_abi::{AgentSession, SessionError, UsageMetadata}
// OUTPUT: SessionRegistry trait, SessionMetadata (single unified record), SessionStatus,
//         SessionMetadataPatch, SessionFilter, SessionOrder, SessionPage,
//         InMemorySessionRegistry, ThreadStats, ThreadUsage,
//         thread_view, thread_tree, primary_thread_for
// POS:    **Harness-level** session collection. Kernel exposes only the bare event-log
//         primitive (`AgentSession::append`, `parent_session_id`); this module builds the
//         queryable / filterable / tree-walkable view that App needs. Single canonical
//         `SessionMetadata` type — no parallel `ThreadView` struct; spawn-tree fields
//         (`session_group_id`, `depth`) are `Option<…>` on the record itself and populated
//         by enrichment helpers (`thread_view` / `thread_tree`) when callers want them.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use alva_kernel_abi::agent_session::{AgentSession, SessionError};
use alva_kernel_abi::base::message::UsageMetadata;

// ===========================================================================
// SessionStatus
// ===========================================================================

/// Lifecycle status of a session in the registry.
///
/// Mirrors Anthropic Managed Agents `session.status`:
/// - `Running`: an agent loop is currently driving events into the session.
/// - `Idle`: not running, waiting for input or HITL approval. Pair with
///   `pending_actions()` (alva-agent-security) to find what's blocking.
/// - `Rescheduling`: transient error (model overload / rate limit / MCP
///   disconnect) — the runtime is backing off and will retry.
/// - `Terminated`: natural completion or fatal error; no further events.
///
/// The registry stores the latest value without history; observers can
/// derive the full transition timeline by scanning `session.status_*`
/// events in the session log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
    Idle,
    Rescheduling,
    Terminated,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Idle => "idle",
            Self::Rescheduling => "rescheduling",
            Self::Terminated => "terminated",
        }
    }
}

// ===========================================================================
// ThreadStats / ThreadUsage
//
// Thread-level (== session-level in alva, since each execution unit owns one
// AgentSession) timing and token accounting. Mirrors Anthropic Managed
// Agents `BetaManagedAgentsSessionThreadStats` and `…SessionThreadUsage`.
// ===========================================================================

/// Wall-clock timing for one thread of execution.
///
/// Stored in milliseconds (Anthropic uses seconds in its API; the registry
/// keeps the higher-precision unit and lets the App layer divide on the way
/// out). All fields accumulate over the thread's lifetime; `update_stats`
/// overwrites, `record_started_ms` / `record_active_ms` accumulate by delta.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadStats {
    /// Time from thread creation to first agent loop iteration. 0 for the
    /// primary thread (which starts immediately on session create). Non-zero
    /// for child threads that wait for scheduling.
    pub startup_ms: u64,
    /// Cumulative time the thread spent inside `run_agent` (running and
    /// emitting events). Excludes idle / awaiting-HITL time.
    pub active_ms: u64,
    /// Wall-clock time since creation. For terminated threads, frozen at the
    /// terminal transition. The registry doesn't auto-tick this — App code
    /// updates it via `update_stats` when status changes.
    pub duration_ms: u64,
}

/// Cumulative token usage across all model requests in one thread. Mirrors
/// Anthropic Managed Agents `…SessionThreadUsage`.
///
/// The registry stores u64 (rather than u32 like `UsageMetadata`) to avoid
/// overflow on long-running sessions. `record_usage` accumulates from a
/// per-request `UsageMetadata`; `update` via patch overwrites.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl ThreadUsage {
    /// Accumulate a per-request `UsageMetadata` into this running total.
    pub fn accumulate(&mut self, delta: &UsageMetadata) {
        self.input_tokens = self.input_tokens.saturating_add(delta.input_tokens as u64);
        self.output_tokens = self.output_tokens.saturating_add(delta.output_tokens as u64);
        if let Some(c) = delta.cache_creation_input_tokens {
            self.cache_creation_input_tokens =
                self.cache_creation_input_tokens.saturating_add(c as u64);
        }
        if let Some(c) = delta.cache_read_input_tokens {
            self.cache_read_input_tokens =
                self.cache_read_input_tokens.saturating_add(c as u64);
        }
    }
}

// ===========================================================================
// SessionMetadata
// ===========================================================================

/// Queryable snapshot of session-level state. Distinct from `AgentSession`
/// (the event log handle): this is the "row" returned by registry listings.
///
/// `created_at` / `updated_at` / `archived_at` are wall-clock epoch millis.
/// Field semantics mirror Anthropic Managed Agents `BetaManagedAgentsSession`
/// modulo names; not all Anthropic fields (`resources`, `outcome_evaluations`,
/// `usage`) are tracked here because they're derived state (resources live in
/// the resource extension; outcomes / usage are computed from the event log).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Stable session id (matches `AgentSession::session_id`).
    pub session_id: String,

    /// Parent session id for sub-agent sessions. `None` for root sessions.
    /// Matches `AgentSession::parent_session_id`.
    pub parent_session_id: Option<String>,

    /// Current lifecycle status.
    pub status: SessionStatus,

    /// Free-form agent identifier. The registry doesn't validate this — it
    /// just indexes it for `SessionFilter::agent_id` queries.
    pub agent_id: Option<String>,

    /// Human-readable title for display.
    pub title: Option<String>,

    /// Free-form key/value metadata. Anthropic recommends max 16 pairs, but
    /// the registry doesn't enforce it.
    pub metadata: BTreeMap<String, String>,

    /// Wall-clock epoch millis at create time.
    pub created_at: i64,

    /// Wall-clock epoch millis of the last mutation (status / title / etc.).
    pub updated_at: i64,

    /// Wall-clock epoch millis when archived. `None` if not archived.
    /// Archived sessions are hidden from default `list` queries; set
    /// `SessionFilter::include_archived` to include them.
    pub archived_at: Option<i64>,

    /// Thread-level timing. Each `AgentSession` is one thread of execution
    /// in alva's model, so per-session stats == per-thread stats. Update via
    /// `SessionRegistry::update_stats` or `SessionMetadataPatch::stats`.
    #[serde(default)]
    pub stats: ThreadStats,

    /// Thread-level token usage. Accumulate per-model-request via
    /// `SessionRegistry::record_usage`, or overwrite via patch.
    #[serde(default)]
    pub usage: ThreadUsage,

    /// Root of the spawn tree this record belongs to (derived field).
    /// `None` when reading raw metadata from the registry; populated by
    /// `thread_view` / `thread_tree` enrichment helpers when the caller
    /// asks for the Anthropic Managed Agents "session_group_id" view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_group_id: Option<String>,

    /// Depth in the spawn tree (derived). 0 for the primary; `parent.depth
    /// + 1` for child sessions. Same population rules as
    /// `session_group_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
}

impl SessionMetadata {
    /// Convenience constructor for a fresh `Idle` session with the given id.
    /// Timestamps are set to now; stats and usage are zero; derived fields
    /// (`session_group_id`, `depth`) are `None`.
    pub fn new(session_id: impl Into<String>) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            session_id: session_id.into(),
            parent_session_id: None,
            status: SessionStatus::Idle,
            agent_id: None,
            title: None,
            metadata: BTreeMap::new(),
            created_at: now,
            updated_at: now,
            archived_at: None,
            stats: ThreadStats::default(),
            usage: ThreadUsage::default(),
            session_group_id: None,
            depth: None,
        }
    }
}

// ===========================================================================
// SessionMetadataPatch
// ===========================================================================

/// Partial-update payload for `SessionRegistry::update`.
///
/// Wrapping each field in `Option<...>` lets callers distinguish "leave
/// alone" from "set to None". For fields that are themselves `Option<T>`,
/// pass `Some(None)` to clear and `Some(Some(value))` to set.
#[derive(Debug, Clone, Default)]
pub struct SessionMetadataPatch {
    pub status: Option<SessionStatus>,
    pub agent_id: Option<Option<String>>,
    pub title: Option<Option<String>>,
    pub metadata: Option<BTreeMap<String, String>>,
    /// Overwrite the entire `ThreadStats` block. To accumulate, use
    /// `SessionRegistry::record_active_ms` etc. instead.
    pub stats: Option<ThreadStats>,
    /// Overwrite the entire `ThreadUsage` block. To accumulate per-request,
    /// use `SessionRegistry::record_usage` instead.
    pub usage: Option<ThreadUsage>,
}

impl SessionMetadataPatch {
    pub fn status(mut self, status: SessionStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(Some(title.into()));
        self
    }

    pub fn clear_title(mut self) -> Self {
        self.title = Some(None);
        self
    }

    pub fn agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(Some(agent_id.into()));
        self
    }

    pub fn metadata(mut self, metadata: BTreeMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn stats(mut self, stats: ThreadStats) -> Self {
        self.stats = Some(stats);
        self
    }

    pub fn usage(mut self, usage: ThreadUsage) -> Self {
        self.usage = Some(usage);
        self
    }
}

// ===========================================================================
// SessionFilter / SessionOrder / SessionPage
// ===========================================================================

/// Filter for `SessionRegistry::list` / `count`. All fields optional; the
/// defaults match "everything except archived, newest first, no cap".
///
/// Maps onto Anthropic `SessionListParams`. `parent_session_id` is an alva-
/// specific extension for the spawn tree — pass `Some(None)` to restrict to
/// roots only, `Some(Some(id))` to filter to a specific parent.
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    /// Filter by one or more statuses. Empty `Vec` / `None` = any status.
    pub statuses: Option<Vec<SessionStatus>>,

    /// Restrict to sessions with this `agent_id`.
    pub agent_id: Option<String>,

    /// Restrict by parent session id. Use `Some(None)` for root-only,
    /// `Some(Some(id))` for a specific parent. `None` = any.
    pub parent_session_id: Option<Option<String>>,

    /// `created_at` lower bound (exclusive — strictly greater).
    pub created_after: Option<i64>,

    /// `created_at` upper bound (exclusive — strictly less).
    pub created_before: Option<i64>,

    /// When false (the default), archived sessions are hidden.
    pub include_archived: bool,

    /// Sort direction by `created_at`. Default `Desc` (newest first).
    pub order: SessionOrder,

    /// Max items in the returned page. 0 means "no cap".
    pub limit: usize,

    /// Opaque resume cursor from a previous `SessionPage::next_cursor`.
    /// The cursor format is implementation-defined; pass it through
    /// verbatim. `None` starts from the beginning of the sort order.
    pub after: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrder {
    /// Newest first (default).
    #[default]
    Desc,
    /// Oldest first.
    Asc,
}

/// One page of session metadata. `next_cursor` is `Some` only when there
/// are more items past this page; pass it as `SessionFilter::after` to
/// resume.
#[derive(Debug, Clone)]
pub struct SessionPage {
    pub items: Vec<SessionMetadata>,
    pub next_cursor: Option<String>,
}

// ===========================================================================
// SessionRegistry trait
// ===========================================================================

/// A collection of `AgentSession`s addressable by id.
///
/// The registry tracks `SessionMetadata` independently of the event log so
/// `list` / `count` queries are O(metadata) instead of scanning per-session
/// event histories. The event log itself remains the source of truth — a
/// fresh registry can be rebuilt by scanning all sessions' logs.
///
/// ## Concurrency
///
/// All methods are `async` and assume the implementation provides interior
/// locking. Multiple callers can `get` concurrently; mutations serialize.
///
/// ## Persistent backends
///
/// In-memory registry holds `Arc<dyn AgentSession>` directly. Persistent
/// backends (SQLite, remote) typically materialize the `AgentSession` handle
/// lazily on `get` by replaying the event log. They keep the metadata
/// projection eager so list queries don't pay event-log cost.
#[async_trait]
pub trait SessionRegistry: Send + Sync {
    /// Insert a session into the registry. Fails with `SessionError::Other`
    /// if `meta.session_id` already exists; callers can `get` first to
    /// implement upsert.
    async fn insert(
        &self,
        session: Arc<dyn AgentSession>,
        meta: SessionMetadata,
    ) -> Result<(), SessionError>;

    /// Retrieve the live `AgentSession` handle by id. `None` if not in the
    /// registry. Persistent backends may materialize the handle on demand
    /// from the underlying event log.
    async fn get(&self, session_id: &str) -> Option<Arc<dyn AgentSession>>;

    /// Read the metadata snapshot for a session. `None` if not in the registry.
    async fn metadata(&self, session_id: &str) -> Option<SessionMetadata>;

    /// Patch metadata. Fields set in the patch are written; others preserved.
    /// `updated_at` is bumped to "now" on every successful call. Errors with
    /// `SessionError::NotFound` if `session_id` is unknown.
    async fn update(
        &self,
        session_id: &str,
        patch: SessionMetadataPatch,
    ) -> Result<(), SessionError>;

    /// Mark the session archived. Default `list` queries hide archived
    /// sessions; readers can still `get` them by id, and the event log is
    /// preserved.
    async fn archive(&self, session_id: &str) -> Result<(), SessionError>;

    /// Hard-remove. The `AgentSession` handle is dropped; persistent
    /// backends should remove the event log too. Prefer `archive` unless
    /// the caller is sure no observer needs the history.
    async fn delete(&self, session_id: &str) -> Result<(), SessionError>;

    /// List sessions matching the filter. Pagination is opaque cursor-based
    /// — `SessionPage::next_cursor` is passed back as `SessionFilter::after`
    /// to resume.
    async fn list(&self, filter: &SessionFilter) -> SessionPage;

    /// Count sessions matching the filter without retrieving the page.
    /// Cheaper than `list` for the same filter when only the count matters.
    async fn count(&self, filter: &SessionFilter) -> usize;

    /// Direct children of `parent_session_id` (one level only). Returns
    /// metadata snapshots ordered by `created_at` ascending.
    ///
    /// Default impl walks `list` with a parent filter; the InMemory
    /// reference impl overrides for clarity but the result is identical.
    async fn children(&self, parent_session_id: &str) -> Vec<SessionMetadata> {
        let page = self
            .list(&SessionFilter {
                parent_session_id: Some(Some(parent_session_id.to_string())),
                include_archived: true,
                order: SessionOrder::Asc,
                ..Default::default()
            })
            .await;
        page.items
    }

    /// All descendants of `root_session_id` (BFS, depth-first equivalents
    /// also acceptable), excluding the root itself. Order is BFS by spawn
    /// hierarchy: direct children first, then grandchildren, etc. Within
    /// each level, items are ordered by `created_at` ascending.
    ///
    /// Default impl walks `children` recursively. Persistent backends can
    /// override with a single recursive-CTE query.
    async fn descendants(&self, root_session_id: &str) -> Vec<SessionMetadata> {
        let mut out = Vec::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(root_session_id.to_string());
        let mut seen: HashSet<String> = HashSet::new();
        // Skip the root itself — descendants are strictly proper descendants.
        seen.insert(root_session_id.to_string());

        while let Some(parent) = queue.pop_front() {
            let kids = self.children(&parent).await;
            for k in kids {
                if seen.insert(k.session_id.clone()) {
                    queue.push_back(k.session_id.clone());
                    out.push(k);
                }
            }
        }
        out
    }

    /// Accumulate per-request token usage into this session's running total.
    /// Implementations MUST make this atomic — reading the existing usage,
    /// adding `delta`, and writing the result back happen as a single
    /// critical section. Default impl is **non-atomic** (read-then-write
    /// over the trait surface) and is provided for backends that genuinely
    /// can't do better; override it.
    async fn record_usage(
        &self,
        session_id: &str,
        delta: &UsageMetadata,
    ) -> Result<(), SessionError> {
        let mut current = self
            .metadata(session_id)
            .await
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?
            .usage;
        current.accumulate(delta);
        self.update(session_id, SessionMetadataPatch::default().usage(current))
            .await
    }

    /// Add `delta_ms` to this session's `active_ms` counter. Atomic in
    /// reference implementations; non-atomic default impl is provided for
    /// the same reason as `record_usage`.
    async fn record_active_ms(
        &self,
        session_id: &str,
        delta_ms: u64,
    ) -> Result<(), SessionError> {
        let mut current = self
            .metadata(session_id)
            .await
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?
            .stats;
        current.active_ms = current.active_ms.saturating_add(delta_ms);
        self.update(session_id, SessionMetadataPatch::default().stats(current))
            .await
    }
}

// ===========================================================================
// InMemorySessionRegistry
// ===========================================================================

/// In-memory reference implementation of `SessionRegistry`. Used by tests,
/// embedded callers that don't need persistence, and as a worked example
/// for persistent backends.
///
/// Storage: a single `HashMap<session_id, Entry>` under a `RwLock`. Reads
/// take a shared lock; mutations take an exclusive lock. Filter / sort /
/// paginate happens on a fresh clone of metadata so the lock is not held
/// across awaits in user code.
pub struct InMemorySessionRegistry {
    entries: RwLock<HashMap<String, Entry>>,
}

struct Entry {
    session: Arc<dyn AgentSession>,
    meta: SessionMetadata,
}

impl InMemorySessionRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemorySessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionRegistry for InMemorySessionRegistry {
    async fn insert(
        &self,
        session: Arc<dyn AgentSession>,
        meta: SessionMetadata,
    ) -> Result<(), SessionError> {
        let mut entries = self.entries.write().await;
        if entries.contains_key(&meta.session_id) {
            return Err(SessionError::Other(format!(
                "session {} already exists in registry",
                meta.session_id
            )));
        }
        entries.insert(meta.session_id.clone(), Entry { session, meta });
        Ok(())
    }

    async fn get(&self, session_id: &str) -> Option<Arc<dyn AgentSession>> {
        self.entries
            .read()
            .await
            .get(session_id)
            .map(|e| e.session.clone())
    }

    async fn metadata(&self, session_id: &str) -> Option<SessionMetadata> {
        self.entries
            .read()
            .await
            .get(session_id)
            .map(|e| e.meta.clone())
    }

    async fn update(
        &self,
        session_id: &str,
        patch: SessionMetadataPatch,
    ) -> Result<(), SessionError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(session_id)
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;
        if let Some(status) = patch.status {
            entry.meta.status = status;
        }
        if let Some(agent_id) = patch.agent_id {
            entry.meta.agent_id = agent_id;
        }
        if let Some(title) = patch.title {
            entry.meta.title = title;
        }
        if let Some(metadata) = patch.metadata {
            entry.meta.metadata = metadata;
        }
        if let Some(stats) = patch.stats {
            entry.meta.stats = stats;
        }
        if let Some(usage) = patch.usage {
            entry.meta.usage = usage;
        }
        entry.meta.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    /// Atomic accumulate: holds the entries write lock for the entire
    /// read+add+write cycle, so concurrent `record_usage` calls don't lose
    /// updates the way the default trait impl would.
    async fn record_usage(
        &self,
        session_id: &str,
        delta: &UsageMetadata,
    ) -> Result<(), SessionError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(session_id)
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;
        entry.meta.usage.accumulate(delta);
        entry.meta.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    /// Atomic accumulate, same rationale as `record_usage`.
    async fn record_active_ms(
        &self,
        session_id: &str,
        delta_ms: u64,
    ) -> Result<(), SessionError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(session_id)
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;
        entry.meta.stats.active_ms = entry.meta.stats.active_ms.saturating_add(delta_ms);
        entry.meta.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    async fn archive(&self, session_id: &str) -> Result<(), SessionError> {
        let mut entries = self.entries.write().await;
        let entry = entries
            .get_mut(session_id)
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;
        let now = chrono::Utc::now().timestamp_millis();
        entry.meta.archived_at = Some(now);
        entry.meta.updated_at = now;
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<(), SessionError> {
        let mut entries = self.entries.write().await;
        if entries.remove(session_id).is_none() {
            return Err(SessionError::NotFound(session_id.to_string()));
        }
        Ok(())
    }

    async fn list(&self, filter: &SessionFilter) -> SessionPage {
        // Clone all matching metadata under the shared lock, then sort /
        // paginate without holding it.
        let snapshot: Vec<SessionMetadata> = {
            let entries = self.entries.read().await;
            entries
                .values()
                .filter(|e| filter_match(&e.meta, filter))
                .map(|e| e.meta.clone())
                .collect()
        };

        let mut items = snapshot;
        match filter.order {
            SessionOrder::Desc => {
                // Tiebreak by session_id to make ordering deterministic when
                // created_at collides (common in tests using millisecond clocks).
                items.sort_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| b.session_id.cmp(&a.session_id))
                });
            }
            SessionOrder::Asc => {
                items.sort_by(|a, b| {
                    a.created_at
                        .cmp(&b.created_at)
                        .then_with(|| a.session_id.cmp(&b.session_id))
                });
            }
        }

        // Cursor: opaque string carrying the session_id of the previous
        // page's last item. Skip past it (and through any ties at the same
        // created_at, which the secondary sort already disambiguated).
        if let Some(cursor) = filter.after.as_deref() {
            if let Some(pos) = items.iter().position(|m| m.session_id == cursor) {
                items.drain(..=pos);
            }
        }

        let next_cursor = if filter.limit > 0 && items.len() > filter.limit {
            let last_id = items[filter.limit - 1].session_id.clone();
            items.truncate(filter.limit);
            Some(last_id)
        } else {
            None
        };

        SessionPage { items, next_cursor }
    }

    async fn count(&self, filter: &SessionFilter) -> usize {
        let entries = self.entries.read().await;
        entries
            .values()
            .filter(|e| filter_match(&e.meta, filter))
            .count()
    }
}

fn filter_match(meta: &SessionMetadata, filter: &SessionFilter) -> bool {
    if !filter.include_archived && meta.archived_at.is_some() {
        return false;
    }
    if let Some(ref statuses) = filter.statuses {
        if !statuses.is_empty() && !statuses.contains(&meta.status) {
            return false;
        }
    }
    if let Some(ref agent_id) = filter.agent_id {
        if meta.agent_id.as_deref() != Some(agent_id.as_str()) {
            return false;
        }
    }
    if let Some(parent_filter) = &filter.parent_session_id {
        // Some(None) = "roots only"; Some(Some(id)) = "child of id".
        if parent_filter.as_deref() != meta.parent_session_id.as_deref() {
            return false;
        }
    }
    if let Some(after) = filter.created_after {
        if meta.created_at <= after {
            return false;
        }
    }
    if let Some(before) = filter.created_before {
        if meta.created_at >= before {
            return false;
        }
    }
    true
}

// ===========================================================================
// Thread-tree enrichment helpers
//
// "Thread" in Anthropic Managed Agents == one execution unit, which in alva
// is one `AgentSession` (== one `SessionMetadata` row). These functions
// walk parent links to populate the two derived fields (`session_group_id`,
// `depth`) on `SessionMetadata` so callers that want the Anthropic-shape
// view get it without a separate `ThreadView` type.
// ===========================================================================

/// Read the metadata for `session_id` and enrich it with `session_group_id`
/// + `depth` by climbing the parent chain. Returns `None` if `session_id`
/// is not in the registry; returns `Some` with derived fields filled even
/// if some ancestor is missing (climb stops at the highest reachable
/// ancestor, which becomes the group root).
pub async fn thread_view(
    registry: &dyn SessionRegistry,
    session_id: &str,
) -> Option<SessionMetadata> {
    let meta = registry.metadata(session_id).await?;
    Some(enrich_with_tree_fields(registry, meta).await)
}

/// All sessions in the spawn tree rooted at `root_id`, BFS order (root
/// first, then immediate children, then grandchildren), each enriched
/// with `session_group_id` + `depth`. Empty `Vec` if `root_id` is unknown.
pub async fn thread_tree(
    registry: &dyn SessionRegistry,
    root_id: &str,
) -> Vec<SessionMetadata> {
    let Some(root) = thread_view(registry, root_id).await else {
        return Vec::new();
    };
    let descendants = registry.descendants(root_id).await;
    let mut out = Vec::with_capacity(1 + descendants.len());
    out.push(root);
    for d in descendants {
        let enriched = enrich_with_tree_fields(registry, d).await;
        out.push(enriched);
    }
    out
}

/// Find the root of the spawn tree containing `session_id`. For an
/// already-root session, returns its own id. `None` if `session_id` is
/// unknown.
pub async fn primary_thread_for(
    registry: &dyn SessionRegistry,
    session_id: &str,
) -> Option<String> {
    let meta = registry.metadata(session_id).await?;
    let (group_id, _depth) = climb_to_root(registry, &meta).await;
    Some(group_id)
}

/// Climb `parent_session_id` links until `None` or a missing ancestor.
/// Returns the topmost reachable session id and the depth (hops taken).
/// Bounded to defend against pathological cycles.
async fn climb_to_root(
    registry: &dyn SessionRegistry,
    meta: &SessionMetadata,
) -> (String, u32) {
    const MAX_HOPS: u32 = 64;
    let mut cursor = meta.clone();
    let mut depth: u32 = 0;
    while let Some(parent_id) = cursor.parent_session_id.clone() {
        if depth >= MAX_HOPS {
            break;
        }
        match registry.metadata(&parent_id).await {
            Some(parent_meta) => {
                cursor = parent_meta;
                depth += 1;
            }
            None => break,
        }
    }
    (cursor.session_id, depth)
}

/// Fill in `session_group_id` and `depth` on a `SessionMetadata`, leaving
/// every other field untouched. Used by `thread_view` / `thread_tree`.
async fn enrich_with_tree_fields(
    registry: &dyn SessionRegistry,
    mut meta: SessionMetadata,
) -> SessionMetadata {
    let (group_id, depth) = climb_to_root(registry, &meta).await;
    meta.session_group_id = Some(group_id);
    meta.depth = Some(depth);
    meta
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::agent_session::InMemoryAgentSession;

    fn make_session(id: &str) -> Arc<dyn AgentSession> {
        Arc::new(InMemoryAgentSession::with_id(id.to_string()))
    }

    fn meta(id: &str, status: SessionStatus, created_at: i64) -> SessionMetadata {
        let mut m = SessionMetadata::new(id);
        m.status = status;
        m.created_at = created_at;
        m.updated_at = created_at;
        m
    }

    #[tokio::test]
    async fn insert_and_get_roundtrip() {
        let r = InMemorySessionRegistry::new();
        let s = make_session("s-1");
        r.insert(s.clone(), SessionMetadata::new("s-1")).await.unwrap();

        let got = r.get("s-1").await.expect("session present");
        assert_eq!(got.session_id(), "s-1");

        let m = r.metadata("s-1").await.expect("meta present");
        assert_eq!(m.session_id, "s-1");
        assert_eq!(m.status, SessionStatus::Idle);
    }

    #[tokio::test]
    async fn insert_rejects_duplicate_ids() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("s-1"), SessionMetadata::new("s-1"))
            .await
            .unwrap();
        let err = r
            .insert(make_session("s-1"), SessionMetadata::new("s-1"))
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::Other(_)));
    }

    #[tokio::test]
    async fn update_patches_selected_fields_only() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("s-1"), SessionMetadata::new("s-1"))
            .await
            .unwrap();
        let before = r.metadata("s-1").await.unwrap();

        // Sleep 2ms to guarantee a different timestamp on update.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;

        r.update(
            "s-1",
            SessionMetadataPatch::default()
                .status(SessionStatus::Running)
                .title("hello"),
        )
        .await
        .unwrap();

        let after = r.metadata("s-1").await.unwrap();
        assert_eq!(after.status, SessionStatus::Running);
        assert_eq!(after.title.as_deref(), Some("hello"));
        assert_eq!(after.created_at, before.created_at, "create timestamp preserved");
        assert!(after.updated_at > before.updated_at, "updated_at bumped");
        assert_eq!(after.agent_id, None, "untouched fields preserved");
    }

    #[tokio::test]
    async fn update_can_clear_optional_field() {
        let r = InMemorySessionRegistry::new();
        let mut m = SessionMetadata::new("s-1");
        m.title = Some("initial".into());
        r.insert(make_session("s-1"), m).await.unwrap();

        r.update("s-1", SessionMetadataPatch::default().clear_title())
            .await
            .unwrap();
        let after = r.metadata("s-1").await.unwrap();
        assert_eq!(after.title, None);
    }

    #[tokio::test]
    async fn update_missing_session_errors() {
        let r = InMemorySessionRegistry::new();
        let err = r
            .update("missing", SessionMetadataPatch::default().status(SessionStatus::Idle))
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[tokio::test]
    async fn archive_hides_from_default_list() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("s-1"), meta("s-1", SessionStatus::Idle, 100))
            .await
            .unwrap();
        r.insert(make_session("s-2"), meta("s-2", SessionStatus::Idle, 200))
            .await
            .unwrap();

        r.archive("s-1").await.unwrap();

        let page = r.list(&SessionFilter::default()).await;
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].session_id, "s-2");

        let with_archived = r
            .list(&SessionFilter {
                include_archived: true,
                ..Default::default()
            })
            .await;
        assert_eq!(with_archived.items.len(), 2);
    }

    #[tokio::test]
    async fn delete_removes_completely() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("s-1"), SessionMetadata::new("s-1"))
            .await
            .unwrap();
        r.delete("s-1").await.unwrap();

        assert!(r.get("s-1").await.is_none());
        assert!(r.metadata("s-1").await.is_none());
        assert_eq!(
            r.count(&SessionFilter {
                include_archived: true,
                ..Default::default()
            })
            .await,
            0
        );

        let err = r.delete("s-1").await.unwrap_err();
        assert!(matches!(err, SessionError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("a"), meta("a", SessionStatus::Running, 100))
            .await
            .unwrap();
        r.insert(make_session("b"), meta("b", SessionStatus::Idle, 200))
            .await
            .unwrap();
        r.insert(make_session("c"), meta("c", SessionStatus::Running, 300))
            .await
            .unwrap();

        let page = r
            .list(&SessionFilter {
                statuses: Some(vec![SessionStatus::Running]),
                order: SessionOrder::Asc,
                ..Default::default()
            })
            .await;
        let ids: Vec<_> = page.items.iter().map(|m| m.session_id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[tokio::test]
    async fn list_filters_by_agent_id() {
        let r = InMemorySessionRegistry::new();
        let mut m_a = SessionMetadata::new("a");
        m_a.agent_id = Some("agent_x".into());
        m_a.created_at = 100;
        let mut m_b = SessionMetadata::new("b");
        m_b.agent_id = Some("agent_y".into());
        m_b.created_at = 200;
        let mut m_c = SessionMetadata::new("c");
        m_c.agent_id = Some("agent_x".into());
        m_c.created_at = 300;

        r.insert(make_session("a"), m_a).await.unwrap();
        r.insert(make_session("b"), m_b).await.unwrap();
        r.insert(make_session("c"), m_c).await.unwrap();

        let page = r
            .list(&SessionFilter {
                agent_id: Some("agent_x".into()),
                order: SessionOrder::Asc,
                ..Default::default()
            })
            .await;
        let ids: Vec<_> = page.items.iter().map(|m| m.session_id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[tokio::test]
    async fn list_filters_by_parent() {
        let r = InMemorySessionRegistry::new();
        let mut root = SessionMetadata::new("root");
        root.created_at = 100;
        let mut child1 = SessionMetadata::new("c1");
        child1.parent_session_id = Some("root".into());
        child1.created_at = 200;
        let mut child2 = SessionMetadata::new("c2");
        child2.parent_session_id = Some("root".into());
        child2.created_at = 300;

        r.insert(make_session("root"), root).await.unwrap();
        r.insert(make_session("c1"), child1).await.unwrap();
        r.insert(make_session("c2"), child2).await.unwrap();

        // Roots only
        let roots = r
            .list(&SessionFilter {
                parent_session_id: Some(None),
                ..Default::default()
            })
            .await;
        assert_eq!(roots.items.len(), 1);
        assert_eq!(roots.items[0].session_id, "root");

        // Specific parent
        let children = r
            .list(&SessionFilter {
                parent_session_id: Some(Some("root".into())),
                order: SessionOrder::Asc,
                ..Default::default()
            })
            .await;
        assert_eq!(children.items.len(), 2);
        assert_eq!(children.items[0].session_id, "c1");
    }

    #[tokio::test]
    async fn list_filters_by_created_at_range() {
        let r = InMemorySessionRegistry::new();
        for (id, t) in [("a", 100), ("b", 200), ("c", 300), ("d", 400)] {
            r.insert(make_session(id), meta(id, SessionStatus::Idle, t))
                .await
                .unwrap();
        }

        let page = r
            .list(&SessionFilter {
                created_after: Some(100),
                created_before: Some(400),
                order: SessionOrder::Asc,
                ..Default::default()
            })
            .await;
        let ids: Vec<_> = page.items.iter().map(|m| m.session_id.as_str()).collect();
        assert_eq!(ids, ["b", "c"], "bounds are exclusive");
    }

    #[tokio::test]
    async fn list_default_order_is_newest_first() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("a"), meta("a", SessionStatus::Idle, 100))
            .await
            .unwrap();
        r.insert(make_session("b"), meta("b", SessionStatus::Idle, 300))
            .await
            .unwrap();
        r.insert(make_session("c"), meta("c", SessionStatus::Idle, 200))
            .await
            .unwrap();

        let page = r.list(&SessionFilter::default()).await;
        let ids: Vec<_> = page.items.iter().map(|m| m.session_id.as_str()).collect();
        assert_eq!(ids, ["b", "c", "a"], "Desc default");
    }

    #[tokio::test]
    async fn list_pagination_via_cursor() {
        let r = InMemorySessionRegistry::new();
        for (id, t) in [("a", 100), ("b", 200), ("c", 300), ("d", 400), ("e", 500)] {
            r.insert(make_session(id), meta(id, SessionStatus::Idle, t))
                .await
                .unwrap();
        }

        // First page: 2 items, Asc.
        let first = r
            .list(&SessionFilter {
                order: SessionOrder::Asc,
                limit: 2,
                ..Default::default()
            })
            .await;
        let first_ids: Vec<_> = first.items.iter().map(|m| m.session_id.as_str()).collect();
        assert_eq!(first_ids, ["a", "b"]);
        let cursor = first.next_cursor.expect("page boundary");

        // Second page: resume.
        let second = r
            .list(&SessionFilter {
                order: SessionOrder::Asc,
                limit: 2,
                after: Some(cursor),
                ..Default::default()
            })
            .await;
        let second_ids: Vec<_> = second.items.iter().map(|m| m.session_id.as_str()).collect();
        assert_eq!(second_ids, ["c", "d"]);
        let cursor2 = second.next_cursor.expect("more pages");

        // Third page: just one item left, no next cursor.
        let third = r
            .list(&SessionFilter {
                order: SessionOrder::Asc,
                limit: 2,
                after: Some(cursor2),
                ..Default::default()
            })
            .await;
        let third_ids: Vec<_> = third.items.iter().map(|m| m.session_id.as_str()).collect();
        assert_eq!(third_ids, ["e"]);
        assert!(third.next_cursor.is_none());
    }

    #[tokio::test]
    async fn count_matches_list_size_for_same_filter() {
        let r = InMemorySessionRegistry::new();
        for (id, status, t) in [
            ("a", SessionStatus::Running, 100),
            ("b", SessionStatus::Idle, 200),
            ("c", SessionStatus::Running, 300),
            ("d", SessionStatus::Terminated, 400),
        ] {
            r.insert(make_session(id), meta(id, status, t)).await.unwrap();
        }
        let filter = SessionFilter {
            statuses: Some(vec![SessionStatus::Running, SessionStatus::Idle]),
            ..Default::default()
        };
        let page = r
            .list(&SessionFilter {
                limit: 0,
                ..filter.clone()
            })
            .await;
        assert_eq!(page.items.len(), r.count(&filter).await);
        assert_eq!(page.items.len(), 3);
    }

    #[tokio::test]
    async fn archived_excluded_by_default_in_count() {
        let r = InMemorySessionRegistry::new();
        for (id, t) in [("a", 100), ("b", 200), ("c", 300)] {
            r.insert(make_session(id), meta(id, SessionStatus::Idle, t))
                .await
                .unwrap();
        }
        r.archive("a").await.unwrap();
        r.archive("b").await.unwrap();

        assert_eq!(r.count(&SessionFilter::default()).await, 1);
        assert_eq!(
            r.count(&SessionFilter {
                include_archived: true,
                ..Default::default()
            })
            .await,
            3
        );
    }

    // -----------------------------------------------------------------------
    // Thread-tree enrichment (thread_view / thread_tree / primary_thread_for)
    //
    // These helpers project the spawn tree onto `SessionMetadata` by filling
    // `session_group_id` and `depth`. Raw metadata returned by `metadata()`
    // leaves both as `None`; enriched metadata returned by `thread_view`
    // sets both.
    // -----------------------------------------------------------------------

    fn child_meta(id: &str, parent: &str) -> SessionMetadata {
        let mut m = SessionMetadata::new(id);
        m.parent_session_id = Some(parent.into());
        m
    }

    #[tokio::test]
    async fn raw_metadata_has_no_enriched_tree_fields() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("root"), SessionMetadata::new("root"))
            .await
            .unwrap();
        let raw = r.metadata("root").await.unwrap();
        assert!(raw.session_group_id.is_none());
        assert!(raw.depth.is_none());
    }

    #[tokio::test]
    async fn thread_view_for_primary_has_zero_depth_and_self_group() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("root"), SessionMetadata::new("root"))
            .await
            .unwrap();

        let view = thread_view(&r, "root").await.unwrap();
        assert_eq!(view.session_id, "root");
        assert_eq!(view.parent_session_id, None);
        assert_eq!(view.session_group_id.as_deref(), Some("root"));
        assert_eq!(view.depth, Some(0));
    }

    #[tokio::test]
    async fn thread_view_for_child_resolves_group_root_and_depth() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("root"), SessionMetadata::new("root"))
            .await
            .unwrap();
        r.insert(make_session("c1"), child_meta("c1", "root")).await.unwrap();
        r.insert(make_session("g1"), child_meta("g1", "c1")).await.unwrap();

        let c1 = thread_view(&r, "c1").await.unwrap();
        assert_eq!(c1.session_group_id.as_deref(), Some("root"));
        assert_eq!(c1.depth, Some(1));

        let g1 = thread_view(&r, "g1").await.unwrap();
        assert_eq!(g1.session_group_id.as_deref(), Some("root"));
        assert_eq!(g1.depth, Some(2));
    }

    #[tokio::test]
    async fn thread_view_with_orphan_parent_falls_back_to_reachable_root() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("orphan"), child_meta("orphan", "ghost-parent"))
            .await
            .unwrap();
        let v = thread_view(&r, "orphan").await.unwrap();
        assert_eq!(v.session_group_id.as_deref(), Some("orphan"));
        assert_eq!(v.depth, Some(0));
    }

    #[tokio::test]
    async fn thread_tree_returns_root_then_bfs_all_enriched() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("root"), SessionMetadata::new("root"))
            .await
            .unwrap();
        r.insert(make_session("a"), child_meta("a", "root")).await.unwrap();
        r.insert(make_session("b"), child_meta("b", "root")).await.unwrap();
        r.insert(make_session("a1"), child_meta("a1", "a")).await.unwrap();
        r.insert(make_session("a2"), child_meta("a2", "a")).await.unwrap();

        let tree = thread_tree(&r, "root").await;
        let ids: Vec<_> = tree.iter().map(|m| m.session_id.as_str()).collect();
        assert_eq!(ids[0], "root");
        assert!(ids[1..3].contains(&"a"));
        assert!(ids[1..3].contains(&"b"));
        assert!(ids[3..].contains(&"a1"));
        assert!(ids[3..].contains(&"a2"));

        for m in &tree {
            assert_eq!(m.session_group_id.as_deref(), Some("root"));
            assert!(m.depth.is_some());
        }
    }

    #[tokio::test]
    async fn primary_thread_for_walks_up_to_root() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("root"), SessionMetadata::new("root"))
            .await
            .unwrap();
        r.insert(make_session("c1"), child_meta("c1", "root")).await.unwrap();
        r.insert(make_session("g1"), child_meta("g1", "c1")).await.unwrap();

        assert_eq!(primary_thread_for(&r, "g1").await.as_deref(), Some("root"));
        assert_eq!(primary_thread_for(&r, "c1").await.as_deref(), Some("root"));
        assert_eq!(primary_thread_for(&r, "root").await.as_deref(), Some("root"));
        assert_eq!(primary_thread_for(&r, "missing").await, None);
    }

    #[tokio::test]
    async fn record_usage_and_active_ms_visible_through_thread_view() {
        let r = InMemorySessionRegistry::new();
        r.insert(make_session("s"), SessionMetadata::new("s")).await.unwrap();

        let u = UsageMetadata {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            cache_creation_input_tokens: Some(10),
            cache_read_input_tokens: Some(20),
        };
        r.record_usage("s", &u).await.unwrap();
        r.record_usage("s", &u).await.unwrap();
        r.record_active_ms("s", 250).await.unwrap();

        let view = thread_view(&r, "s").await.unwrap();
        assert_eq!(view.usage.input_tokens, 200);
        assert_eq!(view.usage.output_tokens, 100);
        assert_eq!(view.usage.cache_read_input_tokens, 40);
        assert_eq!(view.stats.active_ms, 250);
    }
}
