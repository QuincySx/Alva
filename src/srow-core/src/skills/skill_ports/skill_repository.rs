use crate::skills::skill_domain::skill::{Skill, SkillBody, SkillMeta, SkillResource};
use crate::error::SkillError;
use async_trait::async_trait;

/// Skill repository interface: abstracts underlying storage (filesystem / zip / network)
#[async_trait]
pub trait SkillRepository: Send + Sync {
    /// List all known Skills (metadata only, Level 1)
    async fn list_skills(&self) -> Result<Vec<Skill>, SkillError>;

    /// Find Skill by name (metadata only)
    async fn find_skill(&self, name: &str) -> Result<Option<Skill>, SkillError>;

    /// Load Skill instruction layer (Level 2 -- SKILL.md body)
    async fn load_body(&self, name: &str) -> Result<SkillBody, SkillError>;

    /// Load Skill resource file (Level 3 -- on demand)
    async fn load_resource(
        &self,
        name: &str,
        relative_path: &str,
    ) -> Result<SkillResource, SkillError>;

    /// List all resource paths for a Skill (without loading content)
    async fn list_resources(&self, name: &str) -> Result<Vec<String>, SkillError>;

    /// Install Skill (from local path or .zip file)
    async fn install(&self, source: SkillInstallSource) -> Result<SkillMeta, SkillError>;

    /// Remove Skill (only UserInstalled type can be removed)
    async fn remove(&self, name: &str) -> Result<(), SkillError>;

    /// Set Skill enabled/disabled state
    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), SkillError>;
}

/// Skill installation source
pub enum SkillInstallSource {
    /// Local directory path (directory containing SKILL.md)
    LocalDir(std::path::PathBuf),
    /// Local .zip file path
    LocalZip(std::path::PathBuf),
    /// Remote URL (.zip file, HTTPS)
    RemoteUrl(String),
}
