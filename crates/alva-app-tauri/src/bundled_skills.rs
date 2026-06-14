// INPUT:  std::{fs, path}, include_dir
// OUTPUT: ensure_extracted, BUNDLED_SKILLS_VERSION
// POS:    Tauri-side mirror of the CLI bundled-skills logic. Same workspace-root
//         `assets/skills/` source, separate cache dir per-binary so version drift
//         between CLI and Tauri can't cross-pollute.

//! Tauri counterpart to `alva-app-cli/src/bundled_skills.rs`.
//!
//! Both binaries embed the same workspace-root `assets/skills/` tree at
//! compile time and extract on first run to `<cache_dir>/alva/<binary>/
//! bundled-skills-v<VERSION>/`. The path is fed to `SkillsPlugin`
//! alongside project + global skill dirs so user-installed skills can
//! override bundled defaults by name.

use std::fs;
use std::path::PathBuf;

use include_dir::{include_dir, Dir};

// Workspace layout: `crates/alva-app-tauri/Cargo.toml` → `../../assets/skills`.
const BUNDLED_SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../assets/skills");

pub const BUNDLED_SKILLS_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    // Tauri keeps its own subdir so a CLI install with a different version
    // doesn't shadow it (and vice versa).
    base.join("alva")
        .join("tauri")
        .join(format!("bundled-skills-v{}", BUNDLED_SKILLS_VERSION))
}

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
    //! Tests for the bundled-skills extraction completeness check and
    //! the cache-dir path invariant.
    //!
    //! `is_complete` is the cache-validity gate inside `ensure_extracted`
    //! — a wrong `true` ships stale assets to SkillsPlugin; a wrong
    //! `false` re-extracts on every launch (perf regression). Both paths
    //! are exercised here against the real compile-time `BUNDLED_SKILLS`
    //! tree, not a synthetic fixture.
    //!
    //! `bundled_skills_cache_dir()` env logic is intentionally NOT
    //! exercised (set_var across parallel tests is racy / unsafe on
    //! recent rustcs); only the version-suffixed structural invariant
    //! is pinned, which is what callers actually depend on.
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Build a unique directory under the OS temp dir — same hermetic
    /// pattern as sqlite_session/registry.rs:749 (no tempfile dep).
    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = format!(
            "alva-bundled-skills-{label}-{}-{}",
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

    /// Find any file inside the embedded BUNDLED_SKILLS tree — used to
    /// exercise the "one file missing → false" path without hard-coding
    /// a name from the asset tree.
    fn first_embedded_file_path() -> Option<PathBuf> {
        fn walk(dir: &Dir<'_>) -> Option<PathBuf> {
            if let Some(f) = dir.files().next() {
                return Some(f.path().to_path_buf());
            }
            dir.dirs().find_map(walk)
        }
        walk(&BUNDLED_SKILLS)
    }

    #[test]
    fn is_complete_returns_false_for_empty_dir() {
        // BUNDLED_SKILLS embeds the workspace `assets/skills/` tree (at
        // least one SKILL.md). An empty target dir must be diagnosed
        // as incomplete → ensure_extracted re-runs the extract step.
        let dir = unique_temp_dir("empty");
        assert!(
            !is_complete(&dir),
            "empty dir must NOT be diagnosed as complete (would skip extract)"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_complete_returns_true_after_full_extract() {
        // Extract the embedded tree, then is_complete must agree the
        // cache is valid — this is the cache-hit path inside
        // ensure_extracted that avoids re-extracting on every launch.
        let dir = unique_temp_dir("extracted");
        BUNDLED_SKILLS
            .extract(&dir)
            .expect("extract embedded skills");
        assert!(
            is_complete(&dir),
            "post-extract dir must be diagnosed as complete (cache hit)"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_complete_returns_false_when_a_single_file_is_missing() {
        // Pin the "even one missing file invalidates cache" guarantee.
        // Without this, a partial extract (disk-full mid-write, manual
        // tampering) would silently ship to SkillsPlugin.
        let dir = unique_temp_dir("partial");
        BUNDLED_SKILLS
            .extract(&dir)
            .expect("extract embedded skills");
        let Some(rel) = first_embedded_file_path() else {
            // The asset tree is empty (shouldn't happen given the
            // workspace layout, but guard anyway): test is trivially
            // satisfied — drop and return.
            let _ = fs::remove_dir_all(&dir);
            return;
        };
        let victim = dir.join(&rel);
        fs::remove_file(&victim).expect("remove one extracted file");
        assert!(
            !is_complete(&dir),
            "missing file {victim:?} must invalidate cache"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_dir_path_invariant_alva_tauri_versioned_suffix() {
        // bundled_skills_cache_dir() resolves a base via env/dirs but
        // ALWAYS appends `alva/tauri/bundled-skills-v<VERSION>`. The
        // SkillsPlugin consumer + CI cleanup tooling key off this
        // exact suffix structure — pin it.
        let dir = bundled_skills_cache_dir();
        let s = dir.to_string_lossy();
        let expected_leaf = format!("bundled-skills-v{}", BUNDLED_SKILLS_VERSION);
        assert!(
            s.ends_with(&expected_leaf),
            "cache dir must end with versioned leaf: got {s}"
        );
        // Also pin the immediate parents — `alva/tauri/<leaf>` so the
        // CLI binary's cache (alva/cli/<leaf>) can never collide.
        assert!(
            s.contains("alva"),
            "cache dir must live under 'alva' segment: got {s}"
        );
        assert!(
            s.contains("tauri"),
            "cache dir must live under 'tauri' segment: got {s}"
        );
    }
}
