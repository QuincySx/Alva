// INPUT:  std::{fs, path}, include_dir, dirs
// OUTPUT: ensure_extracted, BUNDLED_SKILLS_VERSION
// POS:    Bundles the workspace-root skills tree (`assets/skills/`) into the
//         binary at compile time and extracts it on first run to a versioned
//         cache directory. The path is fed into SkillsPlugin so agents see
//         the bundled skills alongside user/project ones.

//! Built-in skill bundling.
//!
//! Skills shipped with the App live at the workspace root under
//! `assets/skills/` — pure static markdown, not a Cargo crate, because
//! that's what they are. Both `alva-app-cli` and `alva-app-tauri` embed
//! the same tree via `include_dir!` reaching up two levels from their
//! respective `CARGO_MANIFEST_DIR`.
//!
//! On first launch (or when the crate version changes) the tree is
//! extracted to `<cache_dir>/alva/bundled-skills-v<VERSION>/`. The path
//! is passed to `SkillsPlugin` so the agent's skill loader picks them
//! up like any other directory tree.
//!
//! Why a versioned cache dir: cleanly invalidates on upgrade — old
//! bundled skills don't shadow new ones — without needing extraction
//! logic to know about every file individually.

use std::fs;
use std::path::PathBuf;

use include_dir::{include_dir, Dir};

// Workspace layout: `crates/alva-app-cli/Cargo.toml` → `../../assets/skills`.
const BUNDLED_SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../assets/skills");

/// Cache-key version. Bumping forces re-extraction. Tied to the crate
/// version so each release gets a fresh bundled-skills directory.
pub const BUNDLED_SKILLS_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Extract the bundled skills tree to a versioned cache directory and
/// return the path. Idempotent: if the cache directory already exists
/// AND contains every bundled file, returns the path without rewriting.
pub fn ensure_extracted() -> std::io::Result<PathBuf> {
    let dest = bundled_skills_cache_dir();
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() && is_complete(&dest) {
        return Ok(dest);
    }
    fs::create_dir_all(&dest)?;
    BUNDLED_SKILLS.extract(&dest)?;
    Ok(dest)
}

/// Resolved cache directory:
/// - `$XDG_CACHE_HOME/alva/bundled-skills-v<X>/`
/// - else `dirs::cache_dir()/alva/bundled-skills-v<X>/`
/// - else `~/.cache/alva/bundled-skills-v<X>/`
pub fn bundled_skills_cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".cache")
        });
    base.join("alva")
        .join(format!("bundled-skills-v{}", BUNDLED_SKILLS_VERSION))
}

/// Walk the embedded tree and confirm every entry exists on disk under
/// `dest`. Cheap (no content compare) — we trust the version-stamped cache
/// dir name to invalidate correctly across releases.
fn is_complete(dest: &std::path::Path) -> bool {
    fn walk(dir: &Dir<'_>, dest: &std::path::Path) -> bool {
        for f in dir.files() {
            if !dest.join(f.path()).is_file() {
                return false;
            }
        }
        for d in dir.dirs() {
            if !walk(d, dest) {
                return false;
            }
        }
        true
    }
    walk(&BUNDLED_SKILLS, dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Single test exercising:
    /// 1. `ensure_extracted` writes the bundled tree on first call
    /// 2. Second call is idempotent (no rewrite)
    /// 3. `SkillsPlugin::with_bundled` actually finds the extracted skills
    ///    end-to-end (proving the install→read pipeline works)
    ///
    /// All three checks share a single `XDG_CACHE_HOME` env var because Rust
    /// runs unit tests in parallel by default and that env var is process-global.
    #[tokio::test(flavor = "multi_thread")]
    async fn extracts_idempotent_and_skill_is_discoverable() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", tmp.path());

        // 1) Extraction lands at the resolved cache dir under our temp root.
        let first = ensure_extracted().expect("first extract ok");
        let dest = bundled_skills_cache_dir();
        assert_eq!(first, dest);
        assert!(first.starts_with(tmp.path()));
        let skill_md = first.join("project-tooling").join("SKILL.md");
        assert!(
            skill_md.is_file(),
            "expected {} to exist",
            skill_md.display()
        );
        let body = fs::read_to_string(&skill_md).unwrap();
        assert!(body.contains("name: project-tooling"));

        // 2) Second call is a no-op (file mtime unchanged).
        let mtime_before = fs::metadata(&skill_md).unwrap().modified().unwrap();
        let second = ensure_extracted().expect("second extract ok");
        assert_eq!(first, second);
        let mtime_after = fs::metadata(&skill_md).unwrap().modified().unwrap();
        assert_eq!(mtime_before, mtime_after, "second call must not rewrite");

        // 3) End-to-end: SkillsPlugin::with_bundled wires the extracted dir
        //    as the bundled source, the underlying FsSkillRepository scans it,
        //    and the project-tooling skill is in the listed skills.
        let primary = tmp.path().join("project-skills");
        std::fs::create_dir_all(primary.join("user")).unwrap();
        std::fs::create_dir_all(primary.join("mbb")).unwrap();
        let repo = alva_protocol_skill::fs::FsSkillRepository::new(
            first.clone(),
            primary.join("mbb"),
            primary.join("user"),
            primary.join("state.json"),
        );
        use alva_app_core::extension::skills::skill_ports::skill_repository::SkillRepository;
        let skills = repo.list_skills().await.expect("list_skills ok");
        let names: Vec<&str> = skills.iter().map(|s| s.meta.name.as_str()).collect();
        for required in [
            "project-tooling",
            "autonomous-ui-test",
            "autonomous-ui-repair",
        ] {
            assert!(
                names.contains(&required),
                "bundled skill {required:?} must be discoverable; got: {names:?}"
            );
        }
    }
}
