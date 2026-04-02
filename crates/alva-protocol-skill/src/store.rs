// INPUT:  std::sync, tokio::sync, crate::types, crate::error, crate::repository
// OUTPUT: SkillStore
// POS:    In-memory Skill index cache with scan, search, install, remove, and enable/disable operations.
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    error::SkillError,
    repository::{SkillInstallSource, SkillRepository},
    types::{Skill, SkillKind, SkillMeta},
};

/// Skill store: index cache + unified access entry point
/// Scans all Skill directories at app startup, maintains in-memory index
pub struct SkillStore {
    repo: Arc<dyn SkillRepository>,
    /// In-memory index (name -> Skill), populated after first scan
    cache: Arc<RwLock<Vec<Skill>>>,
}

impl SkillStore {
    pub fn new(repo: Arc<dyn SkillRepository>) -> Self {
        Self {
            repo,
            cache: Arc::new(RwLock::new(vec![])),
        }
    }

    /// Scan all Skill directories, populate in-memory index
    /// Called once at app startup
    pub async fn scan(&self) -> Result<(), SkillError> {
        let skills = self.repo.list_skills().await?;
        let mut cache = self.cache.write().await;
        *cache = skills;
        Ok(())
    }

    /// Query all Skills (with metadata and enabled state)
    pub async fn list(&self) -> Vec<Skill> {
        self.cache.read().await.clone()
    }

    /// Find by name (enabled only)
    pub async fn find_enabled(&self, name: &str) -> Option<Skill> {
        self.cache
            .read()
            .await
            .iter()
            .find(|s| s.meta.name == name && s.enabled)
            .cloned()
    }

    /// Find by name (any state)
    pub async fn find(&self, name: &str) -> Option<Skill> {
        self.cache
            .read()
            .await
            .iter()
            .find(|s| s.meta.name == name)
            .cloned()
    }

    /// Find MBB Skills by domain
    pub async fn find_mbb_by_domain(&self, domain: &str) -> Vec<Skill> {
        self.cache
            .read()
            .await
            .iter()
            .filter(|s| {
                s.enabled
                    && matches!(&s.kind, SkillKind::Mbb { domains } if
                        domains.iter().any(|d| domain.ends_with(d.as_str())))
            })
            .cloned()
            .collect()
    }

    /// Search skills by query (simple substring match on name + description)
    pub async fn search(&self, query: &str) -> Vec<Skill> {
        let q = query.to_lowercase();
        self.cache
            .read()
            .await
            .iter()
            .filter(|s| {
                s.enabled
                    && (s.meta.name.to_lowercase().contains(&q)
                        || s.meta.description.to_lowercase().contains(&q))
            })
            .cloned()
            .collect()
    }

    /// Install new Skill
    pub async fn install(&self, source: SkillInstallSource) -> Result<SkillMeta, SkillError> {
        let meta = self.repo.install(source).await?;
        // Re-scan to update index
        self.scan().await?;
        Ok(meta)
    }

    /// Remove Skill (only UserInstalled)
    pub async fn remove(&self, name: &str) -> Result<(), SkillError> {
        // Validate: Bundled Skills cannot be removed
        {
            let cache = self.cache.read().await;
            if let Some(skill) = cache.iter().find(|s| s.meta.name == name) {
                if matches!(skill.kind, SkillKind::Bundled) {
                    return Err(SkillError::CannotRemoveBundledSkill(name.to_string()));
                }
            }
        }
        self.repo.remove(name).await?;
        self.scan().await?;
        Ok(())
    }

