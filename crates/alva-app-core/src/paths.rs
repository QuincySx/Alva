// INPUT:  filesystem paths, XDG/home directory discovery
// OUTPUT: AlvaPaths and workspace-bounded collect_agents_md_chain()
// POS:    Central path policy for app-owned data and read-only ecosystem discovery.
//! XDG-aware path resolution for Alva configuration, skills, MCP, and data.
//!
//! ## Alva-owned layout
//!
//! ```text
//! ~/.config/alva/              ← Global (XDG_CONFIG_HOME)
//! ├── config.json              ← Provider config (key, model, base_url)
//! ├── mcp.json                 ← Global MCP server configs
//! └── skills/                  ← Global skills
//!     ├── bundled/
//!     ├── user/
//!     └── state.json
//!
//! <workspace>/.alva/           ← Project
//! ├── config.json              ← Project provider overrides
//! ├── mcp.json                 ← Project-specific MCP servers
//! ├── skills/                  ← Project-specific skills
//! │   ├── user/
//! │   └── state.json
//! ├── sessions/                ← Session persistence
//! └── checkpoints/             ← File rollback checkpoints
//! ```
//!
//! ## Ecosystem co-load roots (read-only discovery)
//!
//! Skills and MCP configs are also discovered from common cross-tool locations
//! so Alva can load content users already installed for Claude / Codex / Cursor:
//!
//! ```text
//! <workspace>/.agents/skills/     ~/.agents/skills/
//! <workspace>/.claude/skills/     ~/.claude/skills/
//! <workspace>/.mcp.json           <workspace>/mcp.json
//! ```
//!
//! Later paths in each discovery list override earlier ones on id/name clash.
//! Alva-owned project paths always win over ecosystem project paths.

use std::path::{Path, PathBuf};

/// Central path resolver following XDG Base Directory Specification.
#[derive(Debug, Clone)]
pub struct AlvaPaths {
    /// Global config directory (~/.config/alva)
    pub global_dir: PathBuf,
    /// Project data directory (<workspace>/.alva)
    pub project_dir: PathBuf,
    /// Workspace root used for project-relative ecosystem paths
    pub workspace: PathBuf,
}

impl AlvaPaths {
    /// Create path resolver for the given workspace.
    pub fn new(workspace: &Path) -> Self {
        let global_dir = Self::resolve_global_dir();
        let project_dir = workspace.join(".alva");
        Self {
            global_dir,
            project_dir,
            workspace: workspace.to_path_buf(),
        }
    }

