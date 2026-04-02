// INPUT:  serde, serde_yaml, std::collections::HashMap, std::path::PathBuf
// OUTPUT: pub struct SkillMeta, pub struct SkillBody, pub struct SkillResource, pub enum ResourceContentType, pub struct Skill, pub enum SkillKind, pub struct SkillRef, pub enum InjectionPolicy
// POS:    Defines core Skill entity types across three loading levels (metadata, instructions, resources) plus reference and injection strategy for system prompt composition.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Skill metadata (Level 1 -- always resident in context)
/// Corresponds to SKILL.md YAML frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    /// kebab-case, [a-z0-9-], max 64 chars
    pub name: String,
    /// Trigger description, max 1024 chars, no angle brackets
    /// Sole basis for Agent to decide whether to activate this Skill
    pub description: String,
    pub license: Option<String>,
    /// Tool whitelist for this Skill (None = unrestricted)
    pub allowed_tools: Option<Vec<String>>,
    /// Extension metadata (version, author, compatibility, etc.)
    pub metadata: Option<HashMap<String, serde_yaml::Value>>,
}

/// Skill instruction layer (Level 2 -- loaded after trigger)
/// Corresponds to SKILL.md Markdown body (everything after frontmatter)
#[derive(Debug, Clone)]
pub struct SkillBody {
    /// Raw Markdown text of SKILL.md body
    pub markdown: String,
    /// Estimated token count (for context management)
    pub estimated_tokens: u32,
}

/// Single resource file (Level 3 -- loaded on demand)
#[derive(Debug, Clone)]
pub struct SkillResource {
    /// Path relative to skill root directory (e.g. "references/api.md")
    pub relative_path: String,
    pub content: Vec<u8>,
    pub content_type: ResourceContentType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceContentType {
    Markdown,
    Python,
    JavaScript,
    TypeScript,
    Shell,
    Binary,
    Other(String),
}

/// Complete Skill representation (in-memory)
#[derive(Debug, Clone)]
pub struct Skill {
    pub meta: SkillMeta,
    /// Skill type
    pub kind: SkillKind,
    /// Skill root directory path (extracted)
    pub root_path: PathBuf,
    /// Whether enabled
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillKind {
    /// Bundled with app (bundled-skills/)
    Bundled,
    /// Browser-enhanced Skill bound to domains (mbb-skills/)
    Mbb {
        /// Bound domain list, e.g. ["12306.cn"]
        domains: Vec<String>,
    },
    /// User-installed Skill (user skill directory)
    UserInstalled,
}

/// Skill reference: declares usage of a Skill within AgentTemplate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRef {
    /// Corresponds to SkillMeta::name
    pub name: String,
    /// Override injection policy (None = use global default)
    pub injection: Option<InjectionPolicy>,
}

/// Skill injection strategy into system prompt
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionPolicy {
    /// Explicit injection: inject SKILL.md body in full into system prompt
    /// For core skills, ensures Agent always perceives this skill
    Explicit,
    /// Auto injection: inject only description (metadata layer),
    /// Agent uses `use_skill` tool to pull full content on demand
    Auto,
    /// Strict injection: same as explicit, but also restricts Agent
    /// to only use this Skill's allowed_tools
    Strict,
}

impl Default for InjectionPolicy {
    fn default() -> Self {
        InjectionPolicy::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SkillMeta ───────────────────────────────────────────────────────

    #[test]
    fn skill_meta_minimal_construction() {
        let meta = SkillMeta {
            name: "test-skill".into(),
            description: "A test skill".into(),
            license: None,
            allowed_tools: None,
            metadata: None,
        };
        assert_eq!(meta.name, "test-skill");
        assert_eq!(meta.description, "A test skill");
        assert!(meta.license.is_none());
        assert!(meta.allowed_tools.is_none());
        assert!(meta.metadata.is_none());
    }

    #[test]
    fn skill_meta_full_construction() {
        let meta = SkillMeta {
            name: "advanced-skill".into(),
            description: "An advanced skill".into(),
            license: Some("MIT".into()),
            allowed_tools: Some(vec!["read_file".into(), "write_file".into()]),
            metadata: Some(HashMap::from([(
                "version".into(),
                serde_yaml::Value::String("1.0.0".into()),
            )])),
        };
        assert_eq!(meta.license.as_deref(), Some("MIT"));
        assert_eq!(meta.allowed_tools.as_ref().unwrap().len(), 2);
        assert!(meta.metadata.is_some());
    }

    #[test]
    fn skill_meta_serde_roundtrip() {
        let meta = SkillMeta {
            name: "serde-test".into(),
            description: "Test serde".into(),
            license: Some("Apache-2.0".into()),
            allowed_tools: Some(vec!["bash".into()]),
            metadata: None,
        };
        let yaml = serde_yaml::to_string(&meta).unwrap();
        let parsed: SkillMeta = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, "serde-test");
        assert_eq!(parsed.license, Some("Apache-2.0".into()));
    }

