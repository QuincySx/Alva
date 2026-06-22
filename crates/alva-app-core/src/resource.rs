// INPUT:  async_trait, serde, std::collections, tokio::sync::RwLock, std::sync::atomic
// OUTPUT: SessionResource, ResourceKind, ResourceAccess, RepoCheckout, ResourceParams,
//         ResourcePatch, ResourceFilter, ResourceError, ResourceRegistry,
//         InMemoryResourceRegistry, render_resource_instructions
// POS:    **Harness-level** mountable-resource registry. This abstraction is intentionally
//         NOT in `alva-kernel-abi`/`alva-agent-core` — the agent loop doesn't mount resources,
//         tools / extensions do. It lives here in `alva-app-core` so our own apps
//         (alva-app-cli / alva-app-tauri) can expose Anthropic Managed Agents-style
//         `/v1/sessions/:id/resources` endpoints without polluting the SDK. Third-party
//         harnesses are free to skip this and roll their own resource bookkeeping.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// ===========================================================================
// Resource shape
// ===========================================================================

/// How a mutable resource (today: `MemoryStore`) is exposed to the agent.
/// Mirrors Anthropic Managed Agents `access` on memory store resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAccess {
    /// Default — the agent can read and write.
    ReadWrite,
    /// The agent can only read; mutating tools should refuse.
    ReadOnly,
}

impl Default for ResourceAccess {
    fn default() -> Self {
        Self::ReadWrite
    }
}

/// Which Git revision to check out at mount time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RepoCheckout {
    Branch { name: String },
    Commit { sha: String },
}

/// What this mount entry actually is. Mirrors Anthropic Managed Agents
/// `BetaManagedAgentsSessionResource` (file / github_repository /
/// memory_store) and adds a `Skill` variant for alva's progressive-skill
/// loading.
///
/// `*_id` fields are external references — the SDK subsystem that owns
/// the bytes (Files API, MemoryStore registry, skill repository, Git
/// remote) is responsible for the actual data. The mount entry just
/// records the binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceKind {
    File {
        file_id: String,
    },
    GitHubRepository {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkout: Option<RepoCheckout>,
        /// Clone credential. Persistence layers should redact before storage.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authorization_token: Option<String>,
    },
    MemoryStore {
        memory_store_id: String,
    },
    Skill {
        skill_id: String,
        /// Pinned skill version. `None` resolves to "latest".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<u32>,
    },
}

impl ResourceKind {
    /// Stable short tag for `ResourceFilter::kind_tag` matching and logging.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::File { .. } => "file",
            Self::GitHubRepository { .. } => "github_repository",
            Self::MemoryStore { .. } => "memory_store",
            Self::Skill { .. } => "skill",
        }
    }
}

/// One mount entry on a session. Created via `ResourceRegistry::add`;
/// the registry assigns `id`, `created_at`, and `updated_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResource {
    /// Stable id assigned by the registry. Independent of any per-kind
    /// underlying id — the same file can be mounted twice with different
    /// `mount_path` / `access`, each producing its own SessionResource.
    pub id: String,

    pub session_id: String,
    pub kind: ResourceKind,

    /// Container mount path. `None` = "kind-specific default".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount_path: Option<String>,

    pub access: ResourceAccess,

    /// Per-attachment guidance, rendered into the system prompt by
    /// `render_resource_instructions`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    /// Snapshot of underlying resource description at attach time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    pub created_at: i64,
    pub updated_at: i64,
}

// ===========================================================================
// Params / Patch / Filter
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ResourceParams {
    pub kind: ResourceKind,
    pub mount_path: Option<String>,
    pub access: ResourceAccess,
    pub instructions: Option<String>,
    pub description: Option<String>,
}

impl ResourceParams {
    pub fn new(kind: ResourceKind) -> Self {
        Self {
            kind,
            mount_path: None,
            access: ResourceAccess::default(),
            instructions: None,
            description: None,
        }
    }

    pub fn mount_path(mut self, path: impl Into<String>) -> Self {
        self.mount_path = Some(path.into());
        self
    }

