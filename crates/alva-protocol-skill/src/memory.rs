// INPUT:  crate::types, crate::error, crate::repository, async_trait, std::sync
// OUTPUT: InMemorySkill, InMemorySkillRepository
// POS:    In-memory Skill repository implementation for wasm/V8 targets and testing.
//         Does not require filesystem access. install/remove are not supported.
use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::{
    error::SkillError,
    repository::{SkillInstallSource, SkillRepository},
    types::{ResourceContentType, Skill, SkillBody, SkillKind, SkillMeta, SkillResource},
};

/// A single in-memory skill entry
pub struct InMemorySkill {
    pub meta: SkillMeta,
    pub kind: SkillKind,
    /// Raw Markdown body (everything after frontmatter, or the full body content)
    pub body: String,
    /// List of (relative_path, raw_bytes) resource pairs
    pub resources: Vec<(String, Vec<u8>)>,
    pub enabled: bool,
}

/// In-memory Skill repository.
///
/// Suitable for wasm/V8 environments and unit tests where no filesystem is available.
/// `install` and `remove` always return errors — mutations are not supported.
pub struct InMemorySkillRepository {
    skills: Mutex<Vec<InMemorySkill>>,
}

impl InMemorySkillRepository {
    /// Create a new repository pre-populated with the given skills.
    pub fn new(skills: Vec<InMemorySkill>) -> Self {
        Self {
            skills: Mutex::new(skills),
        }
    }

    /// Create an empty repository.
    pub fn empty() -> Self {
        Self::new(vec![])
    }

    /// Infer ResourceContentType from a relative path's extension.
    fn content_type_for(relative_path: &str) -> ResourceContentType {
        let ext = std::path::Path::new(relative_path)
            .extension()
            .and_then(|e| e.to_str());
        match ext {
            Some("md") | Some("markdown") => ResourceContentType::Markdown,
            Some("py") => ResourceContentType::Python,
            Some("js") => ResourceContentType::JavaScript,
            Some("ts") => ResourceContentType::TypeScript,
            Some("sh") | Some("bash") => ResourceContentType::Shell,
            Some(e) => ResourceContentType::Other(e.to_string()),
            None => ResourceContentType::Binary,
        }
    }

    /// Build a `Skill` value from an `InMemorySkill` entry.
    fn to_skill(entry: &InMemorySkill) -> Skill {
        Skill {
            meta: entry.meta.clone(),
            kind: entry.kind.clone(),
            // Use a non-existent sentinel path so callers that inspect root_path
            // can tell this is an in-memory skill.
            root_path: PathBuf::from("/in-memory"),
            enabled: entry.enabled,
        }
    }
}

#[async_trait]
impl SkillRepository for InMemorySkillRepository {
    async fn list_skills(&self) -> Result<Vec<Skill>, SkillError> {
        let guard = self
            .skills
            .lock()
            .map_err(|_| SkillError::Io("mutex poisoned".to_string()))?;
        Ok(guard.iter().map(Self::to_skill).collect())
    }

    async fn find_skill(&self, name: &str) -> Result<Option<Skill>, SkillError> {
        let guard = self
            .skills
            .lock()
            .map_err(|_| SkillError::Io("mutex poisoned".to_string()))?;
        Ok(guard
            .iter()
            .find(|s| s.meta.name == name)
            .map(Self::to_skill))
    }

    async fn load_body(&self, name: &str) -> Result<SkillBody, SkillError> {
        let guard = self
            .skills
            .lock()
            .map_err(|_| SkillError::Io("mutex poisoned".to_string()))?;
        let entry = guard
            .iter()
            .find(|s| s.meta.name == name)
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;
        let markdown = entry.body.trim().to_string();
        let estimated_tokens = (markdown.len() / 4) as u32;
        Ok(SkillBody {
            markdown,
            estimated_tokens,
        })
    }

