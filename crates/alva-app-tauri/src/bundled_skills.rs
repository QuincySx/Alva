// INPUT:  std::{fs, path}, include_dir
// OUTPUT: ensure_extracted, BUNDLED_SKILLS_VERSION
// POS:    Tauri-side mirror of the CLI bundled-skills logic. Same workspace-root
//         `assets/skills/` source, separate cache dir per-binary so version drift
//         between CLI and Tauri can't cross-pollute.

//! Tauri counterpart to `alva-app-cli/src/bundled_skills.rs`.
//!
//! Both binaries embed the same workspace-root `assets/skills/` tree at
//! compile time and extract on first run to `<cache_dir>/alva/<binary>/
//! bundled-skills-v<VERSION>/`. The path is fed to `SkillsExtension`
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
