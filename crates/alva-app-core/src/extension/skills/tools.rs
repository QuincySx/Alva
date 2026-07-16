// INPUT:  std::sync, alva_agent_extension_builtin::skill_tool, crate::extension::skills::{injector, store, skill_domain}, async_trait
// OUTPUT: SkillService
// POS:    Bridges the app skill store/injector to the builtin unified `skill` tool and REPL direct invocation.

use std::sync::Arc;

use alva_agent_extension_builtin::skill_tool::{SkillRegistry, SkillRegistryError};
use async_trait::async_trait;

use crate::extension::skills::injector::SkillInjector;
use crate::extension::skills::skill_domain::skill_config::{InjectionPolicy, SkillRef};
use crate::extension::skills::store::SkillStore;

/// Real skill registry service shared by the model-facing `skill` tool and
/// harness-side direct invocation paths such as the REPL slash fallback.
pub struct SkillService {
    store: Arc<SkillStore>,
    injector: Arc<SkillInjector>,
}

impl SkillService {
    pub fn new(store: Arc<SkillStore>, injector: Arc<SkillInjector>) -> Self {
        Self { store, injector }
    }

    async fn injection_for(
        &self,
        name: &str,
        args: Option<&str>,
    ) -> Result<String, SkillRegistryError> {
        let skill = self
            .store
            .find_enabled(name)
            .await
            .ok_or_else(|| SkillRegistryError::NotFound(name.to_string()))?;

        // Invocation mode controls discovery only. Once named, every skill is
        // expanded using Explicit injection; a declared allowlist upgrades the
        // existing injection policy to Strict so the restriction is visible in
        // the resulting context block.
        let injection = if skill.meta.allowed_tools.is_some() {
            InjectionPolicy::Strict
        } else {
            InjectionPolicy::Explicit
        };
        let mut rendered = self
            .injector
            .build_injection(
                &[SkillRef {
                    name: skill.meta.name.clone(),
                    injection: Some(injection),
                }],
                &[skill],
            )
            .await
            .map_err(|error| SkillRegistryError::Load(error.to_string()))?;

        if let Some(args) = args.map(str::trim).filter(|args| !args.is_empty()) {
            rendered.push_str("\n\n## Invocation Arguments\n\n");
            rendered.push_str(args);
        }

        Ok(rendered)
    }
}

#[async_trait]
impl SkillRegistry for SkillService {
    async fn invoke(&self, skill: &str, args: Option<&str>) -> Result<String, SkillRegistryError> {
        self.injection_for(skill, args).await
    }

    async fn available_names(&self) -> Vec<String> {
        self.store
            .list()
            .await
            .into_iter()
            .filter(|skill| skill.enabled)
            .map(|skill| skill.meta.name)
            .collect()
    }
}