    pub fn access(mut self, access: ResourceAccess) -> Self {
        self.access = access;
        self
    }

    pub fn instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = Some(text.into());
        self
    }

    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }
}

/// Partial-update payload. `Option<Option<T>>` convention: `None` = leave
/// alone, `Some(None)` = clear, `Some(Some(v))` = set. `kind` is NOT
/// patchable — changing the binding is delete + add, not in-place.
#[derive(Debug, Clone, Default)]
pub struct ResourcePatch {
    pub mount_path: Option<Option<String>>,
    pub access: Option<ResourceAccess>,
    pub instructions: Option<Option<String>>,
    pub description: Option<Option<String>>,
}

impl ResourcePatch {
    pub fn mount_path(mut self, path: impl Into<String>) -> Self {
        self.mount_path = Some(Some(path.into()));
        self
    }

    pub fn clear_mount_path(mut self) -> Self {
        self.mount_path = Some(None);
        self
    }

    pub fn access(mut self, access: ResourceAccess) -> Self {
        self.access = Some(access);
        self
    }

    pub fn instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = Some(Some(text.into()));
        self
    }

    pub fn clear_instructions(mut self) -> Self {
        self.instructions = Some(None);
        self
    }

    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(Some(text.into()));
        self
    }

    pub fn clear_description(mut self) -> Self {
        self.description = Some(None);
        self
    }
}

/// Filter for `ResourceRegistry::list_session`.
#[derive(Debug, Clone, Default)]
pub struct ResourceFilter {
    /// Filter to a single kind tag — `"file"` / `"github_repository"` /
    /// `"memory_store"` / `"skill"`.
    pub kind_tag: Option<String>,
}

// ===========================================================================
// Errors
// ===========================================================================

#[derive(Debug, thiserror::Error)]
pub enum ResourceError {
    #[error("resource not found: {0}")]
    NotFound(String),
    #[error("resource error: {0}")]
    Other(String),
}

// ===========================================================================
// Trait
// ===========================================================================

/// Session-scoped resource collection. Maps onto Anthropic Managed Agents
/// `Sessions.resources.{add,retrieve,update,list,delete}`.
///
/// The registry is **independent of** the SDK subsystems that own the
/// bytes (Files API / memory backend / skill repository / Git). Adding
/// here records the **attachment** — App-layer code performs the actual
/// mount (downloading the repo, loading the skill, etc.) using the
/// variant-specific data in `ResourceKind`.
///
/// This split mirrors Anthropic's architecture, where the resource
/// record and the underlying object live in separate subsystems.
#[async_trait]
pub trait ResourceRegistry: Send + Sync {
    async fn add(
        &self,
        session_id: &str,
        params: ResourceParams,
    ) -> Result<SessionResource, ResourceError>;

    async fn retrieve(&self, resource_id: &str) -> Option<SessionResource>;

    async fn update(&self, resource_id: &str, patch: ResourcePatch) -> Result<(), ResourceError>;

    async fn list_session(&self, session_id: &str, filter: &ResourceFilter)
        -> Vec<SessionResource>;

    async fn delete(&self, resource_id: &str) -> Result<(), ResourceError>;
}

// ===========================================================================
// Prompt rendering helper
// ===========================================================================

