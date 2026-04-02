// INPUT:  crate::loader, crate::types, crate::error
// OUTPUT: SkillInjector
// POS:    Builds system prompt injection blocks from SkillRefs using Auto/Explicit/Strict injection policies.
use crate::{
    error::SkillError,
    loader::SkillLoader,
    types::{InjectionPolicy, Skill, SkillRef},
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{InMemorySkill, InMemorySkillRepository};
    use crate::types::{SkillKind, SkillMeta};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_repo_skill(name: &str, body: &str, allowed_tools: Option<Vec<String>>) -> InMemorySkill {
        InMemorySkill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} skill"),
                license: None,
                allowed_tools,
                metadata: None,
            },
            kind: SkillKind::Bundled,
            body: body.to_string(),
            resources: vec![],
            enabled: true,
        }
    }

    fn make_skill_domain(name: &str, allowed_tools: Option<Vec<String>>) -> Skill {
        Skill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} skill"),
                license: None,
                allowed_tools,
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
                description: format!("{name} skill"),
                license: None,
                allowed_tools: None,
                metadata: None,
            },
            kind: SkillKind::Bundled,
            root_path: PathBuf::from("/in-memory"),
            enabled: false,
        }
    }

    fn make_injector(repo_skills: Vec<InMemorySkill>) -> SkillInjector {
        let repo = Arc::new(InMemorySkillRepository::new(repo_skills));
        let loader = SkillLoader::new(repo);
        SkillInjector::new(loader)
    }

    // ── Auto policy ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn auto_policy_produces_meta_summary() {
        let injector = make_injector(vec![
            make_repo_skill("alpha", "alpha body", None),
            make_repo_skill("beta", "beta body", None),
        ]);

        let refs = vec![
            SkillRef {
                name: "alpha".into(),
                injection: Some(InjectionPolicy::Auto),
            },
            SkillRef {
                name: "beta".into(),
                injection: None, // defaults to Auto
            },
        ];

        let skills = vec![
            make_skill_domain("alpha", None),
            make_skill_domain("beta", None),
        ];

        let result = injector.build_injection(&refs, &skills).await.unwrap();
        assert!(result.contains("## Available Skills"));
        assert!(result.contains("**alpha**"));
        assert!(result.contains("**beta**"));
        // Auto should NOT include the body
        assert!(!result.contains("alpha body"));
    }

    // ── Explicit policy ─────────────────────────────────────────────────

    #[tokio::test]
    async fn explicit_policy_includes_full_body() {
        let injector = make_injector(vec![make_repo_skill(
            "code-review",
            "Review all changes carefully.",
            None,
        )]);

        let refs = vec![SkillRef {
            name: "code-review".into(),
            injection: Some(InjectionPolicy::Explicit),
        }];

        let skills = vec![make_skill_domain("code-review", None)];

        let result = injector.build_injection(&refs, &skills).await.unwrap();
        assert!(result.contains("## Skill: code-review"));
        assert!(result.contains("Review all changes carefully."));
    }

    // ── Strict policy ───────────────────────────────────────────────────

    #[tokio::test]
    async fn strict_policy_includes_body_and_tool_restrictions() {
        let allowed = vec!["read_file".into(), "bash".into()];
        let injector = make_injector(vec![make_repo_skill(
            "secure-skill",
            "Only use approved tools.",
            Some(allowed.clone()),
        )]);

        let refs = vec![SkillRef {
            name: "secure-skill".into(),
            injection: Some(InjectionPolicy::Strict),
        }];

        let skills = vec![make_skill_domain("secure-skill", Some(allowed))];

        let result = injector.build_injection(&refs, &skills).await.unwrap();
        assert!(result.contains("## Skill: secure-skill"));
        assert!(result.contains("Only use approved tools."));
        assert!(result.contains("Strict mode: only use tools: read_file, bash"));
    }

    #[tokio::test]
    async fn strict_policy_without_allowed_tools_no_restriction_line() {
        let injector = make_injector(vec![make_repo_skill(
            "open-strict",
            "Strict but unrestricted.",
            None,
        )]);

        let refs = vec![SkillRef {
            name: "open-strict".into(),
            injection: Some(InjectionPolicy::Strict),
        }];

        let skills = vec![make_skill_domain("open-strict", None)];

        let result = injector.build_injection(&refs, &skills).await.unwrap();
        assert!(result.contains("## Skill: open-strict"));
        // No tool restriction line
        assert!(!result.contains("Strict mode"));
    }

    // ── Mixed policies ──────────────────────────────────────────────────

    #[tokio::test]
    async fn mixed_auto_and_explicit() {
        let injector = make_injector(vec![
            make_repo_skill("auto-skill", "auto body", None),
            make_repo_skill("explicit-skill", "explicit body content", None),
        ]);

        let refs = vec![
            SkillRef {
                name: "auto-skill".into(),
                injection: Some(InjectionPolicy::Auto),
            },
            SkillRef {
                name: "explicit-skill".into(),
                injection: Some(InjectionPolicy::Explicit),
            },
        ];

        let skills = vec![
            make_skill_domain("auto-skill", None),
            make_skill_domain("explicit-skill", None),
        ];

        let result = injector.build_injection(&refs, &skills).await.unwrap();

        // Auto part: meta summary only
        assert!(result.contains("## Available Skills"));
        assert!(result.contains("**auto-skill**"));
        assert!(!result.contains("auto body"));

        // Explicit part: full body
        assert!(result.contains("## Skill: explicit-skill"));
        assert!(result.contains("explicit body content"));
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn missing_skill_ref_is_skipped() {
        let injector = make_injector(vec![]);

        let refs = vec![SkillRef {
            name: "nonexistent".into(),
            injection: Some(InjectionPolicy::Auto),
        }];

        let result = injector.build_injection(&refs, &[]).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn disabled_skill_is_skipped() {
        let injector = make_injector(vec![make_repo_skill("dis", "body", None)]);

        let refs = vec![SkillRef {
            name: "dis".into(),
            injection: Some(InjectionPolicy::Explicit),
        }];

        let skills = vec![make_disabled_skill_domain("dis")];

        let result = injector.build_injection(&refs, &skills).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn empty_refs_produces_empty_output() {
        let injector = make_injector(vec![]);
        let result = injector.build_injection(&[], &[]).await.unwrap();
        assert!(result.is_empty());
    }
}
