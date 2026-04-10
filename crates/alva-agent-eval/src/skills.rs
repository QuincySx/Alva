//! Skill discovery — scan filesystem for available skills.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use alva_protocol_skill::fs::FsSkillRepository;
use alva_protocol_skill::repository::SkillRepository;
use alva_protocol_skill::store::SkillStore;

#[derive(Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub kind: String,
    pub enabled: bool,
    pub source_dir: String,
}

#[derive(Serialize)]
pub struct SkillSourceInfo {
    pub path: String,
    pub exists: bool,
    pub label: String,
}

/// Discover standard skill source directories.
pub fn discover_skill_sources() -> Vec<SkillSourceInfo> {
    let mut sources = Vec::new();

    // Global: ~/.config/alva/skills/
    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("alva").join("skills");
        sources.push(SkillSourceInfo {
            exists: path.exists(),
            path: path.to_string_lossy().to_string(),
            label: "Global ~/.config/alva/skills".into(),
        });
    }

    // Claude: ~/.claude/skills/
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".claude").join("skills");
        sources.push(SkillSourceInfo {
            exists: path.exists(),
            path: path.to_string_lossy().to_string(),
            label: "Claude ~/.claude/skills".into(),
        });
    }

    sources
}

/// Scan a skill directory and return all found skills.
pub async fn scan_skills(skill_dir: &Path) -> Vec<SkillInfo> {
    let bundled = skill_dir.join("bundled");
    let mbb = skill_dir.join("mbb");
    let user = skill_dir.join("user");
    let state_file = skill_dir.join("state.json");

    let repo = Arc::new(FsSkillRepository::new(bundled, mbb, user, state_file));
    let store = SkillStore::new(repo as Arc<dyn SkillRepository>);

    if store.scan().await.is_err() {
        return vec![];
    }

    let source = skill_dir.to_string_lossy().to_string();
    store
        .list()
        .await
        .into_iter()
        .map(|s| SkillInfo {
            name: s.meta.name,
            description: s.meta.description,
            kind: format!("{:?}", s.kind),
            enabled: s.enabled,
            source_dir: source.clone(),
        })
        .collect()
}
