// INPUT:  std::sync, crate::types, crate::error, crate::repository
// OUTPUT: SkillLoader
// POS:    Three-level progressive Skill loader: metadata summary, SKILL.md body, and resource files.
use std::sync::Arc;

use crate::{
    error::SkillError,
    repository::SkillRepository,
    types::{Skill, SkillBody, SkillResource},
};

/// Three-level progressive Skill loader
///
/// Level 1 (metadata): always read from memory / index, no disk I/O
/// Level 2 (instructions): read SKILL.md body on demand after user prompt triggers Skill
/// Level 3 (resources): Agent loads via use_skill(level="full") or reads references/ files
pub struct SkillLoader {
    repo: Arc<dyn SkillRepository>,
}

impl SkillLoader {
    pub fn new(repo: Arc<dyn SkillRepository>) -> Self {
        Self { repo }
    }

    /// Build Level 1 context fragment (metadata summary table)
    /// Format follows Wukong's skill list injection: compact list of name + description
    /// This fragment is always injected into system prompt, ~50-150 tokens
    pub async fn build_meta_summary(
        &self,
        skills: &[Skill],
    ) -> Result<String, SkillError> {
        if skills.is_empty() {
            return Ok(String::new());
        }

        let mut lines = vec![
            "## Available Skills".to_string(),
            String::new(),
            "The following skills are available. Use `use_skill` to load full instructions."
                .to_string(),
            String::new(),
        ];

        for skill in skills.iter().filter(|s| s.enabled) {
            lines.push(format!(
                "- **{}**: {}",
                skill.meta.name, skill.meta.description
            ));
        }

        Ok(lines.join("\n"))
    }

    /// Build Level 2 context fragment (single Skill's full SKILL.md body)
    /// Called when user prompt triggers this Skill
    pub async fn load_skill_body(&self, name: &str) -> Result<SkillBody, SkillError> {
        self.repo.load_body(name).await
    }

    /// Build Level 2 inline injection (Explicit/Strict mode pre-injection)
    /// Expands SKILL.md body directly in system prompt
    pub async fn build_explicit_injection(
        &self,
        skill: &Skill,
    ) -> Result<String, SkillError> {
        let body = self.repo.load_body(&skill.meta.name).await?;
        Ok(format!(
            "## Skill: {}\n\n{}\n",
            skill.meta.name, body.markdown
        ))
    }

    /// Load resource file (Level 3)
    /// Triggered by use_skill(level="full") tool or Agent's direct read_file call
    pub async fn load_resource(
        &self,
        skill_name: &str,
        relative_path: &str,
    ) -> Result<SkillResource, SkillError> {
        self.repo.load_resource(skill_name, relative_path).await
    }

    /// List all resource paths for a Skill (for Agent's selective loading)
    pub async fn list_resources(
        &self,
        skill_name: &str,
    ) -> Result<Vec<String>, SkillError> {
        self.repo.list_resources(skill_name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{InMemorySkill, InMemorySkillRepository};
    use crate::types::{SkillKind, SkillMeta};
    use std::path::PathBuf;

    fn make_repo_skill(name: &str, body: &str, resources: Vec<(String, Vec<u8>)>) -> InMemorySkill {
        InMemorySkill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} skill description"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::Bundled,
            body: body.to_string(),
            resources,
            enabled: true,
        }
    }

