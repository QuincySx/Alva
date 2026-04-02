// INPUT:  std::collections, std::path, async_trait, walkdir, serde_json, serde_yaml, tokio::fs, crate::types, crate::error, crate::repository
// OUTPUT: FsSkillRepository
// POS:    Filesystem-backed Skill repository implementation scanning bundled/MBB/user directories.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::{
    error::SkillError,
    repository::{SkillInstallSource, SkillRepository},
    types::{ResourceContentType, Skill, SkillBody, SkillKind, SkillMeta, SkillResource},
};

/// File system Skill repository
///
/// Directory layout (corresponding to Wukong's resources/ structure):
///   <bundled_dir>/           -- App bundle built-in, read-only
///     skill-name/
///       SKILL.md
///       scripts/
///       references/
///   <mbb_dir>/               -- MBB Skills, contains manifest.json
///     manifest.json
///     skill-name/
///       SKILL.md
///       ...
///   <user_dir>/              -- User-installed, read-write
///     skill-name/
///       SKILL.md
///       ...
pub struct FsSkillRepository {
    bundled_dir: PathBuf,
    mbb_dir: PathBuf,
    user_dir: PathBuf,
    /// Skill enabled state persistence file (JSON format)
    state_file: PathBuf,
}

impl FsSkillRepository {
    pub fn new(
        bundled_dir: impl Into<PathBuf>,
        mbb_dir: impl Into<PathBuf>,
        user_dir: impl Into<PathBuf>,
        state_file: impl Into<PathBuf>,
    ) -> Self {
        Self {
            bundled_dir: bundled_dir.into(),
            mbb_dir: mbb_dir.into(),
            user_dir: user_dir.into(),
            state_file: state_file.into(),
        }
    }

    /// Parse a single Skill directory
    async fn parse_skill_dir(
        &self,
        dir: &Path,
        kind: SkillKind,
        enabled_names: &HashSet<String>,
    ) -> Result<Option<Skill>, SkillError> {
        let skill_md_path = dir.join("SKILL.md");
        if !skill_md_path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&skill_md_path)
            .await
            .map_err(|e| SkillError::Io(e.to_string()))?;

        let meta = Self::parse_frontmatter(&content)?;
        let enabled = enabled_names.contains(&meta.name);

