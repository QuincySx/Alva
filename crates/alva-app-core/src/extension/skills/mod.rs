// INPUT:  (none)
// OUTPUT: SkillsPlugin (+ submodules: skill_domain, skill_ports, loader, store, injector, skill_fs, agent_template_service, tools)
// POS:    Skill system plugin — stable directory discovery and unified named invocation.
pub mod agent_template_service;
pub mod injector;
pub mod loader;
pub mod skill_domain;
pub mod skill_fs;
pub mod skill_ports;
pub mod store;
pub mod tools;

mod extension;
pub use extension::SkillsPlugin;
