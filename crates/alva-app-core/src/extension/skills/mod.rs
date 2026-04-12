// INPUT:  (none)
// OUTPUT: SkillsExtension (+ submodules: skill_domain, skill_ports, loader, store, injector, skill_fs, agent_template_service, middleware, tools)
// POS:    Skill system plugin — discovery, loading, injection + Extension impl.
pub mod skill_domain;
pub mod skill_ports;
pub mod loader;
pub mod store;
pub mod injector;
pub mod skill_fs;
pub mod agent_template_service;
pub mod middleware;
pub mod tools;

mod extension;
pub use extension::SkillsExtension;
