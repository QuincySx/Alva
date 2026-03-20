use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    skills::skill_domain::skill::{Skill, SkillKind, SkillMeta},
    error::SkillError,
    skills::skill_ports::skill_repository::{SkillInstallSource, SkillRepository},
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
