// INPUT:  std::{path, sync}, alva_agent_core, alva_agent_extension_builtin::skill_tool, alva_kernel_abi, crate::extension::skills
// OUTPUT: SkillsPlugin
// POS:    Scans skills, publishes the real registry, registers the unified `skill` tool, and contributes the auto directory to stable context.
//! SkillsPlugin — skill discovery, loading, and context injection.

use std::path::PathBuf;
use std::sync::Arc;

use alva_agent_extension_builtin::skill_tool::{SkillRegistry, SkillTool};
use alva_kernel_abi::scope::context::ContextLayer;
use async_trait::async_trait;

use crate::extension::mcp::runtime::McpManager;
use crate::extension::skills::agent_template_service::AgentTemplateService;
use crate::extension::skills::injector::SkillInjector;
use crate::extension::skills::loader::SkillLoader;
use crate::extension::skills::skill_domain::agent_template::GlobalSkillConfig;
use crate::extension::skills::skill_domain::skill::SkillInvocation;
use crate::extension::skills::skill_fs::FsSkillRepository;
use crate::extension::skills::skill_ports::skill_repository::SkillRepository;
use crate::extension::skills::store::SkillStore;
use crate::extension::skills::tools::SkillService;
use crate::extension::{Plugin, Registrar};

/// Skill system: discovery, always-present directory, and named invocation.
pub struct SkillsPlugin {
    store: Arc<SkillStore>,
    loader: Arc<SkillLoader>,
    injector: Arc<SkillInjector>,
}

impl SkillsPlugin {
    /// Create a new SkillsPlugin with the given skill directories.
    /// The first directory is used as primary (bundled/mbb/user subdirs).
    ///
    /// **Note**: only the first directory in `skill_dirs` is consulted.
    /// Use [`Self::with_bundled`] when the App-bundled skills tree lives
    /// elsewhere (e.g. extracted from the binary into a cache dir).
    pub fn new(skill_dirs: Vec<PathBuf>) -> Self {
        let primary = skill_dirs
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(".alva/skills"));
        Self::with_bundled(primary, None)
    }

    /// Construct from an explicit primary skill dir (containing
    /// `mbb/`, `user/`, `state.json`) plus an optional override for the
    /// `bundled/` directory. The override is what makes binary-bundled
    /// skills work: the App extracts its embedded skill tree to a cache
    /// dir, then passes that path here so the repo scans it as the
    /// bundled source instead of looking for a `<primary>/bundled/`
    /// subdir that the user never created.
    pub fn with_bundled(primary: PathBuf, bundled_override: Option<PathBuf>) -> Self {
        let bundled_dir = bundled_override.unwrap_or_else(|| primary.join("bundled"));
        let repo = Arc::new(FsSkillRepository::new(
            bundled_dir,
            primary.join("mbb"),
            primary.join("user"),
            primary.join("state.json"),
        ));
        let store = Arc::new(SkillStore::new(repo.clone() as Arc<dyn SkillRepository>));
        let loader = Arc::new(SkillLoader::new(repo.clone() as Arc<dyn SkillRepository>));
        let injector = Arc::new(SkillInjector::new(SkillLoader::new(
            repo as Arc<dyn SkillRepository>,
        )));

        Self {
            store,
            loader,
            injector,
        }
    }
}

#[async_trait]
impl Plugin for SkillsPlugin {
    fn name(&self) -> &str {
        "skills"
    }
    fn description(&self) -> &str {
        "Skill discovery, loading, and context injection"
    }

    async fn register(&self, r: &Registrar) {
        if let Err(e) = self.store.scan().await {
            tracing::warn!(error = %e, "skills: initial scan failed; skills may be unavailable");
        }

        // Invocation is one path everywhere: the builtin `skill` tool reads
        // this registry capability from the runtime bus, while the REPL uses
        // the same service directly and bypasses model-side tool selection.
        let service = Arc::new(SkillService::new(self.store.clone(), self.injector.clone()));
        r.provide::<SkillService>(service.clone());
        r.provide::<dyn SkillRegistry>(service);
        r.tool(SkillTool);

        // Level 1 directory: enabled auto skills only, stable for the lifetime
        // of this assembled agent. Exact-name explicit invocation remains
        // available through SkillService without advertising it here.
        let auto_skills = self
            .store
            .list()
            .await
            .into_iter()
            .filter(|skill| skill.enabled && skill.meta.invocation == SkillInvocation::Auto)
            .collect::<Vec<_>>();
        match self.loader.build_meta_summary(&auto_skills).await {
            Ok(directory) if !directory.is_empty() => {
                r.system_prompt(ContextLayer::AlwaysPresent, directory);
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "skills: failed to build always-present directory");
            }
        }

        // Publish AgentTemplateService on the bus so the spawn tool can
        // instantiate sub-agent templates (skill injection into the child's
        // system prompt). The MCP manager is `disconnected()` — template
        // `mcp_servers` register but expose no tools until a real MCP
        // transport + shared manager is wired (transport is a stub today).
        let template_service = Arc::new(AgentTemplateService::new(
            self.store.clone(),
            self.injector.clone(),
            Arc::new(McpManager::disconnected()),
            GlobalSkillConfig::default(),
        ));
        r.provide::<AgentTemplateService>(template_service);
    }
}