    /// Enable/disable Skill
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), SkillError> {
        self.repo.set_enabled(name, enabled).await?;
        let mut cache = self.cache.write().await;
        if let Some(skill) = cache.iter_mut().find(|s| s.meta.name == name) {
            skill.enabled = enabled;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{InMemorySkill, InMemorySkillRepository};
    use crate::types::SkillMeta;

    fn make_skill(name: &str, enabled: bool) -> InMemorySkill {
        InMemorySkill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} does things"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::Bundled,
            body: format!("# {name}\nInstructions here."),
            resources: vec![],
            enabled,
        }
    }

    fn make_mbb_skill(name: &str, domains: Vec<String>) -> InMemorySkill {
        InMemorySkill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} MBB skill"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::Mbb { domains },
            body: "MBB body".into(),
            resources: vec![],
            enabled: true,
        }
    }

    fn make_user_skill(name: &str) -> InMemorySkill {
        InMemorySkill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} user skill"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::UserInstalled,
            body: "User body".into(),
            resources: vec![],
            enabled: true,
        }
    }

    fn make_store(skills: Vec<InMemorySkill>) -> SkillStore {
        SkillStore::new(Arc::new(InMemorySkillRepository::new(skills)))
    }

    // ── scan ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn scan_populates_cache() {
        let store = make_store(vec![
            make_skill("alpha", true),
            make_skill("beta", true),
        ]);

        // Before scan, list is empty
        assert!(store.list().await.is_empty());

        store.scan().await.unwrap();
        assert_eq!(store.list().await.len(), 2);
    }

    #[tokio::test]
    async fn scan_empty_repo() {
        let store = make_store(vec![]);
        store.scan().await.unwrap();
        assert!(store.list().await.is_empty());
    }

    // ── find ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn find_returns_skill_regardless_of_enabled() {
        let store = make_store(vec![
            make_skill("enabled-one", true),
            make_skill("disabled-one", false),
        ]);
        store.scan().await.unwrap();

        assert!(store.find("enabled-one").await.is_some());
        assert!(store.find("disabled-one").await.is_some());
        assert!(store.find("missing").await.is_none());
    }

    // ── find_enabled ────────────────────────────────────────────────────

    #[tokio::test]
    async fn find_enabled_only_returns_enabled() {
        let store = make_store(vec![
            make_skill("enabled-one", true),
            make_skill("disabled-one", false),
        ]);
        store.scan().await.unwrap();

        assert!(store.find_enabled("enabled-one").await.is_some());
        assert!(store.find_enabled("disabled-one").await.is_none());
    }

    // ── search ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn search_matches_name() {
        let store = make_store(vec![
            make_skill("code-review", true),
            make_skill("browser-auto", true),
            make_skill("disabled-skill", false),
        ]);
        store.scan().await.unwrap();

        let results = store.search("browser").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meta.name, "browser-auto");
    }

    #[tokio::test]
    async fn search_matches_description() {
        let store = make_store(vec![make_skill("foo", true)]);
        store.scan().await.unwrap();

        // Description is "foo does things"
        let results = store.search("does things").await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn search_is_case_insensitive() {
        let store = make_store(vec![make_skill("MySkill", true)]);
        store.scan().await.unwrap();

        let results = store.search("myskill").await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn search_excludes_disabled() {
        let store = make_store(vec![make_skill("hidden", false)]);
        store.scan().await.unwrap();

        let results = store.search("hidden").await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn search_no_match() {
        let store = make_store(vec![make_skill("alpha", true)]);
        store.scan().await.unwrap();

        let results = store.search("zzz_no_match").await;
        assert!(results.is_empty());
    }

    // ── find_mbb_by_domain ──────────────────────────────────────────────

    #[tokio::test]
    async fn find_mbb_by_domain_exact_match() {
        let store = make_store(vec![make_mbb_skill(
            "train-helper",
            vec!["12306.cn".into()],
        )]);
        store.scan().await.unwrap();

        let results = store.find_mbb_by_domain("12306.cn").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].meta.name, "train-helper");
    }

    #[tokio::test]
    async fn find_mbb_by_domain_suffix_match() {
        let store = make_store(vec![make_mbb_skill(
            "sub-domain",
            vec!["example.com".into()],
        )]);
        store.scan().await.unwrap();

        let results = store.find_mbb_by_domain("sub.example.com").await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn find_mbb_by_domain_no_match() {
        let store = make_store(vec![make_mbb_skill(
            "train-helper",
            vec!["12306.cn".into()],
        )]);
        store.scan().await.unwrap();

        let results = store.find_mbb_by_domain("other.com").await;
        assert!(results.is_empty());
    }

    // ── set_enabled ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn set_enabled_toggles_state() {
        let store = make_store(vec![make_skill("alpha", true)]);
        store.scan().await.unwrap();

        assert!(store.find_enabled("alpha").await.is_some());

        store.set_enabled("alpha", false).await.unwrap();
        assert!(store.find_enabled("alpha").await.is_none());
        // But find() still returns it
        assert!(store.find("alpha").await.is_some());

        store.set_enabled("alpha", true).await.unwrap();
        assert!(store.find_enabled("alpha").await.is_some());
    }

    // ── remove ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn remove_bundled_skill_returns_error() {
        let store = make_store(vec![make_skill("bundled", true)]);
        store.scan().await.unwrap();

        let err = store.remove("bundled").await.unwrap_err();
        assert!(matches!(err, SkillError::CannotRemoveBundledSkill(_)));
    }

    #[tokio::test]
    async fn remove_user_skill_delegates_to_repo() {
        // InMemorySkillRepository does not support remove, so we expect an Io error
        let store = make_store(vec![make_user_skill("custom")]);
        store.scan().await.unwrap();

        let err = store.remove("custom").await.unwrap_err();
        // InMemorySkillRepository returns Io error for remove
        assert!(matches!(err, SkillError::Io(_)));
    }

    // ── install ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn install_unsupported_in_memory() {
        let store = make_store(vec![]);
        store.scan().await.unwrap();

        let err = store
            .install(crate::repository::SkillInstallSource::LocalDir("/tmp/x".into()))
            .await
            .unwrap_err();
        assert!(matches!(err, SkillError::Io(_)));
    }
}
