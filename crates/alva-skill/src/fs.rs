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

    /// Load enabled skill names from state file
    async fn load_enabled_names(&self) -> HashSet<String> {
        let Ok(content) = tokio::fs::read_to_string(&self.state_file).await else {
            // If state file doesn't exist, default: all bundled skills are enabled
            return HashSet::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return HashSet::new();
        };
        value["enabled"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
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
