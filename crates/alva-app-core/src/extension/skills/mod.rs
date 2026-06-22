// INPUT:  (none)
// OUTPUT: SkillsPlugin (+ submodules: skill_domain, skill_ports, loader, store, injector, skill_fs, agent_template_service, middleware, tools)
// POS:    Skill system plugin — discovery, loading, injection + Extension impl.
pub mod agent_template_service;
pub mod injector;
pub mod loader;
pub mod middleware;
pub mod skill_domain;
pub mod skill_fs;
pub mod skill_ports;
pub mod store;
pub mod tools;

mod extension;
pub use extension::SkillsPlugin;
