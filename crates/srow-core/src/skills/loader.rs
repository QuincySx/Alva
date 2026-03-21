// INPUT:  std::sync, crate::skills::skill_domain::skill, crate::error, crate::skills::skill_ports::skill_repository
// OUTPUT: SkillLoader
// POS:    Three-level progressive Skill loader: metadata summary, SKILL.md body, and resource files.
use std::sync::Arc;

use crate::{
    skills::skill_domain::skill::{Skill, SkillBody, SkillResource},
    error::SkillError,
    skills::skill_ports::skill_repository::SkillRepository,
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
