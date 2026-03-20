use crate::{
    application::skill_loader::SkillLoader,
    domain::{
        skill::Skill,
        skill_config::{InjectionPolicy, SkillRef},
    },
    error::SkillError,
};

/// Injects Skill content into Agent system prompt
///
/// Injection result is appended as a section of system prompt.
/// AgentEngine concatenates it with AgentConfig::system_prompt_base when building LLMRequest.
pub struct SkillInjector {
    loader: SkillLoader,
}

impl SkillInjector {
    pub fn new(loader: SkillLoader) -> Self {
        Self { loader }
    }

    /// Build complete system prompt injection block for a set of SkillRefs
    ///
    /// Injection strategies:
    /// - Auto:     inject only Level 1 metadata summary table (description + trigger)
    /// - Explicit: inject Level 1 metadata + full SKILL.md body
    /// - Strict:   same as Explicit, additionally declares tool restrictions in prompt
    pub async fn build_injection(
        &self,
        skill_refs: &[SkillRef],
        available_skills: &[Skill],
    ) -> Result<String, SkillError> {
        let mut auto_skills: Vec<Skill> = vec![];
        let mut explicit_skills: Vec<(&Skill, &SkillRef)> = vec![];

        for skill_ref in skill_refs {
            let Some(skill) = available_skills.iter().find(|s| s.meta.name == skill_ref.name)
            else {
                continue; // Not found -- skip, lenient policy
            };
            if !skill.enabled {
                continue;
            }

            let policy = skill_ref
                .injection
                .as_ref()
                .unwrap_or(&InjectionPolicy::Auto);

            match policy {
                InjectionPolicy::Auto => auto_skills.push(skill.clone()),
                InjectionPolicy::Explicit | InjectionPolicy::Strict => {
                    explicit_skills.push((skill, skill_ref));
                }
            }
        }

        let mut parts: Vec<String> = vec![];

        // 1. Auto mode: aggregate into metadata summary table
        if !auto_skills.is_empty() {
            let meta_summary = self.loader.build_meta_summary(&auto_skills).await?;
            if !meta_summary.is_empty() {
                parts.push(meta_summary);
            }
        }

        // 2. Explicit/Strict mode: inline-expand each Skill's full content
        for (skill, skill_ref) in &explicit_skills {
            let injected = self.loader.build_explicit_injection(skill).await?;
            parts.push(injected);

            // Strict mode: declare tool constraints in prompt
            let policy = skill_ref
                .injection
                .as_ref()
                .unwrap_or(&InjectionPolicy::Auto);
            if *policy == InjectionPolicy::Strict {
                if let Some(allowed_tools) = &skill.meta.allowed_tools {
                    parts.push(format!(
                        "> [Skill: {}] Strict mode: only use tools: {}\n",
                        skill.meta.name,
                        allowed_tools.join(", ")
                    ));
                }
            }
        }

        Ok(parts.join("\n\n"))
    }
}