    // ── SkillBody ───────────────────────────────────────────────────────

    #[test]
    fn skill_body_construction() {
        let body = SkillBody {
            markdown: "# Instructions\nDo this.".into(),
            estimated_tokens: 6,
        };
        assert!(body.markdown.starts_with("# Instructions"));
        assert_eq!(body.estimated_tokens, 6);
    }

    // ── SkillResource ───────────────────────────────────────────────────

    #[test]
    fn skill_resource_construction() {
        let resource = SkillResource {
            relative_path: "references/api.md".into(),
            content: b"# API Reference".to_vec(),
            content_type: ResourceContentType::Markdown,
        };
        assert_eq!(resource.relative_path, "references/api.md");
        assert_eq!(resource.content_type, ResourceContentType::Markdown);
    }

    // ── ResourceContentType ─────────────────────────────────────────────

    #[test]
    fn resource_content_type_equality() {
        assert_eq!(ResourceContentType::Markdown, ResourceContentType::Markdown);
        assert_eq!(ResourceContentType::Python, ResourceContentType::Python);
        assert_ne!(ResourceContentType::Python, ResourceContentType::JavaScript);
        assert_eq!(
            ResourceContentType::Other("toml".into()),
            ResourceContentType::Other("toml".into())
        );
        assert_ne!(
            ResourceContentType::Other("toml".into()),
            ResourceContentType::Other("yaml".into())
        );
    }

    // ── SkillKind ───────────────────────────────────────────────────────

    #[test]
    fn skill_kind_serde_roundtrip() {
        let variants = vec![
            SkillKind::Bundled,
            SkillKind::Mbb {
                domains: vec!["example.com".into(), "test.org".into()],
            },
            SkillKind::UserInstalled,
        ];
        for kind in &variants {
            let json = serde_json::to_string(kind).unwrap();
            let parsed: SkillKind = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, kind);
        }
    }

    // ── SkillRef ────────────────────────────────────────────────────────

    #[test]
    fn skill_ref_with_injection_policy() {
        let skill_ref = SkillRef {
            name: "my-skill".into(),
            injection: Some(InjectionPolicy::Explicit),
        };
        assert_eq!(skill_ref.name, "my-skill");
        assert_eq!(skill_ref.injection, Some(InjectionPolicy::Explicit));
    }

    #[test]
    fn skill_ref_without_injection_policy() {
        let skill_ref = SkillRef {
            name: "default-skill".into(),
            injection: None,
        };
        assert!(skill_ref.injection.is_none());
    }

    // ── InjectionPolicy ─────────────────────────────────────────────────

    #[test]
    fn injection_policy_default_is_auto() {
        assert_eq!(InjectionPolicy::default(), InjectionPolicy::Auto);
    }

    #[test]
    fn injection_policy_serde_roundtrip() {
        let policies = vec![
            InjectionPolicy::Auto,
            InjectionPolicy::Explicit,
            InjectionPolicy::Strict,
        ];
        for policy in &policies {
            let json = serde_json::to_string(policy).unwrap();
            let parsed: InjectionPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, policy);
        }
    }

    #[test]
    fn injection_policy_serde_snake_case() {
        let json = serde_json::to_string(&InjectionPolicy::Auto).unwrap();
        assert_eq!(json, "\"auto\"");
        let json = serde_json::to_string(&InjectionPolicy::Explicit).unwrap();
        assert_eq!(json, "\"explicit\"");
        let json = serde_json::to_string(&InjectionPolicy::Strict).unwrap();
        assert_eq!(json, "\"strict\"");
    }

    // ── Skill (complete) ────────────────────────────────────────────────

    #[test]
    fn skill_construction() {
        let skill = Skill {
            meta: SkillMeta {
                name: "browser-skill".into(),
                description: "Automates browser".into(),
                license: None,
                allowed_tools: Some(vec!["navigate".into()]),
                metadata: None,
            },
            kind: SkillKind::Mbb {
                domains: vec!["example.com".into()],
            },
            root_path: PathBuf::from("/skills/browser-skill"),
            enabled: true,
        };
        assert!(skill.enabled);
        assert_eq!(skill.meta.name, "browser-skill");
        assert_eq!(skill.root_path, PathBuf::from("/skills/browser-skill"));
    }
}
