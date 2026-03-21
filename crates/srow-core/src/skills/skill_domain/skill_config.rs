// INPUT:  serde
// OUTPUT: SkillRef, InjectionPolicy
// POS:    Defines Skill reference and injection strategy (Auto/Explicit/Strict) for system prompt composition.
use serde::{Deserialize, Serialize};

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
