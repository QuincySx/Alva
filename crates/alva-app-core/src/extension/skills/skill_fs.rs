// INPUT:  alva_protocol_skill::fs
// OUTPUT: pub FsSkillRepository
// POS:    Re-exports the filesystem-based SkillRepository implementation.
// Re-export from protocol crate — single source of truth
pub use alva_protocol_skill::fs::FsSkillRepository;
