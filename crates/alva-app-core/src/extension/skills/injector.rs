// INPUT:  alva_protocol_skill::injector
// OUTPUT: pub SkillInjector
// POS:    Re-exports the SkillInjector from alva-protocol-skill for system prompt injection.
// Re-export from protocol crate — single source of truth
pub use alva_protocol_skill::injector::SkillInjector;
