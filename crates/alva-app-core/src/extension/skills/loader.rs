// INPUT:  alva_protocol_skill::loader
// OUTPUT: pub SkillLoader
// POS:    Re-exports the SkillLoader from alva-protocol-skill for loading skill content.
// Re-export from protocol crate — single source of truth
pub use alva_protocol_skill::loader::SkillLoader;
