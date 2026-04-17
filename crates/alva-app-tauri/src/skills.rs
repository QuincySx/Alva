// INPUT:  alva_protocol_skill (FsSkillRepository, SkillRepository, SkillStore), dirs
// OUTPUT: SkillSourceInfo / SkillInfo + discover_skill_sources / scan_skills helpers
// POS:    Skill discovery primitives for the Tauri shell. Ported from
//         alva-app-eval::skills so the Tauri UI can list skills without
//         depending on the eval crate.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use alva_protocol_skill::fs::FsSkillRepository;
use alva_protocol_skill::repository::SkillRepository;
use alva_protocol_skill::store::SkillStore;

#[derive(Serialize, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub kind: String,
    pub enabled: bool,
    pub source_dir: String,
}

#[derive(Serialize, Clone)]
pub struct SkillSourceInfo {
    pub path: String,
    pub exists: bool,
    pub label: String,
}

pub fn discover_skill_sources() -> Vec<SkillSourceInfo> {
    let mut sources = Vec::new();

    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("alva").join("skills");
        sources.push(SkillSourceInfo {
            exists: path.exists(),
            path: path.to_string_lossy().to_string(),
            label: "Global ~/.config/alva/skills".into(),
        });
    }

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
