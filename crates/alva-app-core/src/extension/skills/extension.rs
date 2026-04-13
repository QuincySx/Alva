//! SkillsExtension — skill discovery, loading, and context injection.

use std::path::PathBuf;
use std::sync::Arc;

use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

use crate::extension::{Extension, ExtensionContext, HostAPI};
use crate::extension::skills::store::SkillStore;
use crate::extension::skills::loader::SkillLoader;
use crate::extension::skills::injector::SkillInjector;
use crate::extension::skills::tools::{SearchSkillsTool, UseSkillTool};
use crate::extension::skills::middleware::SkillInjectionMiddleware;
use crate::extension::skills::skill_fs::FsSkillRepository;
use crate::extension::skills::skill_ports::skill_repository::SkillRepository;

/// Skill system: discovery, loading, and context injection.
/// Provides SearchSkillsTool + UseSkillTool and SkillInjectionMiddleware.
pub struct SkillsExtension {
    store: Arc<SkillStore>,
    loader: Arc<SkillLoader>,
    injector: Arc<SkillInjector>,
}

impl SkillsExtension {
    /// Create a new SkillsExtension with the given skill directories.
    /// The first directory is used as primary (bundled/mbb/user subdirs).
    pub fn new(skill_dirs: Vec<PathBuf>) -> Self {
        let primary_dir = skill_dirs.first().cloned()
            .unwrap_or_else(|| PathBuf::from(".alva/skills"));

        let repo = Arc::new(FsSkillRepository::new(
            primary_dir.join("bundled"),
            primary_dir.join("mbb"),
            primary_dir.join("user"),
            primary_dir.join("state.json"),
        ));
        let store = Arc::new(SkillStore::new(repo.clone() as Arc<dyn SkillRepository>));
        let loader = Arc::new(SkillLoader::new(repo.clone() as Arc<dyn SkillRepository>));
        let injector = Arc::new(SkillInjector::new(SkillLoader::new(repo as Arc<dyn SkillRepository>)));

        Self { store, loader, injector }
    }
}

#[async_trait]
impl Extension for SkillsExtension {
    fn name(&self) -> &str { "skills" }
    fn description(&self) -> &str { "Skill discovery, loading, and context injection" }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(SearchSkillsTool { store: self.store.clone() }),
            Box::new(UseSkillTool { store: self.store.clone(), loader: self.loader.clone() }),
        ]
    }

    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(SkillInjectionMiddleware::with_defaults(
            self.store.clone(),
            self.injector.clone(),
        )));
    }

    async fn configure(&self, _ctx: &ExtensionContext) {
        let _ = self.store.scan().await;
    }
}