/// Render every resource's `instructions` / `description` into a
/// system-prompt block — mirrors Anthropic injecting per-attachment
/// guidance into the agent's prompt. Returns the empty string when the
/// session has no resources with text.
pub async fn render_resource_instructions(
    registry: &dyn ResourceRegistry,
    session_id: &str,
) -> String {
    let resources = registry
        .list_session(session_id, &ResourceFilter::default())
        .await;
    let with_text: Vec<&SessionResource> = resources
        .iter()
        .filter(|r| r.instructions.is_some() || r.description.is_some())
        .collect();
    if with_text.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Session Resources\n\n");
    for r in with_text {
        let mount = r.mount_path.as_deref().unwrap_or("(default mount)");
        let access = match r.access {
            ResourceAccess::ReadWrite => "read-write",
            ResourceAccess::ReadOnly => "read-only",
        };
        out.push_str(&format!("### {} ({}, {})\n", r.kind.tag(), mount, access,));
        if let Some(desc) = &r.description {
            out.push_str(desc);
            out.push('\n');
        }
        if let Some(instr) = &r.instructions {
            out.push_str(instr);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

// ===========================================================================
// InMemoryResourceRegistry
// ===========================================================================

/// In-memory reference implementation. Process-lifetime storage; persistent
/// backends (SQLite, REST-backed remote) implement `ResourceRegistry`
/// against their own store. Id format `rsrc_<hex>` mirrors Anthropic's
/// `sesrsc_…` style.
pub struct InMemoryResourceRegistry {
    by_id: RwLock<HashMap<String, SessionResource>>,
    id_counter: AtomicU64,
}

impl InMemoryResourceRegistry {
    pub fn new() -> Self {
        Self {
            by_id: RwLock::new(HashMap::new()),
            id_counter: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> String {
        let n = self.id_counter.fetch_add(1, Ordering::SeqCst);
        format!("rsrc_{n:08x}")
    }
}

impl Default for InMemoryResourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ResourceRegistry for InMemoryResourceRegistry {
    async fn add(
        &self,
        session_id: &str,
        params: ResourceParams,
    ) -> Result<SessionResource, ResourceError> {
        let now = chrono::Utc::now().timestamp_millis();
        let resource = SessionResource {
            id: self.next_id(),
            session_id: session_id.to_string(),
            kind: params.kind,
            mount_path: params.mount_path,
            access: params.access,
            instructions: params.instructions,
            description: params.description,
            created_at: now,
            updated_at: now,
        };
        self.by_id
            .write()
            .await
            .insert(resource.id.clone(), resource.clone());
        Ok(resource)
    }

    async fn retrieve(&self, resource_id: &str) -> Option<SessionResource> {
        self.by_id.read().await.get(resource_id).cloned()
    }

    async fn update(&self, resource_id: &str, patch: ResourcePatch) -> Result<(), ResourceError> {
        let mut entries = self.by_id.write().await;
        let entry = entries
            .get_mut(resource_id)
            .ok_or_else(|| ResourceError::NotFound(resource_id.to_string()))?;
        if let Some(mp) = patch.mount_path {
            entry.mount_path = mp;
        }
        if let Some(access) = patch.access {
            entry.access = access;
        }
        if let Some(inst) = patch.instructions {
            entry.instructions = inst;
        }
        if let Some(desc) = patch.description {
            entry.description = desc;
        }
        entry.updated_at = chrono::Utc::now().timestamp_millis();
        Ok(())
    }

    async fn list_session(
        &self,
        session_id: &str,
        filter: &ResourceFilter,
    ) -> Vec<SessionResource> {
        let entries = self.by_id.read().await;
        let mut out: Vec<SessionResource> = entries
            .values()
            .filter(|r| r.session_id == session_id)
            .filter(|r| {
                filter
                    .kind_tag
                    .as_deref()
                    .is_none_or(|tag| r.kind.tag() == tag)
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        out
    }

    async fn delete(&self, resource_id: &str) -> Result<(), ResourceError> {
        let mut entries = self.by_id.write().await;
        if entries.remove(resource_id).is_none() {
            return Err(ResourceError::NotFound(resource_id.to_string()));
        }
        Ok(())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn file_kind(id: &str) -> ResourceKind {
        ResourceKind::File {
            file_id: id.to_string(),
        }
    }

    fn memory_kind(id: &str) -> ResourceKind {
        ResourceKind::MemoryStore {
            memory_store_id: id.to_string(),
        }
    }

    fn skill_kind(id: &str, ver: Option<u32>) -> ResourceKind {
        ResourceKind::Skill {
            skill_id: id.to_string(),
            version: ver,
        }
    }

    fn repo_kind(url: &str) -> ResourceKind {
        ResourceKind::GitHubRepository {
            url: url.to_string(),
            checkout: Some(RepoCheckout::Branch {
                name: "main".to_string(),
            }),
            authorization_token: Some("ghp_secret".to_string()),
        }
    }

    #[test]
    fn kind_tag_matches_anthropic_strings() {
        assert_eq!(file_kind("f").tag(), "file");
        assert_eq!(repo_kind("u").tag(), "github_repository");
        assert_eq!(memory_kind("m").tag(), "memory_store");
        assert_eq!(skill_kind("s", None).tag(), "skill");
    }

    #[test]
    fn default_access_is_read_write() {
        assert_eq!(ResourceAccess::default(), ResourceAccess::ReadWrite);
    }

    #[tokio::test]
    async fn add_assigns_id_and_timestamps() {
        let r = InMemoryResourceRegistry::new();
        let res = r
            .add("sesn-1", ResourceParams::new(file_kind("f-1")))
            .await
            .unwrap();
        assert!(res.id.starts_with("rsrc_"));
        assert_eq!(res.session_id, "sesn-1");
        assert_eq!(res.created_at, res.updated_at);
        assert!(res.created_at > 0);
    }

    #[tokio::test]
    async fn retrieve_roundtrips() {
        let r = InMemoryResourceRegistry::new();
        let added = r
            .add(
                "sesn-1",
                ResourceParams::new(memory_kind("memstore-1"))
                    .mount_path("/mnt/memory/notes")
                    .access(ResourceAccess::ReadOnly)
                    .instructions("Use this store to recall earlier conversations.")
                    .description("user-preferences"),
            )
            .await
            .unwrap();

        let got = r.retrieve(&added.id).await.unwrap();
        assert_eq!(got, added);
        assert_eq!(got.access, ResourceAccess::ReadOnly);
        assert_eq!(got.mount_path.as_deref(), Some("/mnt/memory/notes"));
    }

    #[tokio::test]
    async fn update_patches_selected_fields_only() {
        let r = InMemoryResourceRegistry::new();
        let added = r
            .add(
                "sesn-1",
                ResourceParams::new(file_kind("f-1"))
                    .mount_path("/mnt/uploads/f-1")
                    .instructions("first"),
            )
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(2)).await;

        r.update(
            &added.id,
            ResourcePatch::default()
                .instructions("revised")
                .access(ResourceAccess::ReadOnly),
        )
        .await
        .unwrap();

        let got = r.retrieve(&added.id).await.unwrap();
        assert_eq!(got.instructions.as_deref(), Some("revised"));
        assert_eq!(got.access, ResourceAccess::ReadOnly);
        assert_eq!(got.mount_path.as_deref(), Some("/mnt/uploads/f-1"));
        assert!(got.updated_at > got.created_at);
    }

    #[tokio::test]
    async fn update_can_clear_optional_fields() {
        let r = InMemoryResourceRegistry::new();
        let added = r
            .add(
                "sesn-1",
                ResourceParams::new(file_kind("f-1"))
                    .mount_path("/mnt/x")
                    .instructions("init")
                    .description("init"),
            )
            .await
            .unwrap();

        r.update(
            &added.id,
            ResourcePatch::default()
                .clear_mount_path()
                .clear_instructions()
                .clear_description(),
        )
        .await
        .unwrap();

        let got = r.retrieve(&added.id).await.unwrap();
        assert!(got.mount_path.is_none());
        assert!(got.instructions.is_none());
        assert!(got.description.is_none());
    }

    #[tokio::test]
    async fn update_missing_resource_errors() {
        let r = InMemoryResourceRegistry::new();
        let err = r
            .update("missing", ResourcePatch::default().instructions("x"))
            .await
            .unwrap_err();
        assert!(matches!(err, ResourceError::NotFound(_)));
    }

    #[tokio::test]
    async fn list_session_returns_only_that_sessions_resources_ordered() {
        let r = InMemoryResourceRegistry::new();
        let _a = r
            .add("sesn-1", ResourceParams::new(file_kind("f-a")))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let _b = r
            .add("sesn-2", ResourceParams::new(file_kind("f-b")))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let _c = r
            .add("sesn-1", ResourceParams::new(memory_kind("memstore-1")))
            .await
            .unwrap();

        let on_1 = r.list_session("sesn-1", &ResourceFilter::default()).await;
        let kinds: Vec<_> = on_1.iter().map(|r| r.kind.tag()).collect();
        assert_eq!(kinds, ["file", "memory_store"]);

        let on_2 = r.list_session("sesn-2", &ResourceFilter::default()).await;
        assert_eq!(on_2.len(), 1);

        let missing = r.list_session("ghost", &ResourceFilter::default()).await;
        assert!(missing.is_empty());
    }

    #[tokio::test]
    async fn list_session_filters_by_kind_tag() {
        let r = InMemoryResourceRegistry::new();
        r.add("sesn", ResourceParams::new(file_kind("f")))
            .await
            .unwrap();
        r.add("sesn", ResourceParams::new(memory_kind("m1")))
            .await
            .unwrap();
        r.add("sesn", ResourceParams::new(memory_kind("m2")))
            .await
            .unwrap();
        r.add("sesn", ResourceParams::new(skill_kind("s", Some(3))))
            .await
            .unwrap();

        let mems = r
            .list_session(
                "sesn",
                &ResourceFilter {
                    kind_tag: Some("memory_store".into()),
                },
            )
            .await;
        assert_eq!(mems.len(), 2);
        for m in &mems {
            assert_eq!(m.kind.tag(), "memory_store");
        }

        let skills = r
            .list_session(
                "sesn",
                &ResourceFilter {
                    kind_tag: Some("skill".into()),
                },
            )
            .await;
        assert_eq!(skills.len(), 1);
        match &skills[0].kind {
            ResourceKind::Skill { skill_id, version } => {
                assert_eq!(skill_id, "s");
                assert_eq!(*version, Some(3));
            }
            _ => panic!("expected skill"),
        }
    }

    #[tokio::test]
    async fn delete_removes_record() {
        let r = InMemoryResourceRegistry::new();
        let added = r
            .add("sesn", ResourceParams::new(file_kind("f")))
            .await
            .unwrap();

        r.delete(&added.id).await.unwrap();
        assert!(r.retrieve(&added.id).await.is_none());
        assert!(r
            .list_session("sesn", &ResourceFilter::default())
            .await
            .is_empty());

        let err = r.delete(&added.id).await.unwrap_err();
        assert!(matches!(err, ResourceError::NotFound(_)));
    }

    #[tokio::test]
    async fn render_resource_instructions_skips_empty_and_orders_by_creation() {
        let r = InMemoryResourceRegistry::new();
        r.add(
            "sesn",
            ResourceParams::new(file_kind("f")).instructions("Read F when asked"),
        )
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        r.add("sesn", ResourceParams::new(memory_kind("m1")))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        r.add(
            "sesn",
            ResourceParams::new(memory_kind("m2")).description("user-preferences"),
        )
        .await
        .unwrap();

        let rendered = render_resource_instructions(&r, "sesn").await;
        assert!(rendered.contains("## Session Resources"));
        assert!(rendered.contains("file"));
        assert!(rendered.contains("Read F when asked"));
        assert!(rendered.contains("memory_store"));
        assert!(rendered.contains("user-preferences"));
        let file_at = rendered.find("file (").unwrap();
        let mem_at = rendered.find("memory_store (").unwrap();
        assert!(file_at < mem_at);
    }

    #[tokio::test]
    async fn render_returns_empty_when_no_resources() {
        let r = InMemoryResourceRegistry::new();
        assert!(render_resource_instructions(&r, "sesn").await.is_empty());
    }

    #[tokio::test]
    async fn render_returns_empty_when_no_text() {
        let r = InMemoryResourceRegistry::new();
        r.add("sesn", ResourceParams::new(file_kind("f")))
            .await
            .unwrap();
        assert!(render_resource_instructions(&r, "sesn").await.is_empty());
    }

    #[test]
    fn serde_round_trip_for_each_kind() {
        for kind in [
            file_kind("file-1"),
            memory_kind("memstore-1"),
            skill_kind("skill-x", Some(2)),
            repo_kind("https://github.com/anthropics/anthropic-sdk-typescript"),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let back: ResourceKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }
}
