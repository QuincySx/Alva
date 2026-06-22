// INPUT:  thiserror
// OUTPUT: SkillError
// POS:    Defines the root error enum for the skill subsystem.
//         MCP-related variants are excluded (belong in alva-protocol-mcp).
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

#[cfg(test)]
mod tests {
    //! Tests for SkillError Display interpolation.
    //!
    //! Mirror of the mcp/error.rs test pattern — pure Display pin so
    //! that a dropped `{0}` placeholder doesn't surface "Skill '' not
    //! found"-style garbage to users. PathTraversal is intentionally
    //! a separate pin: security audits + user diagnostics both need
    //! the attempted path string in the message verbatim.
    use super::*;

    #[test]
    fn skill_not_found_quotes_name() {
        let e = SkillError::SkillNotFound("autonomous-ui-test".into());
        assert_eq!(e.to_string(), "Skill 'autonomous-ui-test' not found");
    }

    #[test]
    fn invalid_skill_md_includes_payload() {
        let e = SkillError::InvalidSkillMd("missing closing --- delimiter".into());
        assert_eq!(
            e.to_string(),
            "Invalid SKILL.md: missing closing --- delimiter"
        );
    }

    #[test]
    fn invalid_frontmatter_includes_payload() {
        let e = SkillError::InvalidFrontmatter("expected 'name' key".into());
        assert_eq!(
            e.to_string(),
            "Invalid SKILL.md frontmatter: expected 'name' key"
        );
    }

    #[test]
    fn cannot_remove_bundled_skill_quotes_name() {
        // Pin: bundled skills can't be removed — error message MUST
        // include the skill name so users know what to leave alone.
        let e = SkillError::CannotRemoveBundledSkill("autonomous-ui-test".into());
        assert_eq!(
            e.to_string(),
            "Cannot remove bundled skill 'autonomous-ui-test'"
        );
    }

    #[test]
    fn path_traversal_includes_attempted_path_for_audit() {
        // SECURITY-relevant pin: PathTraversal error message must
        // include the attempted path so security audits + user
        // diagnostics can see what was rejected. A future refactor
        // that dropped the path would lose audit evidence.
        let e = SkillError::PathTraversal("../etc/passwd".into());
        assert_eq!(e.to_string(), "Path traversal attempt: '../etc/passwd'");
    }

    #[test]
    fn serialization_includes_payload() {
        let e = SkillError::Serialization("unexpected EOF at line 5".into());
        assert_eq!(
            e.to_string(),
            "Serialization error: unexpected EOF at line 5"
        );
    }

    #[test]
    fn io_includes_payload() {
        let e = SkillError::Io("permission denied".into());
        assert_eq!(e.to_string(), "IO error: permission denied");
    }

    #[test]
    fn all_variants_implement_debug() {
        // Smoke: Debug works for all variants (used in logs + panic
        // messages throughout).
        let variants = vec![
            SkillError::SkillNotFound("a".into()),
            SkillError::InvalidSkillMd("b".into()),
            SkillError::InvalidFrontmatter("c".into()),
            SkillError::CannotRemoveBundledSkill("d".into()),
            SkillError::PathTraversal("e".into()),
            SkillError::Serialization("f".into()),
            SkillError::Io("g".into()),
        ];
        for v in &variants {
            assert!(!format!("{v:?}").is_empty());
        }
    }
}