    /// Resolve the global config directory.
    ///
    /// Priority:
    /// 1. `$XDG_CONFIG_HOME/alva`
    /// 2. `dirs::config_dir()/alva` (platform-appropriate fallback)
    /// 3. `~/.config/alva` (hardcoded fallback)
    fn resolve_global_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("alva");
        }
        dirs::config_dir()
            .map(|d| d.join("alva"))
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".config")
                    .join("alva")
            })
    }

    fn home_dir() -> Option<PathBuf> {
        dirs::home_dir()
    }

    // ── Provider config ─────────────────────────────────────────────

    pub fn global_config(&self) -> PathBuf {
        self.global_dir.join("config.json")
    }

    pub fn project_config(&self) -> PathBuf {
        self.project_dir.join("config.json")
    }

    // ── MCP config ──────────────────────────────────────────────────

    pub fn global_mcp_config(&self) -> PathBuf {
        self.global_dir.join("mcp.json")
    }

    pub fn project_mcp_config(&self) -> PathBuf {
        self.project_dir.join("mcp.json")
    }

    /// MCP config files to load, **low → high priority** (later overrides).
    ///
    /// Order: Alva global → ecosystem project roots → Alva project.
    /// Missing files are skipped by the loader.
    pub fn mcp_discovery_paths(&self) -> Vec<PathBuf> {
        vec![
            self.global_mcp_config(),
            self.workspace.join(".mcp.json"),
            self.workspace.join("mcp.json"),
            self.project_mcp_config(),
        ]
    }

    // ── Agent templates ─────────────────────────────────────────────

    pub fn global_agents_config(&self) -> PathBuf {
        self.global_dir.join("agents.toml")
    }

    pub fn project_agents_config(&self) -> PathBuf {
        self.project_dir.join("agents.toml")
    }

    // ── Skills ──────────────────────────────────────────────────────

    pub fn global_skills_dir(&self) -> PathBuf {
        self.global_dir.join("skills")
    }

    pub fn project_skills_dir(&self) -> PathBuf {
        self.project_dir.join("skills")
    }

    /// Extra skill roots co-loaded with the Alva primary tree.
    ///
    /// Flat layout (`<root>/<skill-name>/SKILL.md`). Alva primary
    /// (`.alva/skills` with `user/` / `bundled/`) is **not** included —
    /// callers pass that separately as the writable primary.
    ///
    /// Order is **low → high priority** for last-wins merge inside the
    /// repository: ecosystem global first, then Alva global, then project
    /// ecosystem. Project Alva `user/` still wins via primary scan order.
    pub fn skill_extra_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        if let Some(home) = Self::home_dir() {
            dirs.push(home.join(".agents").join("skills"));
            dirs.push(home.join(".claude").join("skills"));
        }

        // Alva global: both flat root and legacy `user/` subdir.
        dirs.push(self.global_skills_dir());
        dirs.push(self.global_skills_dir().join("user"));

        dirs.push(self.workspace.join(".agents").join("skills"));
        dirs.push(self.workspace.join(".claude").join("skills"));

        dirs
    }

    /// Human-readable skill source list for UI / diagnostics.
    pub fn skill_source_labels(&self) -> Vec<(String, PathBuf)> {
        let mut out = vec![
            ("Project .alva/skills".into(), self.project_skills_dir()),
            (
                "Project .agents/skills".into(),
                self.workspace.join(".agents").join("skills"),
            ),
            (
                "Project .claude/skills".into(),
                self.workspace.join(".claude").join("skills"),
            ),
            (
                "Global ~/.config/alva/skills".into(),
                self.global_skills_dir(),
            ),
        ];
        if let Some(home) = Self::home_dir() {
            out.push((
                "Global ~/.agents/skills".into(),
                home.join(".agents").join("skills"),
            ));
            out.push((
                "Global ~/.claude/skills".into(),
                home.join(".claude").join("skills"),
            ));
        }
        out
    }

    // ── Subprocess plugin extensions ────────────────────────────────

    pub fn global_extensions_dir(&self) -> PathBuf {
        self.global_dir.join("extensions")
    }

    pub fn project_extensions_dir(&self) -> PathBuf {
        self.project_dir.join("extensions")
    }

    // ── Session / checkpoint data ───────────────────────────────────

    pub fn sessions_dir(&self) -> PathBuf {
        self.project_dir.join("sessions")
    }

    pub fn checkpoints_dir(&self) -> PathBuf {
        self.project_dir.join("checkpoints")
    }
}

