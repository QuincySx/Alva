// INPUT:  ALVA workspace paths and the canonical filesystem Skill repository
// OUTPUT: Shared skill catalog with real discovery and enabled-state writes
// POS:    Hub skill management plane; mirrors the SkillsPlugin repository wiring

use std::path::Path;
use std::sync::Arc;

use alva_app_core::AlvaPaths;
use alva_protocol_skill::{fs::FsSkillRepository, repository::SkillRepository, types::Skill};

#[derive(Clone)]
pub struct SkillCatalog {
    repository: Arc<dyn SkillRepository>,
}

impl SkillCatalog {
    pub fn new(workspace: &Path) -> Self {
        let paths = AlvaPaths::new(workspace);
        let primary = paths.project_skills_dir();
        let repository = FsSkillRepository::new(
            primary.join("bundled"),
            primary.join("mbb"),
            primary.join("user"),
            primary.join("state.json"),
        )
        .with_extra_dirs(paths.skill_extra_dirs())
        .with_flat_primary(primary);
        Self {
            repository: Arc::new(repository),
        }
    }

    pub async fn list(&self) -> Result<Vec<Skill>, String> {
        let mut skills = self
            .repository
            .list_skills()
            .await
            .map_err(|error| error.to_string())?;
        skills.sort_by(|left, right| left.meta.name.cmp(&right.meta.name));
        Ok(skills)
    }

    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), String> {
        self.repository
            .set_enabled(name, enabled)
            .await
            .map_err(|error| error.to_string())
    }

    pub fn list_blocking(&self) -> Result<Vec<Skill>, String> {
        runtime()?.block_on(self.list())
    }

    pub fn set_enabled_blocking(&self, name: &str, enabled: bool) -> Result<(), String> {
        runtime()?.block_on(self.set_enabled(name, enabled))
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("start skill catalog runtime: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn catalog_uses_the_same_project_skill_tree_as_runtime() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join(".alva/skills/user/release-helper");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: release-helper\ndescription: Review release readiness\n---\nRun tests.\n",
        )
        .unwrap();

        let catalog = SkillCatalog::new(root.path());
        let skills = catalog.list().await.unwrap();
        assert!(skills
            .iter()
            .any(|skill| skill.meta.name == "release-helper"));
        catalog.set_enabled("release-helper", true).await.unwrap();
        assert!(catalog
            .list()
            .await
            .unwrap()
            .iter()
            .any(|skill| skill.meta.name == "release-helper" && skill.enabled));
    }
}