        Ok(Some(Skill {
            meta,
            kind,
            root_path: dir.to_path_buf(),
            enabled,
        }))
    }

    /// Parse YAML frontmatter (content between --- delimiters)
    pub fn parse_frontmatter(content: &str) -> Result<SkillMeta, SkillError> {
        // Find first "---" and second "---"
        let after_first = content
            .strip_prefix("---")
            .and_then(|s| s.strip_prefix('\n').or_else(|| s.strip_prefix("\r\n")))
            .ok_or(SkillError::InvalidSkillMd(
                "missing opening '---'".to_string(),
            ))?;

        let end_idx = after_first
            .find("\n---")
            .ok_or(SkillError::InvalidSkillMd(
                "missing closing '---'".to_string(),
            ))?;

        let yaml_str = &after_first[..end_idx];

        serde_yaml::from_str::<SkillMeta>(yaml_str)
            .map_err(|e| SkillError::InvalidFrontmatter(e.to_string()))
    }

    /// Parse SKILL.md body (everything after frontmatter)
    pub fn parse_body(content: &str) -> SkillBody {
        // Skip frontmatter: find second "---"
        let body_start = if content.starts_with("---") {
            // Find the closing "---" after the opening one
            let after_opening = &content[3..];
            if let Some(newline_pos) = after_opening.find('\n') {
                let after_first_newline = &after_opening[newline_pos + 1..];
                if let Some(closing_pos) = after_first_newline.find("\n---") {
                    let absolute_pos = 3 + newline_pos + 1 + closing_pos;
                    // Skip past \n---
                    let end = absolute_pos + 4; // \n---
                    // Skip the newline after closing ---
                    if end < content.len() && content.as_bytes()[end] == b'\n' {
                        end + 1
                    } else if end + 1 < content.len()
                        && &content[end..end + 2] == "\r\n"
                    {
                        end + 2
                    } else {
                        end
                    }
                } else {
                    0
                }
            } else {
                0
            }
        } else {
            0
        };
        let markdown = content[body_start..].trim().to_string();
        let estimated_tokens = (markdown.len() / 4) as u32;
        SkillBody {
            markdown,
            estimated_tokens,
        }
    }

    /// Read MBB manifest.json domain bindings
    async fn read_mbb_manifest(&self) -> HashMap<String, Vec<String>> {
        let manifest_path = self.mbb_dir.join("manifest.json");
        let Ok(content) = tokio::fs::read_to_string(&manifest_path).await else {
            return HashMap::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return HashMap::new();
        };
        let mut map = HashMap::new();
        if let Some(skills) = value["skills"].as_array() {
            for skill in skills {
                let id = skill["id"].as_str().unwrap_or_default().to_string();
                let domains = skill["domains"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                map.insert(id, domains);
            }
        }
        map
    }

    /// Load enabled skill names from state file.
    ///
    /// When the state file does not exist (first run or fresh install),
    /// defaults to all bundled skills enabled — scans bundled_dir for SKILL.md
    /// frontmatter names.
    async fn load_enabled_names(&self) -> HashSet<String> {
        let Ok(content) = tokio::fs::read_to_string(&self.state_file).await else {
            // No state file → default: all bundled skills enabled
            return self.scan_bundled_names().await;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return self.scan_bundled_names().await;
        };
        value["enabled"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_else(|| {
                // "enabled" key missing or not an array → treat as first run
                HashSet::new()
            })
    }

    /// Scan bundled_dir for skill names (used as default when no state file exists).
    async fn scan_bundled_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();
        let Ok(mut entries) = tokio::fs::read_dir(&self.bundled_dir).await else {
            return names;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let skill_md = entry.path().join("SKILL.md");
            if let Ok(content) = tokio::fs::read_to_string(&skill_md).await {
                if let Ok(meta) = Self::parse_frontmatter(&content) {
                    names.insert(meta.name);
                }
            }
        }
        names
    }

    /// Save enabled skill names to state file
    async fn save_enabled_names(&self, names: &HashSet<String>) -> Result<(), SkillError> {
        let value = serde_json::json!({
            "enabled": names.iter().collect::<Vec<_>>()
        });
        let content = serde_json::to_string_pretty(&value)
            .map_err(|e| SkillError::Serialization(e.to_string()))?;

        // Ensure parent directory exists
        if let Some(parent) = self.state_file.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| SkillError::Io(e.to_string()))?;
        }

        tokio::fs::write(&self.state_file, content)
            .await
            .map_err(|e| SkillError::Io(e.to_string()))
    }

    /// Scan a directory for skill subdirectories
    async fn scan_dir(
        &self,
        dir: &Path,
        kind_fn: impl Fn(&str) -> SkillKind,
        enabled_names: &HashSet<String>,
    ) -> Vec<Skill> {
        let mut skills = vec![];
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            return skills;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false)
            {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                let kind = kind_fn(&dir_name);
                if let Ok(Some(skill)) = self
                    .parse_skill_dir(&entry.path(), kind, enabled_names)
                    .await
                {
                    skills.push(skill);
                }
            }
        }
        skills
    }
}

#[async_trait]
impl SkillRepository for FsSkillRepository {
    async fn list_skills(&self) -> Result<Vec<Skill>, SkillError> {
        let enabled_names = self.load_enabled_names().await;
        let mbb_domains = self.read_mbb_manifest().await;
        let mut skills = vec![];

        // Scan Bundled Skills
        let bundled = self
            .scan_dir(&self.bundled_dir, |_| SkillKind::Bundled, &enabled_names)
            .await;
        skills.extend(bundled);

        // Scan MBB Skills
        let mbb_dir = self.mbb_dir.clone();
        if let Ok(mut entries) = tokio::fs::read_dir(&mbb_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry
                    .file_type()
                    .await
                    .map(|t| t.is_dir())
                    .unwrap_or(false)
                {
                    let dir_name = entry.file_name().to_string_lossy().to_string();
                    let domains = mbb_domains.get(&dir_name).cloned().unwrap_or_default();
                    let kind = SkillKind::Mbb { domains };
                    if let Ok(Some(skill)) = self
                        .parse_skill_dir(&entry.path(), kind, &enabled_names)
                        .await
                    {
                        skills.push(skill);
                    }
                }
            }
        }