/// Collect `AGENTS.md` files from `start` upward until (and including) `stop`
/// or the filesystem root.
///
/// Returns paths **outer → inner** (workspace ancestors first, nearest last)
/// so callers can concatenate with nearest winning by appearing later in the
/// prompt. `stop` must be an ancestor of `start`, and only files whose resolved
/// target remains inside `stop` are kept.
pub fn collect_agents_md_chain(start: &Path, stop: &Path) -> Vec<PathBuf> {
    let Ok(start) = start.canonicalize() else {
        return Vec::new();
    };
    let Ok(stop) = stop.canonicalize() else {
        return Vec::new();
    };
    if !start.starts_with(&stop) {
        return Vec::new();
    }

    let mut chain = Vec::new();
    let mut cur = start;
    loop {
        let candidate = cur.join("AGENTS.md");
        if candidate.is_file() {
            if let Ok(canon) = candidate.canonicalize() {
                // A symlinked AGENTS.md is allowed only when its target remains
                // inside the same workspace boundary.
                if canon.starts_with(&stop) {
                    chain.push(canon);
                }
            }
        }
        if cur == stop {
            break;
        }
        match cur.parent() {
            Some(parent) if parent != cur => cur = parent.to_path_buf(),
            _ => break,
        }
    }

    // We walked inner → outer; reverse to outer → inner.
    chain.reverse();
    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn mcp_discovery_ends_with_project_alva() {
        let ws = std::env::temp_dir().join(format!(
            "alva-paths-mcp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&ws).unwrap();
        let paths = AlvaPaths::new(&ws);
        let list = paths.mcp_discovery_paths();
        assert_eq!(list.last().unwrap(), &paths.project_mcp_config());
        assert!(list.iter().any(|p| p.ends_with(".mcp.json")));
        assert!(list.iter().any(
            |p| p.file_name().and_then(|n| n.to_str()) == Some("mcp.json")
                && p.parent() == Some(ws.as_path())
        ));
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn skill_extra_dirs_include_agents_and_claude() {
        let ws = std::env::temp_dir().join(format!(
            "alva-paths-skills-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&ws).unwrap();
        let paths = AlvaPaths::new(&ws);
        let extras = paths.skill_extra_dirs();
        assert!(extras
            .iter()
            .any(|p| p.ends_with(".agents/skills") || p.ends_with(".agents\\skills")));
        assert!(extras
            .iter()
            .any(|p| p.ends_with(".claude/skills") || p.ends_with(".claude\\skills")));
        // primary .alva/skills is NOT in extras
        assert!(!extras.iter().any(|p| p == &paths.project_skills_dir()));
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn collect_agents_md_chain_outer_to_inner() {
        let root = std::env::temp_dir().join(format!(
            "alva-paths-agents-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let nested = root.join("crates").join("pkg");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("AGENTS.md"), "root").unwrap();
        fs::write(nested.join("AGENTS.md"), "nested").unwrap();

        let chain = collect_agents_md_chain(&nested, &root);
        assert_eq!(chain.len(), 2);
        assert!(chain[0].ends_with("AGENTS.md"));
        let contents: Vec<_> = chain
            .iter()
            .map(|p| fs::read_to_string(p).unwrap())
            .collect();
        assert_eq!(contents, vec!["root".to_string(), "nested".to_string()]);

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn collect_agents_md_chain_rejects_symlink_target_outside_workspace() {
        use std::os::unix::fs::symlink;

        let unique = format!(
            "alva-paths-agents-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let root = std::env::temp_dir().join(&unique);
        let nested = root.join("crates").join("pkg");
        let outside = std::env::temp_dir().join(format!("{unique}-outside.md"));
        fs::create_dir_all(&nested).unwrap();
        fs::write(&outside, "outside").unwrap();
        fs::write(nested.join("AGENTS.md"), "nested").unwrap();
        symlink(&outside, root.join("AGENTS.md")).unwrap();

        let chain = collect_agents_md_chain(&nested, &root);
        assert_eq!(chain.len(), 1);
        assert_eq!(fs::read_to_string(&chain[0]).unwrap(), "nested");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn collect_agents_md_chain_requires_stop_to_be_ancestor() {
        let unique = format!(
            "alva-paths-agents-boundary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let root = std::env::temp_dir().join(unique);
        let start = root.join("start");
        let unrelated_stop = root.join("other");
        fs::create_dir_all(&start).unwrap();
        fs::create_dir_all(&unrelated_stop).unwrap();
        fs::write(start.join("AGENTS.md"), "must not load").unwrap();

        assert!(collect_agents_md_chain(&start, &unrelated_stop).is_empty());

        let _ = fs::remove_dir_all(&root);
    }
}
