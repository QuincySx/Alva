// INPUT:  alva_protocol_skill::repository
// OUTPUT: pub SkillInstallSource, SkillRepository
// POS:    Re-exports the SkillRepository trait and SkillInstallSource enum.
// Re-export from protocol crate — single source of truth
pub use alva_protocol_skill::repository::{SkillInstallSource, SkillRepository};