        // Scan User-installed Skills
        let user = self
            .scan_dir(
                &self.user_dir,
                |_| SkillKind::UserInstalled,
                &enabled_names,
            )
            .await;
        skills.extend(user);

        Ok(skills)
    }

    async fn find_skill(&self, name: &str) -> Result<Option<Skill>, SkillError> {
        let skills = self.list_skills().await?;
        Ok(skills.into_iter().find(|s| s.meta.name == name))
    }

    async fn load_body(&self, name: &str) -> Result<SkillBody, SkillError> {
        let skill = self
            .find_skill(name)
            .await?
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;

        let skill_md_path = skill.root_path.join("SKILL.md");
        let content = tokio::fs::read_to_string(&skill_md_path)
            .await
            .map_err(|e| SkillError::Io(e.to_string()))?;

        Ok(Self::parse_body(&content))
    }

    async fn load_resource(
        &self,
        name: &str,
        relative_path: &str,
    ) -> Result<SkillResource, SkillError> {
        let skill = self
            .find_skill(name)
            .await?
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;

        // Security check: prevent path traversal
        let resource_path = skill.root_path.join(relative_path);
        if !resource_path.starts_with(&skill.root_path) {
            return Err(SkillError::PathTraversal(relative_path.to_string()));
        }

        let content = tokio::fs::read(&resource_path)
            .await
            .map_err(|e| SkillError::Io(e.to_string()))?;

        let content_type = match resource_path.extension().and_then(|e| e.to_str()) {
            Some("md") | Some("markdown") => ResourceContentType::Markdown,
            Some("py") => ResourceContentType::Python,
            Some("js") => ResourceContentType::JavaScript,
            Some("ts") => ResourceContentType::TypeScript,
            Some("sh") | Some("bash") => ResourceContentType::Shell,
            Some(ext) => ResourceContentType::Other(ext.to_string()),
            None => ResourceContentType::Binary,
        };

        Ok(SkillResource {
            relative_path: relative_path.to_string(),
            content,
            content_type,
        })
    }

    async fn list_resources(&self, name: &str) -> Result<Vec<String>, SkillError> {
        let skill = self
            .find_skill(name)
            .await?
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;

        let mut resources = vec![];

        fn collect_paths(
            root: &Path,
            dir: &Path,
            resources: &mut Vec<String>,
        ) -> Result<(), std::io::Error> {
            for entry in walkdir::WalkDir::new(dir)
                .min_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    let file_name = entry.file_name().to_string_lossy();
                    // Exclude SKILL.md itself
                    if file_name == "SKILL.md" {
                        continue;
                    }
                    if let Ok(rel) = entry.path().strip_prefix(root) {
                        resources.push(rel.to_string_lossy().to_string());
                    }
                }
            }
            Ok(())
        }

        collect_paths(&skill.root_path, &skill.root_path, &mut resources)
            .map_err(|e| SkillError::Io(e.to_string()))?;

        Ok(resources)
    }

    async fn install(&self, source: SkillInstallSource) -> Result<SkillMeta, SkillError> {
        match source {
            SkillInstallSource::LocalDir(dir_path) => {
                // Validate SKILL.md exists
                let skill_md = dir_path.join("SKILL.md");
                if !skill_md.exists() {
                    return Err(SkillError::InvalidSkillMd(
                        "SKILL.md not found in source directory".to_string(),
                    ));
                }

                let content = tokio::fs::read_to_string(&skill_md)
                    .await
                    .map_err(|e| SkillError::Io(e.to_string()))?;
                let meta = Self::parse_frontmatter(&content)?;

                // Copy to user_dir/<skill_name>/
                let dest = self.user_dir.join(&meta.name);
                if dest.exists() {
                    tokio::fs::remove_dir_all(&dest)
                        .await
                        .map_err(|e| SkillError::Io(e.to_string()))?;
                }
                copy_dir_recursive(&dir_path, &dest)
                    .await
                    .map_err(|e| SkillError::Io(e.to_string()))?;

                // Enable by default
                let mut enabled = self.load_enabled_names().await;
                enabled.insert(meta.name.clone());
                self.save_enabled_names(&enabled).await?;

                Ok(meta)
            }
            SkillInstallSource::LocalZip(_) | SkillInstallSource::RemoteUrl(_) => {
                // Placeholder -- zip extraction and remote download not yet implemented
                Err(SkillError::Io(
                    "zip and remote installation not yet implemented".to_string(),
                ))
            }
        }
    }

    async fn remove(&self, name: &str) -> Result<(), SkillError> {
        let dest = self.user_dir.join(name);
        if dest.exists() {
            tokio::fs::remove_dir_all(&dest)
                .await
                .map_err(|e| SkillError::Io(e.to_string()))?;
        }

        // Remove from enabled state
        let mut enabled = self.load_enabled_names().await;
        enabled.remove(name);
        self.save_enabled_names(&enabled).await?;

        Ok(())
    }

    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), SkillError> {
        let mut names = self.load_enabled_names().await;
        if enabled {
            names.insert(name.to_string());
        } else {
            names.remove(name);
        }
        self.save_enabled_names(&names).await
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::SkillRepository;

    const SKILL_MD_ALPHA: &str = r#"---
name: alpha
description: Alpha skill for testing
---

# Alpha Instructions

Do alpha things.
"#;

    const SKILL_MD_BETA: &str = r#"---
name: beta
description: Beta skill for testing
license: MIT
allowed_tools:
  - read_file
  - bash
---

# Beta Instructions

Do beta things.
"#;

    /// Create a temporary directory tree with bundled, mbb, user dirs and state file.
    /// Returns (FsSkillRepository, TempDir).
    fn setup_repo() -> (tempfile::TempDir, FsSkillRepository) {
        let dir = tempfile::tempdir().unwrap();
        let bundled = dir.path().join("bundled");
        let mbb = dir.path().join("mbb");
        let user = dir.path().join("user");
        let state = dir.path().join("state.json");

        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&mbb).unwrap();
        std::fs::create_dir_all(&user).unwrap();

        let repo = FsSkillRepository::new(&bundled, &mbb, &user, &state);
        (dir, repo)
    }

    fn write_skill(base_dir: &Path, name: &str, content: &str) {
        let skill_dir = base_dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    fn write_skill_with_resources(base_dir: &Path, name: &str, content: &str, resources: &[(&str, &[u8])]) {
        let skill_dir = base_dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
        for (path, data) in resources {
            let full = skill_dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(full, data).unwrap();
        }
    }

    // ── parse_frontmatter ───────────────────────────────────────────────

    #[test]
    fn parse_frontmatter_valid() {
        let meta = FsSkillRepository::parse_frontmatter(SKILL_MD_ALPHA).unwrap();
        assert_eq!(meta.name, "alpha");
        assert_eq!(meta.description, "Alpha skill for testing");
        assert!(meta.license.is_none());
        assert!(meta.allowed_tools.is_none());
    }

    #[test]
    fn parse_frontmatter_with_optional_fields() {
        let meta = FsSkillRepository::parse_frontmatter(SKILL_MD_BETA).unwrap();
        assert_eq!(meta.name, "beta");
        assert_eq!(meta.license, Some("MIT".into()));
        assert_eq!(
            meta.allowed_tools,
            Some(vec!["read_file".into(), "bash".into()])
        );
    }

    #[test]
    fn parse_frontmatter_missing_opening_delimiter() {
        let err = FsSkillRepository::parse_frontmatter("no frontmatter here").unwrap_err();
        assert!(matches!(err, SkillError::InvalidSkillMd(_)));
    }

    #[test]
    fn parse_frontmatter_missing_closing_delimiter() {
        let content = "---\nname: broken\n";
        let err = FsSkillRepository::parse_frontmatter(content).unwrap_err();
        assert!(matches!(err, SkillError::InvalidSkillMd(_)));
    }

    // ── parse_body ──────────────────────────────────────────────────────

    #[test]
    fn parse_body_extracts_after_frontmatter() {
        let body = FsSkillRepository::parse_body(SKILL_MD_ALPHA);
        assert!(body.markdown.starts_with("# Alpha Instructions"));
        assert!(body.markdown.contains("Do alpha things."));
        assert!(body.estimated_tokens > 0);
    }

    #[test]
    fn parse_body_no_frontmatter_returns_full_content() {
        let content = "Just some markdown content.";
        let body = FsSkillRepository::parse_body(content);
        assert_eq!(body.markdown, "Just some markdown content.");
    }

    // ── list_skills ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_skills_scans_bundled_dir() {
        let (dir, repo) = setup_repo();
        let bundled = dir.path().join("bundled");
        write_skill(&bundled, "alpha", SKILL_MD_ALPHA);
        write_skill(&bundled, "beta", SKILL_MD_BETA);

        // No state file => all bundled skills enabled by default
        let skills = repo.list_skills().await.unwrap();
        assert_eq!(skills.len(), 2);

        let names: Vec<_> = skills.iter().map(|s| s.meta.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));

        // Bundled skills should be enabled (default behavior)
        for skill in &skills {
            assert!(skill.enabled);
            assert_eq!(skill.kind, SkillKind::Bundled);
        }
    }

    #[tokio::test]
    async fn list_skills_empty_dirs() {
        let (_dir, repo) = setup_repo();
        let skills = repo.list_skills().await.unwrap();
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn list_skills_includes_user_installed() {
        let (dir, repo) = setup_repo();
        let user = dir.path().join("user");
        write_skill(&user, "custom", SKILL_MD_ALPHA);

        // Write state file to enable the skill
        let state = dir.path().join("state.json");
        std::fs::write(&state, r#"{"enabled": ["alpha"]}"#).unwrap();

        let skills = repo.list_skills().await.unwrap();
        let user_skills: Vec<_> = skills
            .iter()
            .filter(|s| matches!(s.kind, SkillKind::UserInstalled))
            .collect();
        assert_eq!(user_skills.len(), 1);
    }

    // ── find_skill ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn find_skill_found() {
        let (dir, repo) = setup_repo();
        write_skill(&dir.path().join("bundled"), "alpha", SKILL_MD_ALPHA);

        let found = repo.find_skill("alpha").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().meta.name, "alpha");
    }

    #[tokio::test]
    async fn find_skill_not_found() {
        let (_dir, repo) = setup_repo();
        let found = repo.find_skill("missing").await.unwrap();
        assert!(found.is_none());
    }

    // ── load_body ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn load_body_returns_markdown() {
        let (dir, repo) = setup_repo();
        write_skill(&dir.path().join("bundled"), "alpha", SKILL_MD_ALPHA);

        let body = repo.load_body("alpha").await.unwrap();
        assert!(body.markdown.contains("Alpha Instructions"));
    }

    #[tokio::test]
    async fn load_body_not_found() {
        let (_dir, repo) = setup_repo();
        let err = repo.load_body("missing").await.unwrap_err();
        assert!(matches!(err, SkillError::SkillNotFound(_)));
    }

    // ── load_resource / list_resources ───────────────────────────────────

    #[tokio::test]
    async fn load_resource_returns_file_content() {
        let (dir, repo) = setup_repo();
        write_skill_with_resources(
            &dir.path().join("bundled"),
            "alpha",
            SKILL_MD_ALPHA,
            &[("refs/api.md", b"# API Reference")],
        );

        let resource = repo.load_resource("alpha", "refs/api.md").await.unwrap();
        assert_eq!(resource.relative_path, "refs/api.md");
        assert_eq!(resource.content, b"# API Reference");
        assert_eq!(resource.content_type, ResourceContentType::Markdown);
    }

    #[tokio::test]
    async fn list_resources_excludes_skill_md() {
        let (dir, repo) = setup_repo();
        write_skill_with_resources(
            &dir.path().join("bundled"),
            "alpha",
            SKILL_MD_ALPHA,
            &[
                ("scripts/run.sh", b"#!/bin/sh\necho hi"),
                ("refs/notes.md", b"# Notes"),
            ],
        );

        let paths = repo.list_resources("alpha").await.unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|p| p.contains("run.sh")));
        assert!(paths.iter().any(|p| p.contains("notes.md")));
        // SKILL.md should be excluded
        assert!(!paths.iter().any(|p| p.contains("SKILL.md")));
    }

    // ── set_enabled ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn set_enabled_persists_to_state_file() {
        let (dir, repo) = setup_repo();
        write_skill(&dir.path().join("bundled"), "alpha", SKILL_MD_ALPHA);

        repo.set_enabled("alpha", true).await.unwrap();

        let state_file = dir.path().join("state.json");
        let content = tokio::fs::read_to_string(&state_file).await.unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        let enabled = value["enabled"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert!(enabled.contains(&"alpha".to_string()));
    }

    // ── install from local dir ──────────────────────────────────────────

    #[tokio::test]
    async fn install_from_local_dir() {
        let (dir, repo) = setup_repo();

        // Create a source directory
        let source = dir.path().join("source-skill");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("SKILL.md"), SKILL_MD_ALPHA).unwrap();
        std::fs::write(source.join("extra.txt"), "extra file").unwrap();

        let meta = repo
            .install(SkillInstallSource::LocalDir(source))
            .await
            .unwrap();
        assert_eq!(meta.name, "alpha");

        // Verify copied to user dir
        let installed = dir.path().join("user").join("alpha");
        assert!(installed.join("SKILL.md").exists());
        assert!(installed.join("extra.txt").exists());
    }

    #[tokio::test]
    async fn install_from_local_dir_missing_skill_md() {
        let (dir, repo) = setup_repo();
        let source = dir.path().join("empty-source");
        std::fs::create_dir_all(&source).unwrap();

        let err = repo
            .install(SkillInstallSource::LocalDir(source))
            .await
            .unwrap_err();
        assert!(matches!(err, SkillError::InvalidSkillMd(_)));
    }

    // ── remove ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn remove_deletes_user_skill_dir() {
        let (dir, repo) = setup_repo();
        let user = dir.path().join("user");
        write_skill(&user, "custom", SKILL_MD_ALPHA);
        assert!(user.join("custom").exists());

        repo.remove("custom").await.unwrap();
        assert!(!user.join("custom").exists());
    }

    // ── MBB skills ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_skills_includes_mbb_with_domains() {
        let (dir, repo) = setup_repo();
        let mbb = dir.path().join("mbb");
        write_skill(&mbb, "train-helper", SKILL_MD_ALPHA);

        // Write manifest.json
        let manifest = serde_json::json!({
            "skills": [
                { "id": "train-helper", "domains": ["12306.cn", "railway.com"] }
            ]
        });
        std::fs::write(mbb.join("manifest.json"), manifest.to_string()).unwrap();

        // Enable train-helper (renamed to "alpha" in SKILL.md)
        let state = dir.path().join("state.json");
        std::fs::write(&state, r#"{"enabled": ["alpha"]}"#).unwrap();

        let skills = repo.list_skills().await.unwrap();
        let mbb_skills: Vec<_> = skills
            .iter()
            .filter(|s| matches!(&s.kind, SkillKind::Mbb { .. }))
            .collect();
        assert_eq!(mbb_skills.len(), 1);

        match &mbb_skills[0].kind {
            SkillKind::Mbb { domains } => {
                assert!(domains.contains(&"12306.cn".to_string()));
                assert!(domains.contains(&"railway.com".to_string()));
            }
            _ => panic!("expected Mbb kind"),
        }
    }
}

/// Recursively copy a directory
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(dst).await?;

    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let ty = entry.file_type().await?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            Box::pin(copy_dir_recursive(&entry.path(), &dest_path)).await?;
        } else {
            tokio::fs::copy(entry.path(), &dest_path).await?;
        }
    }

    Ok(())
}
