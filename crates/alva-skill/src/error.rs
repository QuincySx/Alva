// INPUT:  thiserror
// OUTPUT: SkillError
// POS:    Defines the root error enum for the skill subsystem.
//         MCP-related variants are excluded (belong in alva-mcp).
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("Skill '{0}' not found")]
    SkillNotFound(String),

    #[error("Invalid SKILL.md: {0}")]
    InvalidSkillMd(String),

    #[error("Invalid SKILL.md frontmatter: {0}")]
    InvalidFrontmatter(String),

    #[error("Cannot remove bundled skill '{0}'")]
    CannotRemoveBundledSkill(String),

    #[error("Path traversal attempt: '{0}'")]
    PathTraversal(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(String),
}
