// INPUT:  serde, std::collections, std::path
// OUTPUT: SkillMeta, SkillBody, SkillResource, ResourceContentType, Skill, SkillKind
// POS:    Defines core Skill entity types across three loading levels: metadata, instructions, and resources.
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
