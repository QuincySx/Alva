//! XDG-aware path resolution for Alva configuration, skills, MCP, and data.
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

use std::path::{Path, PathBuf};

/// Central path resolver following XDG Base Directory Specification.
#[derive(Debug, Clone)]
pub struct AlvaPaths {
    /// Global config directory (~/.config/alva)
    pub global_dir: PathBuf,
    /// Project data directory (<workspace>/.alva)
    pub project_dir: PathBuf,
}

impl AlvaPaths {
    /// Create path resolver for the given workspace.
    pub fn new(workspace: &Path) -> Self {
        let global_dir = Self::resolve_global_dir();
        let project_dir = workspace.join(".alva");
        Self {
            global_dir,
            project_dir,
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

    // ── Skills ──────────────────────────────────────────────────────

    pub fn global_skills_dir(&self) -> PathBuf {
        self.global_dir.join("skills")
    }

    pub fn project_skills_dir(&self) -> PathBuf {
        self.project_dir.join("skills")
    }

    // ── Session / checkpoint data ───────────────────────────────────

    pub fn sessions_dir(&self) -> PathBuf {
        self.project_dir.join("sessions")
    }

    pub fn checkpoints_dir(&self) -> PathBuf {
        self.project_dir.join("checkpoints")
    }
}
