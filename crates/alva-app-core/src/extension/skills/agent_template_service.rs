// INPUT:  std::collections, std::sync, crate::extension::mcp::runtime, crate::extension::skills::{injector, store, skill_domain}, crate::error
// OUTPUT: AgentTemplateService, AgentTemplateInstance
// POS:    Instantiates AgentTemplate into runtime config: merged skills, system prompt, MCP servers, and tool whitelist.
use std::collections::HashSet;
use std::sync::Arc;

use crate::{
    extension::mcp::runtime::McpManager,
    extension::skills::{
        injector::SkillInjector,
        store::SkillStore,
    },
    extension::skills::skill_domain::{
        agent_template::{AgentTemplate, GlobalSkillConfig},
        mcp::McpServerConfig,
        skill_config::SkillRef,
    },
    error::SkillError,
};

/// From AgentTemplate, instantiate all runtime config needed:
/// 1. Merge GlobalSkillConfig + AgentTemplate::skills
/// 2. Build system prompt injection block
/// 3. Collect MCP Server list to connect
/// 4. Export MCP Tool name list (for EngineBuilder::allowed_tools filtering)
pub struct AgentTemplateService {
    skill_store: Arc<SkillStore>,
    injector: Arc<SkillInjector>,
    mcp_manager: Arc<McpManager>,
    global_config: GlobalSkillConfig,
}

/// Template instantiation result
pub struct AgentTemplateInstance {
    /// Complete system prompt (base + skill injection)
    pub system_prompt: String,
    /// MCP Server IDs to activate for this instance
    pub mcp_server_ids: Vec<String>,
    /// Available tool names (including MCP tools), for AgentConfig::allowed_tools
    pub allowed_tools: Option<Vec<String>>,
}

impl AgentTemplateService {
    pub fn new(
        skill_store: Arc<SkillStore>,
        injector: Arc<SkillInjector>,
        mcp_manager: Arc<McpManager>,
        global_config: GlobalSkillConfig,
    ) -> Self {
        Self {
            skill_store,
            injector,
            mcp_manager,
            global_config,
        }
    }

    /// Instantiate AgentTemplate, return runtime config
    pub async fn instantiate(
        &self,
        template: &AgentTemplate,
    ) -> Result<AgentTemplateInstance, SkillError> {
        // 1. Merge Skill reference list (global baseline + template include - template exclude)
        let skill_refs = self.merge_skill_refs(template);

        // 2. Resolve Skill instances from SkillStore
        let all_skills = self.skill_store.list().await;

        // 3. Build system prompt
        let skill_injection = self
            .injector
            .build_injection(&skill_refs, &all_skills)
            .await?;

        let system_prompt = if skill_injection.is_empty() {
            template.system_prompt_base.clone()
        } else {
            format!("{}\n\n{}", template.system_prompt_base, skill_injection)
        };

        // 4. Merge MCP Server list
        let mcp_server_ids = self.merge_mcp_servers(template).await;

        // 5. Build tool whitelist
        let allowed_tools = self
            .build_allowed_tools(template, &mcp_server_ids)
            .await;

        Ok(AgentTemplateInstance {
            system_prompt,
            mcp_server_ids,
            allowed_tools,
        })
    }

    /// Merge Skill references: global baseline + include - exclude
    fn merge_skill_refs(&self, template: &AgentTemplate) -> Vec<SkillRef> {
        let mut refs: Vec<SkillRef> = vec![];

        if template.skills.inherit_global {
            refs.extend(self.global_config.enabled_skills.clone());
        }

        // Append template-level include
        refs.extend(template.skills.include.clone());

        // Apply template-level exclude
        let exclude_set: HashSet<&str> =
            template.skills.exclude.iter().map(|s| s.as_str()).collect();
        refs.retain(|r| !exclude_set.contains(r.name.as_str()));

        // Apply template-level default injection policy (where SkillRef has no explicit setting)
        for r in &mut refs {
            if r.injection.is_none() {
                r.injection = Some(template.skills.default_injection.clone());
            }
        }

        refs
    }

    /// Merge MCP Server configs and register with McpManager
    async fn merge_mcp_servers(&self, template: &AgentTemplate) -> Vec<String> {
        let mut configs: Vec<McpServerConfig> = vec![];

        if template.mcp_servers.inherit_global {
            configs.extend(self.global_config.mcp_servers.clone());
        }

        configs.extend(template.mcp_servers.include.clone());

        let exclude_set: HashSet<&str> = template
            .mcp_servers
            .exclude
            .iter()
            .map(|s| s.as_str())
            .collect();
        configs.retain(|c| !exclude_set.contains(c.id.as_str()));

        let ids: Vec<String> = configs.iter().map(|c| c.id.clone()).collect();

        // Register with McpManager (idempotent)
        for config in configs {
            self.mcp_manager.register(config).await;
        }

        ids
    }

    /// Build allowed_tools whitelist
    /// MCP tool naming format: `mcp:<server_id>:<tool_name>`
    async fn build_allowed_tools(
        &self,
        template: &AgentTemplate,
        mcp_server_ids: &[String],
    ) -> Option<Vec<String>> {
        if let Some(explicit_tools) = &template.allowed_tools {
            return Some(explicit_tools.clone());
        }

        // If there are MCP Servers, dynamically build list including MCP tools
        // Otherwise return None (unrestricted, use all registered tools)
        if mcp_server_ids.is_empty() {
            return None;
        }

        // Collect currently connected MCP tool names
        let mcp_tools: Vec<String> = self
            .mcp_manager
            .list_all_tools()
            .await
            .iter()
            .filter(|t| mcp_server_ids.contains(&t.server_id))
            .map(|t| format!("mcp:{}:{}", t.server_id, t.tool_name))
            .collect();

        if mcp_tools.is_empty() {
            None
        } else {
            Some(mcp_tools)
        }
    }
}