    async fn load_resource(
        &self,
        name: &str,
        relative_path: &str,
    ) -> Result<SkillResource, SkillError> {
        let guard = self
            .skills
            .lock()
            .map_err(|_| SkillError::Io("mutex poisoned".to_string()))?;
        let entry = guard
            .iter()
            .find(|s| s.meta.name == name)
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;
        let (path, content) = entry
            .resources
            .iter()
            .find(|(p, _)| p == relative_path)
            .ok_or_else(|| {
                SkillError::Io(format!(
                    "resource '{}' not found in skill '{}'",
                    relative_path, name
                ))
            })?;
        Ok(SkillResource {
            relative_path: path.clone(),
            content: content.clone(),
            content_type: Self::content_type_for(path),
        })
    }

    async fn list_resources(&self, name: &str) -> Result<Vec<String>, SkillError> {
        let guard = self
            .skills
            .lock()
            .map_err(|_| SkillError::Io("mutex poisoned".to_string()))?;
        let entry = guard
            .iter()
            .find(|s| s.meta.name == name)
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;
        Ok(entry.resources.iter().map(|(p, _)| p.clone()).collect())
    }

    async fn install(&self, _source: SkillInstallSource) -> Result<SkillMeta, SkillError> {
        Err(SkillError::Io(
            "install is not supported by InMemorySkillRepository".to_string(),
        ))
    }

    async fn remove(&self, _name: &str) -> Result<(), SkillError> {
        Err(SkillError::Io(
            "remove is not supported by InMemorySkillRepository".to_string(),
        ))
    }

    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), SkillError> {
        let mut guard = self
            .skills
            .lock()
            .map_err(|_| SkillError::Io("mutex poisoned".to_string()))?;
        let entry = guard
            .iter_mut()
            .find(|s| s.meta.name == name)
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;
        entry.enabled = enabled;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(name: &str, body: &str) -> InMemorySkill {
        InMemorySkill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} skill"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::Bundled,
            body: body.to_string(),
            resources: vec![],
            enabled: true,
        }
    }

    #[tokio::test]
    async fn test_list_skills() {
        let repo = InMemorySkillRepository::new(vec![
            make_skill("alpha", "alpha body"),
            make_skill("beta", "beta body"),
        ]);
        let skills = repo.list_skills().await.unwrap();
        assert_eq!(skills.len(), 2);
        let names: Vec<_> = skills.iter().map(|s| s.meta.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[tokio::test]
    async fn test_find_skill() {
        let repo = InMemorySkillRepository::new(vec![make_skill("alpha", "alpha body")]);
        let found = repo.find_skill("alpha").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().meta.name, "alpha");
    }

    #[tokio::test]
    async fn test_find_skill_not_found() {
        let repo = InMemorySkillRepository::empty();
        let found = repo.find_skill("missing").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_load_body() {
        let repo =
            InMemorySkillRepository::new(vec![make_skill("alpha", "  alpha body content  ")]);
        let body = repo.load_body("alpha").await.unwrap();
        assert_eq!(body.markdown, "alpha body content");
    }

    #[tokio::test]
    async fn test_load_body_not_found() {
        let repo = InMemorySkillRepository::empty();
        let err = repo.load_body("missing").await.unwrap_err();
        assert!(matches!(err, SkillError::SkillNotFound(_)));
    }

    #[tokio::test]
    async fn test_set_enabled() {
        let repo = InMemorySkillRepository::new(vec![make_skill("alpha", "body")]);
        // Initially enabled
        let skill = repo.find_skill("alpha").await.unwrap().unwrap();
        assert!(skill.enabled);

        // Disable
        repo.set_enabled("alpha", false).await.unwrap();
        let skill = repo.find_skill("alpha").await.unwrap().unwrap();
        assert!(!skill.enabled);

        // Re-enable
        repo.set_enabled("alpha", true).await.unwrap();
        let skill = repo.find_skill("alpha").await.unwrap().unwrap();
        assert!(skill.enabled);
    }

    #[tokio::test]
    async fn test_set_enabled_not_found() {
        let repo = InMemorySkillRepository::empty();
        let err = repo.set_enabled("missing", true).await.unwrap_err();
        assert!(matches!(err, SkillError::SkillNotFound(_)));
    }
}
