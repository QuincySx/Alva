// INPUT:  alva_protocol_skill::types
// OUTPUT: pub ResourceContentType, Skill, SkillBody, SkillKind, SkillMeta, SkillResource
// POS:    Re-exports core Skill domain types from alva-protocol-skill.
// Re-export from protocol crate — single source of truth
pub use alva_protocol_skill::types::{
    ResourceContentType, Skill, SkillBody, SkillKind, SkillMeta, SkillResource,
};