    fn make_disabled_repo_skill(name: &str) -> InMemorySkill {
        InMemorySkill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} disabled skill"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::Bundled,
            body: "disabled body".into(),
            resources: vec![],
            enabled: false,
        }
    }

    fn make_skill_domain(name: &str) -> Skill {
        Skill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} skill description"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::Bundled,
            root_path: PathBuf::from("/in-memory"),
            enabled: true,
        }
    }

    fn make_disabled_skill_domain(name: &str) -> Skill {
        Skill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} disabled skill"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::Bundled,
            root_path: PathBuf::from("/in-memory"),
            enabled: false,
        }
    }

    fn make_loader(skills: Vec<InMemorySkill>) -> SkillLoader {
        SkillLoader::new(Arc::new(InMemorySkillRepository::new(skills)))
    }

    // ── Level 1: build_meta_summary ─────────────────────────────────────

    #[tokio::test]
    async fn meta_summary_empty_skills() {
        let loader = make_loader(vec![]);
        let summary = loader.build_meta_summary(&[]).await.unwrap();
        assert!(summary.is_empty());
    }

    #[tokio::test]
    async fn meta_summary_includes_enabled_skills() {
        let loader = make_loader(vec![
            make_repo_skill("alpha", "body", vec![]),
            make_repo_skill("beta", "body", vec![]),
        ]);
        let skills = vec![make_skill_domain("alpha"), make_skill_domain("beta")];

        let summary = loader.build_meta_summary(&skills).await.unwrap();
        assert!(summary.contains("## Available Skills"));
        assert!(summary.contains("**alpha**"));
        assert!(summary.contains("**beta**"));
    }

    #[tokio::test]
    async fn meta_summary_excludes_disabled_skills() {
        let loader = make_loader(vec![
            make_repo_skill("enabled-one", "body", vec![]),
            make_disabled_repo_skill("disabled-one"),
        ]);
        let skills = vec![
            make_skill_domain("enabled-one"),
            make_disabled_skill_domain("disabled-one"),
        ];

        let summary = loader.build_meta_summary(&skills).await.unwrap();
        assert!(summary.contains("**enabled-one**"));
        assert!(!summary.contains("**disabled-one**"));
    }

    // ── Level 2: load_skill_body ────────────────────────────────────────

    #[tokio::test]
    async fn load_skill_body_returns_trimmed_markdown() {
        let loader = make_loader(vec![make_repo_skill(
            "alpha",
            "  # Instructions\nDo this.  ",
            vec![],
        )]);

        let body = loader.load_skill_body("alpha").await.unwrap();
        assert_eq!(body.markdown, "# Instructions\nDo this.");
        assert!(body.estimated_tokens > 0);
    }

    #[tokio::test]
    async fn load_skill_body_not_found() {
        let loader = make_loader(vec![]);
        let err = loader.load_skill_body("missing").await.unwrap_err();
        assert!(matches!(err, SkillError::SkillNotFound(_)));
    }

    // ── Level 2: build_explicit_injection ───────────────────────────────

    #[tokio::test]
    async fn explicit_injection_includes_header_and_body() {
        let loader = make_loader(vec![make_repo_skill(
            "my-skill",
            "Step 1: do X\nStep 2: do Y",
            vec![],
        )]);
        let skill = make_skill_domain("my-skill");

        let injection = loader.build_explicit_injection(&skill).await.unwrap();
        assert!(injection.starts_with("## Skill: my-skill"));
        assert!(injection.contains("Step 1: do X"));
        assert!(injection.contains("Step 2: do Y"));
    }

    // ── Level 3: load_resource ──────────────────────────────────────────

    #[tokio::test]
    async fn load_resource_returns_content() {
        let loader = make_loader(vec![make_repo_skill(
            "alpha",
            "body",
            vec![("refs/api.md".into(), b"# API".to_vec())],
        )]);

        let resource = loader.load_resource("alpha", "refs/api.md").await.unwrap();
        assert_eq!(resource.relative_path, "refs/api.md");
        assert_eq!(resource.content, b"# API");
    }

    #[tokio::test]
    async fn load_resource_not_found() {
        let loader = make_loader(vec![make_repo_skill("alpha", "body", vec![])]);
        let err = loader
            .load_resource("alpha", "missing.md")
            .await
            .unwrap_err();
        assert!(matches!(err, SkillError::Io(_)));
    }

    // ── Level 3: list_resources ─────────────────────────────────────────

    #[tokio::test]
    async fn list_resources_returns_paths() {
        let loader = make_loader(vec![make_repo_skill(
            "alpha",
            "body",
            vec![
                ("scripts/run.sh".into(), b"#!/bin/sh".to_vec()),
                ("refs/api.md".into(), b"api".to_vec()),
            ],
        )]);

        let paths = loader.list_resources("alpha").await.unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"scripts/run.sh".to_string()));
        assert!(paths.contains(&"refs/api.md".to_string()));
    }

    #[tokio::test]
    async fn list_resources_skill_not_found() {
        let loader = make_loader(vec![]);
        let err = loader.list_resources("missing").await.unwrap_err();
        assert!(matches!(err, SkillError::SkillNotFound(_)));
    }
}
