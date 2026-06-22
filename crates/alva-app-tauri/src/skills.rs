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

#[cfg(test)]
mod tests {
    //! Tests for `scan_skills` — the Tauri-side entry point that the UI
    //! calls to populate its skill list. Built on `std::env::temp_dir()`
    //! + a unique pid/nanos-suffixed path so we stay hermetic without
    //! pulling tempfile into alva-app-tauri's deps (same pattern as
    //! sqlite_session/registry.rs).
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Build a unique directory under the OS temp dir. Caller is
    /// responsible for cleanup at the end of the test.
    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = format!(
            "alva-skills-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write a `SKILL.md` with valid frontmatter under `<base>/<name>/`.
    fn write_skill(base: &Path, name: &str, description: &str) {
        let skill_dir = base.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        let body =
            format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nbody.\n");
        fs::write(skill_dir.join("SKILL.md"), body).unwrap();
    }

    #[tokio::test]
    async fn scan_skills_returns_empty_for_nonexistent_dir() {
        // Path that demonstrably does not exist — scan_dir falls through
        // to vec![] on read_dir errors, list_skills returns Ok(empty),
        // scan_skills returns the empty list (NOT a panic).
        let bogus = std::env::temp_dir().join(format!(
            "alva-skills-does-not-exist-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        // Don't create it.
        let result = scan_skills(&bogus).await;
        assert!(
            result.is_empty(),
            "expected empty Vec, got {} items",
            result.len()
        );
    }

    #[tokio::test]
    async fn scan_skills_returns_empty_for_empty_subdirs() {
        // skill_dir exists but bundled / mbb / user are absent — same as
        // a freshly-created profile. Must NOT error or panic.
        let dir = unique_temp_dir("empty");
        let result = scan_skills(&dir).await;
        assert!(
            result.is_empty(),
            "expected empty Vec, got {} items",
            result.len()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn scan_skills_finds_bundled_skill_with_metadata() {
        let dir = unique_temp_dir("bundled");
        let bundled = dir.join("bundled");
        write_skill(&bundled, "alpha", "Alpha bundled skill");

        let result = scan_skills(&dir).await;
        assert_eq!(
            result.len(),
            1,
            "should find exactly one skill, got {}",
            result.len()
        );
        let s = &result[0];
        assert_eq!(s.name, "alpha");
        assert_eq!(s.description, "Alpha bundled skill");
        // Debug-format render of SkillKind::Bundled.
        assert_eq!(s.kind, "Bundled");
        // Bundled skills enable by default when no state.json overrides them.
        assert!(s.enabled, "bundled skill should default to enabled");
        // source_dir echoes the input path (UI uses this to group skills).
        assert_eq!(s.source_dir, dir.to_string_lossy());

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn scan_skills_includes_user_installed_with_correct_kind() {
        let dir = unique_temp_dir("user");
        let user = dir.join("user");
        write_skill(&user, "beta", "Beta user skill");

        let result = scan_skills(&dir).await;
        assert_eq!(
            result.len(),
            1,
            "should find exactly one skill, got {}",
            result.len()
        );
        let s = &result[0];
        assert_eq!(s.name, "beta");
        // Pinned regression guard: user-installed skill's kind serializes
        // as "UserInstalled" — the UI filter logic relies on the exact
        // Debug shape since this field crosses to TS as a plain string.
        assert_eq!(s.kind, "UserInstalled");

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn scan_skills_aggregates_bundled_and_user() {
        // Both buckets populated — scan_skills must return both in one
        // flat Vec, with distinct kinds.
        let dir = unique_temp_dir("mixed");
        let bundled = dir.join("bundled");
        let user = dir.join("user");
        write_skill(&bundled, "from-bundle", "B");
        write_skill(&user, "from-user", "U");

        let result = scan_skills(&dir).await;
        assert_eq!(
            result.len(),
            2,
            "should aggregate both, got {}",
            result.len()
        );
        let kinds_by_name: std::collections::HashMap<_, _> = result
            .iter()
            .map(|s| (s.name.as_str(), s.kind.as_str()))
            .collect();
        assert_eq!(kinds_by_name.get("from-bundle"), Some(&"Bundled"));
        assert_eq!(kinds_by_name.get("from-user"), Some(&"UserInstalled"));

        let _ = fs::remove_dir_all(&dir);
    }
}
